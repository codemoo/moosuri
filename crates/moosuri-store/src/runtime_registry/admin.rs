//! Installation roles are independent of private member roles. Session views
//! must arrive from freshly authenticated, identity-verified private IPC, never
//! browser JSON. No credentials or private data are persisted here.
use super::*;
use moosuri_core::{Role, SessionView};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Clone, Serialize, Deserialize)]
pub struct Member {
    pub account_id: String,
    pub tenant_id: String,
    pub volume_id: String,
    pub generation: i64,
    pub email: String,
    pub route_state: String,
    pub role: String,
    pub role_revision: i64,
    pub eligible: bool,
    pub suspended: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invitation_request_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invitation_name: Option<String>,
}
/// A short admission pin, not a bearer/session. Gateway must verify the current
/// private session on every HTTP request before obtaining one.
pub struct AdminSession {
    account: String,
    tenant: String,
    volume: String,
    generation: i64,
    revision: i64,
    issued: Instant,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Promote,
    Demote,
    Suspend,
    Resume,
}
const COLUMNS: &str = "a.account_id,a.tenant_id,a.volume_id,a.generation,a.email,a.route_state,COALESCE(m.role,'member'),COALESCE(m.role_revision,0),COALESCE(m.eligible,0),COALESCE(m.suspended,0),a.request_id,o.result";
const JOIN: &str = " FROM allocations a LEFT JOIN installation_members m ON m.account_id=a.account_id AND m.tenant_id=a.tenant_id AND m.volume_id=a.volume_id AND m.generation=a.generation LEFT JOIN installation_admin_operations o ON o.request_id=a.request_id";
fn decode_member(r: &rusqlite::Row<'_>) -> rusqlite::Result<Member> {
    let mut m = Member {
        account_id: r.get(0)?,
        tenant_id: r.get(1)?,
        volume_id: r.get(2)?,
        generation: r.get(3)?,
        email: r.get(4)?,
        route_state: r.get(5)?,
        role: r.get(6)?,
        role_revision: r.get(7)?,
        eligible: r.get(8)?,
        suspended: r.get(9)?,
        invitation_request_id: None,
        invitation_name: None,
    };
    if m.route_state == "reserved" {
        if let Some(raw) = r.get::<_, Option<String>>(11)? {
            let request: String = r.get(10)?;
            let i: Invitation =
                serde_json::from_str(&raw).map_err(|_| rusqlite::Error::InvalidQuery)?;
            if i.request_id.to_string() != request
                || i.account_id != m.account_id
                || i.tenant_id != m.tenant_id
                || i.volume_id != m.volume_id
                || i.generation != m.generation
                || i.email != m.email
            {
                return Err(rusqlite::Error::InvalidQuery);
            }
            m.invitation_request_id = Some(i.request_id);
            m.invitation_name = Some(i.name);
        }
    }
    Ok(m)
}
fn member(c: &Connection, id: &str) -> Result<Member> {
    c.query_row(
        &format!("SELECT {COLUMNS}{JOIN} WHERE a.account_id=?1"),
        [id],
        decode_member,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}
fn exact(m: &Member, a: &Allocation) -> bool {
    m.account_id == a.account_id
        && m.tenant_id == a.tenant_id
        && m.volume_id == a.volume_id
        && m.generation == a.generation
}
fn usable(m: &Member) -> bool {
    m.route_state == "ready" && m.role == "admin" && m.eligible && !m.suspended
}
fn count(c: &Connection) -> Result<i64> {
    Ok(c.query_row("SELECT count(*) FROM installation_members m JOIN allocations a ON a.account_id=m.account_id AND a.tenant_id=m.tenant_id AND a.volume_id=m.volume_id AND a.generation=m.generation WHERE a.route_state='ready' AND m.role='admin' AND m.eligible=1 AND m.suspended=0",[],|r|r.get(0))?)
}
pub(super) fn guard_fence(c: &Connection, a: &Allocation) -> Result<()> {
    let m = member(c, &a.account_id)?;
    if exact(&m, a) && usable(&m) && count(c)? <= 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
pub(super) fn unsuspended(c: &Connection, a: &Allocation) -> Result<()> {
    let m = member(c, &a.account_id)?;
    if !exact(&m, a) || m.suspended {
        return Err(StoreError::Forbidden);
    }
    Ok(())
}
fn observe(c: &Connection, a: &Allocation, v: &SessionView) -> Result<Member> {
    let m = member(c, &a.account_id)?;
    if !exact(&m, a)
        || m.suspended
        || m.route_state != "ready"
        || v.user.id != a.account_id
        || v.user.email != m.email
        || v.user.role != Role::Member
    {
        return Err(StoreError::Forbidden);
    }
    let eligible = !v.password_change_required && !v.user.must_change_password;
    if m.role_revision == 0 {
        c.execute("INSERT INTO installation_members VALUES(?1,?2,?3,?4,'member',1,?5,0) ON CONFLICT(account_id) DO UPDATE SET tenant_id=excluded.tenant_id,volume_id=excluded.volume_id,generation=excluded.generation,role='member',role_revision=installation_members.role_revision+1,eligible=excluded.eligible,suspended=0",params![a.account_id,a.tenant_id,a.volume_id,a.generation,eligible])?;
    } else if m.eligible != eligible {
        c.execute("UPDATE installation_members SET eligible=?2,role_revision=role_revision+1 WHERE account_id=?1",params![a.account_id,eligible])?;
    }
    member(c, &a.account_id)
}
pub fn observe_session(root: &Path, a: &Allocation, view: &SessionView) -> Result<Member> {
    let (_lock, mut c) = open(root, false)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let m = observe(&tx, a, view)?;
    tx.commit()?;
    Ok(m)
}
pub fn checked_session(root: &Path, a: &Allocation, view: &SessionView) -> Result<AdminSession> {
    let m = observe_session(root, a, view)?;
    if !usable(&m) {
        return Err(StoreError::Forbidden);
    }
    Ok(AdminSession {
        account: m.account_id,
        tenant: m.tenant_id,
        volume: m.volume_id,
        generation: m.generation,
        revision: m.role_revision,
        issued: Instant::now(),
    })
}
fn check(c: &Connection, s: &AdminSession) -> Result<()> {
    let m = member(c, &s.account)?;
    if s.issued.elapsed() > Duration::from_secs(5)
        || !usable(&m)
        || m.tenant_id != s.tenant
        || m.volume_id != s.volume
        || m.generation != s.generation
        || m.role_revision != s.revision
    {
        return Err(StoreError::Forbidden);
    }
    Ok(())
}
pub fn list_members(root: &Path, s: &AdminSession) -> Result<Vec<Member>> {
    let (_lock, c) = open(root, false)?;
    check(&c, s)?;
    let mut q = c.prepare(&format!(
        "SELECT {COLUMNS}{JOIN} ORDER BY a.email,a.account_id"
    ))?;
    Ok(q.query_map([], decode_member)?
        .collect::<std::result::Result<Vec<_>, _>>()?)
}
/// Safe local operator metadata, never sufficient to authorize a web action.
pub fn status(root: &Path, account: Uuid) -> Result<Member> {
    let (_lock, c) = open(root, false)?;
    member(&c, &account.to_string())
}

/// One-time installer authority. Exact replay never repromotes a demoted admin.
pub fn bootstrap(root: &Path, request: Uuid, a: &Allocation, v: &SessionView) -> Result<Member> {
    if request.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    let (_lock, mut c) = open(root, false)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if tx.query_row("SELECT EXISTS(SELECT 1 FROM installation_admin_operations WHERE request_id=?1) OR EXISTS(SELECT 1 FROM allocations WHERE request_id=?1)",[request.to_string()],|r|r.get::<_,bool>(0))? {return Err(StoreError::Conflict);}
    let m = observe(&tx, a, v)?;
    if !m.eligible || m.suspended {
        return Err(StoreError::Forbidden);
    }
    let prior:Option<(String,String,String,String,i64)>=tx.query_row("SELECT request_id,account_id,tenant_id,volume_id,generation FROM installation_admin_bootstrap WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    if let Some(p) = prior {
        if p != (
            request.to_string(),
            a.account_id.clone(),
            a.tenant_id.clone(),
            a.volume_id.clone(),
            a.generation,
        ) {
            return Err(StoreError::Conflict);
        }
        tx.commit()?;
        return Ok(m);
    }
    // Never bootstrap a partially imported or manually inconsistent role table.
    if tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM installation_members WHERE role='admin')",
        [],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(StoreError::Conflict);
    }
    tx.execute(
        "INSERT INTO installation_admin_bootstrap VALUES(1,?1,?2,?3,?4,?5)",
        params![
            request.to_string(),
            a.account_id,
            a.tenant_id,
            a.volume_id,
            a.generation
        ],
    )?;
    tx.execute("UPDATE installation_members SET role='admin',role_revision=role_revision+1 WHERE account_id=?1",[&a.account_id])?;
    let m = member(&tx, &a.account_id)?;
    tx.commit()?;
    Ok(m)
}
pub fn change_member(
    root: &Path,
    s: &AdminSession,
    request: Uuid,
    target: Uuid,
    expected_generation: i64,
    expected_role_revision: i64,
    change: Change,
) -> Result<Member> {
    if request.is_nil() || target.is_nil() || expected_generation <= 0 || expected_role_revision < 0
    {
        return Err(StoreError::InvalidInput);
    }
    let (_lock, mut c) = open(root, false)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check(&tx, s)?;
    if request_used_for_allocation_or_bootstrap(&tx, request)? {
        return Err(StoreError::Conflict);
    }

    let fingerprint = serde_json::to_string(&(
        &s.account,
        &s.tenant,
        &s.volume,
        s.generation,
        s.revision,
        target,
        expected_generation,
        expected_role_revision,
        change,
    ))
    .map_err(|_| StoreError::InvalidInput)?;
    let previous: Option<(String, String)> = tx
        .query_row(
            "SELECT fingerprint,result FROM installation_admin_operations WHERE request_id=?1",
            [request.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((f, result)) = previous {
        if f != fingerprint {
            return Err(StoreError::Conflict);
        }
        // Historical role revision is retained; never reuse an outcome across generations.
        let prior: Member = serde_json::from_str(&result).map_err(|_| StoreError::Corrupt)?;
        let current = member(&tx, &target.to_string())?;
        if current.generation != expected_generation
            || prior.account_id != target.to_string()
            || prior.generation != expected_generation
        {
            return Err(StoreError::Conflict);
        }
        return Ok(prior);
    }
    let m = member(&tx, &target.to_string())?;
    if m.generation != expected_generation || m.role_revision != expected_role_revision {
        return Err(StoreError::Conflict);
    }
    let (role, suspended) = match change {
        Change::Promote => {
            if m.route_state != "ready" || !m.eligible || m.suspended {
                return Err(StoreError::Forbidden);
            }
            ("admin", m.suspended)
        }
        Change::Demote => ("member", m.suspended),
        Change::Suspend => (m.role.as_str(), true),
        Change::Resume => (m.role.as_str(), false),
    };
    if usable(&m) && (role != "admin" || suspended) && count(&tx)? <= 1 {
        return Err(StoreError::Conflict);
    }
    if m.role_revision == 0 {
        tx.execute(
            "INSERT INTO installation_members VALUES(?1,?2,?3,?4,?5,1,0,?6)",
            params![
                m.account_id,
                m.tenant_id,
                m.volume_id,
                m.generation,
                role,
                suspended
            ],
        )?;
    } else {
        tx.execute("UPDATE installation_members SET role=?2,suspended=?3,role_revision=role_revision+1 WHERE account_id=?1",params![m.account_id,role,suspended])?;
    }
    let result = member(&tx, &m.account_id)?;
    let raw = serde_json::to_string(&result).map_err(|_| StoreError::Corrupt)?;
    tx.execute(
        "INSERT INTO installation_admin_operations VALUES(?1,?2,?3)",
        params![request.to_string(), fingerprint, raw],
    )?;
    tx.commit()?;
    Ok(result)
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Invitation {
    pub request_id: Uuid,
    pub account_id: String,
    pub tenant_id: String,
    pub volume_id: String,
    pub generation: i64,
    pub email: String,
    pub name: String,
}
/// Only reserves control metadata; the operator must provision/fulfill separately.
pub fn reserve_invitation(
    root: &Path,
    s: &AdminSession,
    request: Uuid,
    name: String,
    email: String,
) -> Result<Invitation> {
    if request.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    let name = crate::name_valid(name, 200)?;
    let email = email_valid(email)?;
    let (_lock, mut c) = open(root, false)?;
    let tx = c.transaction_with_behavior(TransactionBehavior::Immediate)?;
    check(&tx, s)?;
    if tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM installation_admin_bootstrap WHERE request_id=?1)",
        [request.to_string()],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(StoreError::Conflict);
    }

    let fingerprint = serde_json::to_string(&(
        "invitation",
        &s.account,
        &s.tenant,
        &s.volume,
        s.generation,
        s.revision,
        &name,
        &email,
    ))
    .map_err(|_| StoreError::InvalidInput)?;
    let previous: Option<(String, String)> = tx
        .query_row(
            "SELECT fingerprint,result FROM installation_admin_operations WHERE request_id=?1",
            [request.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((f, result)) = previous {
        if f != fingerprint {
            return Err(StoreError::Conflict);
        }
        let prior: Invitation = serde_json::from_str(&result).map_err(|_| StoreError::Corrupt)?;
        let current = member(&tx, &prior.account_id)?;
        if prior.tenant_id != current.tenant_id
            || prior.volume_id != current.volume_id
            || prior.generation != current.generation
            || prior.email != current.email
        {
            return Err(StoreError::Conflict);
        }
        return Ok(prior);
    }
    // A raw operator reservation with the same request is not a web invitation.
    if tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM allocations WHERE request_id=?1)",
        [request.to_string()],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(StoreError::Conflict);
    }
    let a = reserve_in(&tx, request, email.clone())?;
    let invitation = Invitation {
        request_id: request,
        account_id: a.account_id,
        tenant_id: a.tenant_id,
        volume_id: a.volume_id,
        generation: a.generation,
        email,
        name,
    };
    let result = serde_json::to_string(&invitation).map_err(|_| StoreError::Corrupt)?;
    tx.execute(
        "INSERT INTO installation_admin_operations VALUES(?1,?2,?3)",
        params![request.to_string(), fingerprint, result],
    )?;
    tx.commit()?;
    Ok(invitation)
}
/// Operator-only planned invitation metadata. Manual reservations have no plan.
pub fn operator_invitation(root: &Path, request: Uuid) -> Result<Option<Invitation>> {
    let (_lock, c) = open(root, false)?;
    let raw: Option<String> = c
        .query_row(
            "SELECT result FROM installation_admin_operations WHERE request_id=?1",
            [request.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let i: Invitation = serde_json::from_str(&raw).map_err(|_| StoreError::Conflict)?;
    let m = member(&c, &i.account_id)?;
    let allocation_request: String = c.query_row(
        "SELECT request_id FROM allocations WHERE account_id=?1",
        [&i.account_id],
        |r| r.get(0),
    )?;
    if i.request_id != request
        || allocation_request != request.to_string()
        || i.tenant_id != m.tenant_id
        || i.volume_id != m.volume_id
        || i.generation != m.generation
        || i.email != m.email
    {
        return Err(StoreError::Conflict);
    }
    Ok(Some(i))
}
fn request_used_for_allocation_or_bootstrap(c: &Connection, request: Uuid) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM allocations WHERE request_id=?1) OR EXISTS(SELECT 1 FROM installation_admin_bootstrap WHERE request_id=?1)",[request.to_string()],|r|r.get(0))?)
}
pub(super) fn guard_reserve(c: &Connection, request: Uuid, email: &str) -> Result<()> {
    if request.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    if c.query_row(
        "SELECT EXISTS(SELECT 1 FROM installation_admin_bootstrap WHERE request_id=?1)",
        [request.to_string()],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(StoreError::Conflict);
    }
    let previous: Option<String> = c
        .query_row(
            "SELECT result FROM installation_admin_operations WHERE request_id=?1",
            [request.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(raw) = previous {
        let invite: Invitation = serde_json::from_str(&raw).map_err(|_| StoreError::Conflict)?;
        if invite.request_id != request || invite.email != email {
            return Err(StoreError::Conflict);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;
