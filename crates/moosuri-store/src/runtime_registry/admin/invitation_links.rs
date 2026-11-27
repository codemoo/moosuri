//! One-use web links for reserved invitations. Only a SHA-256 digest of the
//! 32-byte capability is stored; no token, password, or session is persisted.
//! Every use revalidates the bound issuer, the exact invitation, and the TTL
//! under the registry directory lock.
use super::*;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

pub const LINK_TTL_SECONDS: i64 = 24 * 60 * 60;

/// Public link state. It never carries the capability or its digest.
#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Link {
    pub invitation: Invitation,
    pub ready: bool,
    pub expires_at: i64,
}

struct Checked {
    link: Link,
    state: String,
    allocation: Allocation,
}

/// Canonical base64url without padding for exactly 32 bytes.
fn digest(token: &str) -> Result<Vec<u8>> {
    if token.len() != 43 {
        return Err(StoreError::InvalidInput);
    }
    let raw = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| StoreError::InvalidInput)?;
    if raw.len() != 32 || URL_SAFE_NO_PAD.encode(&raw) != token {
        return Err(StoreError::InvalidInput);
    }
    Ok(Sha256::digest(&raw).to_vec())
}

fn denied(e: StoreError) -> StoreError {
    match e {
        StoreError::NotFound | StoreError::Corrupt => StoreError::Unauthorized,
        other => other,
    }
}

/// The current web-issued invitation and route state, or `None` when the
/// request is not a live web invitation (first-install, raw CLI, suspended,
/// fenced, or replaced generation).
fn current(c: &Connection, request: &str) -> Result<Option<(Invitation, String)>> {
    let row: Option<(Allocation, String, String, bool, Option<String>, Option<String>)> = c
        .query_row(
            "SELECT a.account_id,a.tenant_id,a.volume_id,a.generation,a.email,a.route_state,
                    COALESCE(m.suspended,0),json_extract(o.fingerprint,'$[0]'),o.result
             FROM allocations a
             LEFT JOIN installation_members m ON m.account_id=a.account_id AND m.tenant_id=a.tenant_id
               AND m.volume_id=a.volume_id AND m.generation=a.generation
             LEFT JOIN installation_admin_operations o ON o.request_id=a.request_id
             WHERE a.request_id=?1",
            [request],
            |r| {
                Ok((
                    decode(r)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .optional()?;
    let Some((a, email, state, suspended, kind, raw)) = row else {
        return Ok(None);
    };
    if suspended || state == "fenced" || kind.as_deref() != Some("invitation") {
        return Ok(None);
    }
    let Some(raw) = raw else { return Ok(None) };
    let Ok(i) = serde_json::from_str::<Invitation>(&raw) else {
        return Ok(None);
    };
    if i.request_id.to_string() != request
        || i.account_id != a.account_id
        || i.tenant_id != a.tenant_id
        || i.volume_id != a.volume_id
        || i.generation != a.generation
        || i.email != email
    {
        return Ok(None);
    }
    Ok(Some((i, state)))
}

fn allocation_of(i: &Invitation) -> Allocation {
    Allocation {
        account_id: i.account_id.clone(),
        tenant_id: i.tenant_id.clone(),
        volume_id: i.volume_id.clone(),
        generation: i.generation,
    }
}

/// Full revalidation of capability, TTL, issuer authority and invitation.
fn validate(c: &Connection, token: &str, now: i64) -> Result<Checked> {
    let digest = digest(token).map_err(|_| StoreError::Unauthorized)?;
    type Row = (
        String,
        Allocation,
        String,
        String,
        String,
        i64,
        i64,
        i64,
        i64,
    );
    let row: Option<Row> = c
        .query_row(
            "SELECT request_id,account_id,tenant_id,volume_id,generation,issuer_account_id,
                    issuer_tenant_id,issuer_volume_id,issuer_generation,issuer_role_revision,expires_at,invitee_epoch
             FROM installation_invitation_links WHERE digest=?1",
            [digest.as_slice()],
            |r| {
                Ok((
                    r.get(0)?,
                    Allocation {
                        account_id: r.get(1)?,
                        tenant_id: r.get(2)?,
                        volume_id: r.get(3)?,
                        generation: r.get(4)?,
                    },
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                    r.get(11)?,
                ))
            },
        )
        .optional()?;
    let Some((
        request,
        bound,
        issuer,
        issuer_tenant,
        issuer_volume,
        issuer_generation,
        revision,
        expires_at,
        invitee_epoch,
    )) = row
    else {
        return Err(StoreError::Unauthorized);
    };
    if expires_at <= now {
        return Err(StoreError::Unauthorized);
    }
    let m = member(c, &issuer).map_err(denied)?;
    if !usable(&m)
        || m.tenant_id != issuer_tenant
        || m.volume_id != issuer_volume
        || m.generation != issuer_generation
        || m.role_revision != revision
    {
        return Err(StoreError::Unauthorized);
    }
    let (invitation, state) = current(c, &request)?.ok_or(StoreError::Unauthorized)?;
    let allocation = allocation_of(&invitation);
    let epoch: i64 = c.query_row(
        "SELECT session_epoch FROM allocations WHERE account_id=?1",
        [&allocation.account_id],
        |r| r.get(0),
    )?;
    if allocation != bound || epoch != invitee_epoch {
        return Err(StoreError::Unauthorized);
    }
    Ok(Checked {
        link: Link {
            invitation,
            ready: state == "ready",
            expires_at,
        },
        state,
        allocation,
    })
}

/// Create or replace the single current link for a reserved web invitation.
pub fn issue(root: &Path, s: &AdminSession, request: Uuid, token: &str) -> Result<Link> {
    issue_at(root, s, request, token, crate::now)
}

fn issue_at(
    root: &Path,
    s: &AdminSession,
    request: Uuid,
    token: &str,
    now: impl FnOnce() -> Result<i64>,
) -> Result<Link> {
    if request.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    let digest = digest(token)?;
    let (_lock, mut db) = open(root, false)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check(&tx, s)?;
    let expires_at = now()?
        .checked_add(LINK_TTL_SECONDS)
        .ok_or(StoreError::InvalidInput)?;
    let id = request.to_string();
    let (invitation, state) = current(&tx, &id)?.ok_or(StoreError::Conflict)?;
    if state != "reserved" {
        return Err(StoreError::Conflict);
    }
    let reused: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM installation_invitation_links WHERE digest=?1 AND request_id<>?2)",
        params![digest, id],
        |r| r.get(0),
    )?;
    if reused {
        return Err(StoreError::Conflict);
    }
    let invitee_epoch: i64 = tx.query_row(
        "SELECT session_epoch FROM allocations WHERE request_id=?1",
        [&id],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO installation_invitation_links(request_id,digest,account_id,tenant_id,volume_id,generation,
            issuer_account_id,issuer_tenant_id,issuer_volume_id,issuer_generation,issuer_role_revision,expires_at,invitee_epoch)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
         ON CONFLICT(request_id) DO UPDATE SET digest=excluded.digest,account_id=excluded.account_id,
            tenant_id=excluded.tenant_id,volume_id=excluded.volume_id,generation=excluded.generation,
            issuer_account_id=excluded.issuer_account_id,issuer_tenant_id=excluded.issuer_tenant_id,
            issuer_volume_id=excluded.issuer_volume_id,issuer_generation=excluded.issuer_generation,
            issuer_role_revision=excluded.issuer_role_revision,expires_at=excluded.expires_at,invitee_epoch=excluded.invitee_epoch",
        params![
            id,
            digest,
            invitation.account_id,
            invitation.tenant_id,
            invitation.volume_id,
            invitation.generation,
            s.account,
            s.tenant,
            s.volume,
            s.generation,
            s.revision,
            expires_at,
            invitee_epoch
        ],
    )?;
    tx.commit()?;
    Ok(Link {
        invitation,
        ready: false,
        expires_at,
    })
}

/// Revalidate a link. `ready` is only a truthful report of a completed redemption.
pub fn inspect(root: &Path, token: &str) -> Result<Link> {
    inspect_at(root, token, crate::now)
}

/// A link-issued invitation must never fall back to the operator/timer
/// activation path, including after expiry or a lost initialization response.
pub fn requires_link(root: &Path, request: Uuid) -> Result<bool> {
    let (_lock, db) = open(root, false)?;
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM installation_invitation_links WHERE request_id=?1)",
        [request.to_string()],
        |r| r.get(0),
    )?)
}

/// Legacy operator initialization cannot race a newly issued web link.
pub fn with_unlinked<T>(
    root: &Path,
    request: Uuid,
    expected: &Allocation,
    action: impl FnOnce() -> T,
) -> Result<T> {
    let (_lock, db) = open(root, false)?;
    let allowed: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM allocations a WHERE request_id=?1 AND account_id=?2 AND tenant_id=?3 AND volume_id=?4 AND generation=?5 AND route_state='reserved' AND NOT EXISTS(SELECT 1 FROM installation_invitation_links l WHERE l.request_id=a.request_id) AND NOT EXISTS(SELECT 1 FROM installation_members m WHERE m.account_id=a.account_id AND m.tenant_id=a.tenant_id AND m.volume_id=a.volume_id AND m.generation=a.generation AND m.suspended=1))",
        params![request.to_string(),expected.account_id,expected.tenant_id,expected.volume_id,expected.generation], |r|r.get(0))?;
    if !allowed {
        return Err(StoreError::Conflict);
    }
    Ok(action())
}

fn inspect_at(root: &Path, token: &str, now: impl FnOnce() -> Result<i64>) -> Result<Link> {
    let (_lock, db) = open(root, false)?;
    Ok(validate(&db, token, now()?)?.link)
}

/// Run a bounded local private-volume action while the registry directory lock
/// pins the link, issuer, and still-reserved invitation. The callback must not
/// touch the registry again or perform network I/O.
pub fn with_pending<T>(
    root: &Path,
    token: &str,
    expected: &Allocation,
    action: impl FnOnce() -> T,
) -> Result<T> {
    with_pending_at(root, token, expected, crate::now, action)
}

fn with_pending_at<T>(
    root: &Path,
    token: &str,
    expected: &Allocation,
    now: impl FnOnce() -> Result<i64>,
    action: impl FnOnce() -> T,
) -> Result<T> {
    let (_lock, db) = open(root, false)?;
    let checked = validate(&db, token, now()?)?;
    if checked.allocation != *expected {
        return Err(StoreError::Conflict);
    }
    if checked.state != "reserved" {
        return Err(StoreError::Conflict);
    }
    Ok(action())
}

/// Mark the exact reserved allocation ready. Exact ready replay succeeds.
pub fn activate(root: &Path, token: &str, expected: &Allocation) -> Result<()> {
    activate_at(root, token, expected, crate::now)
}

fn activate_at(
    root: &Path,
    token: &str,
    expected: &Allocation,
    now: impl FnOnce() -> Result<i64>,
) -> Result<()> {
    let (_lock, mut db) = open(root, false)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let checked = validate(&tx, token, now()?)?;
    if checked.allocation != *expected {
        return Err(StoreError::Conflict);
    }
    match checked.state.as_str() {
        "ready" => return Ok(()),
        "reserved" => {}
        _ => return Err(StoreError::Conflict),
    }
    let changed = tx.execute(
        "UPDATE allocations SET route_state='ready' WHERE request_id=?1 AND account_id=?2 AND tenant_id=?3
         AND volume_id=?4 AND generation=?5 AND route_state='reserved'",
        params![
            checked.link.invitation.request_id.to_string(),
            expected.account_id,
            expected.tenant_id,
            expected.volume_id,
            expected.generation
        ],
    )?;
    if changed != 1 {
        return Err(StoreError::Conflict);
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests;
