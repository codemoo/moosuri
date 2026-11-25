//! Member public-page watch (schema 100): "check this public page every hour and tell me only when it
//! changes".
//!
//! * **Grant** – one explicit request per watch, pinned to the owner (tenant/account/volume/generation),
//!   the account auth epoch, the gateway host epoch, the original thread, the exact public `https://` URL
//!   (and its host), the cadence and the end date. A password change or a newer host epoch revokes it.
//! * **Slots** – the primary key of the check table is the dedup key. A slot is claimed (with a fresh
//!   disposable workspace id) and `next_due` is advanced in one transaction *before* any browser IO. A
//!   restart, or a claim older than [`STALE_CLAIM`], becomes `unknown`; it is never read twice and never
//!   alerts twice. The reader then only ever *opens* that fresh workspace and *stops* it. A human's live
//!   workspace is never addressed: ids are minted here and checked against every browser workspace id.
//! * **Reading** – through the existing verified member browser broker (see the app's `public_watch`
//!   reader). No generic network client, no member cookies, no model, no actions other than read/close.
//! * **Change** – a content watch compares verified observations with its initial baseline. A price
//!   watch compares a single displayed KRW price on the pinned page with its stored condition and
//!   alerts on entry, including an already qualifying first read. Failed reads keep the last state.
//! * **Evidence** – only a SHA-256 and a <=200 character excerpt, encrypted with the provider key and shown
//!   only while the grant is live. A price notice includes the observed amount and the requested limit.
//!
//! ## Narrow extension point
//! The module exposes four timer calls ([`Store::public_watch_orphans`], [`Store::ack_public_watch_workspace`],
//! [`Store::claim_due_public_watches`], [`Store::settle_public_watch`]), three member calls (create, list,
//! stop) and two hooks (`restart`, `notice_current`). Reading is behind the server's `PublicReads` trait.
use super::*;

mod price;
mod types;
pub use types::*;

/// Active watches per member. Stopped, expired and revoked ones do not count.
const ACTIVE_LIMIT: i64 = 4;
/// Slots claimed per timer tick; the rest wait for the next tick.
const BATCH: i64 = 2;
/// A claim that never settled (crash, lost task) is `unknown` after this long.
const STALE_CLAIM: i64 = 600;
const KEEP_CHECKS: i64 = 30;
const RECENT: i64 = 10;
const LIST_LIMIT: i64 = 30;
/// Visible text considered per read. A change beyond this prefix is not seen.
const MAX_TEXT: usize = 4000;
const EXCERPT: usize = 200;
/// Allowed clock skew between the broker's observation time and this store.
const SKEW: i64 = 60;

// ---------------------------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------------------------

fn canonical(value: &str) -> Result<()> {
    Uuid::parse_str(value)
        .ok()
        .filter(|id| !id.is_nil() && id.to_string() == value)
        .map(|_| ())
        .ok_or(StoreError::InvalidInput)
}

/// Host names that are never a public page, whatever DNS says.
fn private_name(host: &str) -> bool {
    const SUFFIXES: [&str; 8] = [
        "localhost",
        "local",
        "internal",
        "intranet",
        "lan",
        "home",
        "corp",
        "localdomain",
    ];
    !host.contains('.')
        || host.ends_with('.')
        || SUFFIXES
            .iter()
            .any(|s| host == *s || host.ends_with(&format!(".{s}")))
        // A final label without a letter is a (shorthand) number, not a registrable name.
        || host.rsplit('.').next().is_none_or(|tld| !tld.chars().any(|c| c.is_ascii_alphabetic()))
}

/// The canonical explicit public page address and its pinned host.
/// `https` only, a DNS name (no IP literal), default port, no credentials, query or fragment.
pub(crate) fn public_url(raw: &str) -> Result<(String, String)> {
    if raw.is_empty()
        || raw.len() > 2048
        || raw.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(StoreError::InvalidInput);
    }
    let url = url::Url::parse(raw).map_err(|_| StoreError::InvalidInput)?;
    let Some(url::Host::Domain(host)) = url.host() else {
        return Err(StoreError::InvalidInput);
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || private_name(host)
    {
        return Err(StoreError::InvalidInput);
    }
    Ok((url.as_str().to_owned(), host.to_owned()))
}

/// The observed address must still be an `https` page on the pinned host.
fn same_host(pinned: &str, observed: &str) -> bool {
    url::Url::parse(observed).is_ok_and(|u| {
        u.scheme() == "https"
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
            && matches!(u.host(), Some(url::Host::Domain(h)) if h.eq_ignore_ascii_case(pinned))
    })
}

/// Visible text only: control characters dropped, whitespace collapsed, bounded.
fn normalize(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

fn excerpt(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = chars.by_ref().take(EXCERPT).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Identity of the observed content: the pinned address and the bounded normalized text.
fn fingerprint(url: &str, text: &str) -> String {
    let mut input = b"moosuri/public-watch/v1\0".to_vec();
    input.extend_from_slice(url.as_bytes());
    input.push(0);
    input.extend_from_slice(text.as_bytes());
    hex(Sha256::digest(&input).as_slice())
}

/// Ordinary requests keep their original digest byte for byte (replays stay valid); a price condition
/// is part of the identity of a request, so the same id with another condition conflicts.
fn request_digest(r: &PublicWatchRequest, url: &str) -> Result<Vec<u8>> {
    let bytes = match r.price_condition {
        None => serde_json::to_vec(&(
            "moosuri/public-watch/request/v1",
            &r.thread_id,
            &r.message,
            url,
            r.every_seconds,
            r.days,
        )),
        Some(p) => serde_json::to_vec(&(
            "moosuri/public-watch/request/price/v1",
            &r.thread_id,
            &r.message,
            url,
            r.every_seconds,
            r.days,
            p.won,
            p.inclusive,
        )),
    }
    .map_err(|_| StoreError::InvalidInput)?;
    Ok(Sha256::digest(bytes).to_vec())
}

/// Band state of a price watch, kept in the `fingerprint` column (same atomic commit as the check).
fn band(url: &str, inside: bool) -> String {
    fingerprint(
        url,
        if inside {
            "price-band:in"
        } else {
            "price-band:out"
        },
    )
}

fn condition_text(p: PublicWatchPrice) -> String {
    format!(
        "{} {}",
        price::display(p.won),
        if p.inclusive { "이하" } else { "미만" }
    )
}

fn notice_id(watch: &str, due: i64) -> String {
    let mut input = b"moosuri/public-watch/notice/v1\0".to_vec();
    input.extend_from_slice(watch.as_bytes());
    input.extend_from_slice(&due.to_be_bytes());
    let hash = Sha256::digest(&input);
    let mut id: [u8; 16] = hash[..16].try_into().unwrap_or([1; 16]);
    id[6] = (id[6] & 15) | 0x50;
    id[8] = (id[8] & 63) | 0x80;
    Uuid::from_bytes(id).to_string()
}

fn aad(owner: VolumeOwner, watch: &str, host: &str, due: i64) -> Vec<u8> {
    format!(
        "moosuri/public-watch/v1/{}/{}/{}/{}/{watch}/{host}/{due}",
        owner.tenant, owner.account, owner.volume, owner.generation
    )
    .into_bytes()
}

// ---------------------------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------------------------

struct Watch {
    id: String,
    auth_epoch: i64,
    thread: String,
    url: String,
    host: String,
    every: i64,
    days: i64,
    state: String,
    revision: i64,
    expires: i64,
    next_due: Option<i64>,
    digest: Vec<u8>,
    fingerprint: Option<String>,
    last_due: Option<i64>,
    host_epoch: i64,
    price: Option<PublicWatchPrice>,
}

const WATCH: &str =
    "SELECT w.id,w.auth_epoch,w.thread_id,w.url,w.host,w.every_seconds,w.days,w.state,w.end_reason,
 w.revision,w.expires_at,w.next_due,w.request_digest,w.fingerprint,w.last_due,w.host_epoch,
 w.price_won,w.price_inclusive FROM member_public_watches w";
const OWNED: &str = "w.tenant_id=?1 AND w.account_id=?2 AND w.volume_id=?3 AND w.generation=?4";
const AUTHORIZED: &str = "EXISTS(SELECT 1 FROM accounts a WHERE a.id=w.account_id AND a.tenant_id=w.tenant_id
    AND a.auth_epoch=w.auth_epoch AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)";
/// The same ownership test for rows of the check table.
const OWNED_CHECKS: &str =
    "tenant_id=?1 AND account_id=?2 AND watch_id IN(SELECT id FROM member_public_watches
    WHERE tenant_id=?1 AND account_id=?2 AND volume_id=?3 AND generation=?4)";

fn watch_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Watch> {
    Ok(Watch {
        id: r.get(0)?,
        auth_epoch: r.get(1)?,
        thread: r.get(2)?,
        url: r.get(3)?,
        host: r.get(4)?,
        every: r.get(5)?,
        days: r.get(6)?,
        state: r.get(7)?,
        revision: r.get(9)?,
        expires: r.get(10)?,
        next_due: r.get(11)?,
        digest: r.get(12)?,
        fingerprint: r.get(13)?,
        last_due: r.get(14)?,
        host_epoch: r.get(15)?,
        price: match (r.get::<_, Option<i64>>(16)?, r.get::<_, Option<bool>>(17)?) {
            (None, None) => None,
            (Some(won), Some(inclusive)) if (1..=price::MAX_WON).contains(&won) => {
                Some(PublicWatchPrice { won, inclusive })
            }
            _ => return Err(rusqlite::Error::InvalidQuery),
        },
    })
}

fn own(owner: VolumeOwner) -> (String, String, String, i64) {
    (
        owner.tenant.to_string(),
        owner.account.to_string(),
        owner.volume.to_string(),
        owner.generation as i64,
    )
}

fn find(c: &Connection, owner: VolumeOwner, id: &str) -> Result<Option<Watch>> {
    let (t, a, v, g) = own(owner);
    Ok(c.query_row(
        &format!("{WATCH} WHERE {OWNED} AND w.id=?5"),
        params![t, a, v, g, id],
        watch_row,
    )
    .optional()?)
}

fn load(c: &Connection, owner: VolumeOwner, id: &str) -> Result<Watch> {
    find(c, owner, id)?.ok_or(StoreError::NotFound)
}

fn authorized(c: &Connection, tenant: &str, account: &str, epoch: i64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts a WHERE a.id=?2 AND a.tenant_id=?1 AND a.auth_epoch=?3
            AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)",
        params![tenant, account, epoch],
        |r| r.get(0),
    )?)
}

// ---------------------------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------------------------

fn suppress_push(c: &Connection, owner: VolumeOwner, id: &str, time: i64) -> Result<()> {
    let t = owner.tenant.to_string();
    c.execute(
        "UPDATE local_notifications SET push_suppressed=1 WHERE tenant_id=?1 AND schedule_id=?2 AND push_suppressed=0",
        params![t, id],
    )?;
    c.execute(
        "UPDATE push_queue SET state='suppressed',nonce=NULL,deadline=NULL,finished_at=?3,reason='public_watch_stopped'
         WHERE tenant_id=?1 AND state IN('queued','claimed')
           AND notification_id IN(SELECT id FROM local_notifications WHERE tenant_id=?1 AND schedule_id=?2)",
        params![t, id, time],
    )?;
    Ok(())
}

/// Drops the private excerpts. Final slot results are `withdrawn`; unsettled claims are fenced too.
fn purge(c: &Connection, owner: VolumeOwner, id: &str) -> Result<()> {
    let (t, a, _, _) = own(owner);
    c.execute(
        "UPDATE member_public_watch_checks SET state='withdrawn',cipher=NULL,fingerprint=NULL
         WHERE tenant_id=?1 AND account_id=?2 AND watch_id=?3 AND (cipher IS NOT NULL OR state='claimed')",
        params![t, a, id],
    )?;
    Ok(())
}

/// Final state for an active watch. Stopping or revoking also cancels undelivered pushes;
/// revoking also drops private excerpts.
fn end(
    c: &Connection,
    owner: VolumeOwner,
    id: &str,
    state: &str,
    reason: &str,
    time: i64,
) -> Result<()> {
    let (t, a, _, _) = own(owner);
    let changed = c.execute(
        "UPDATE member_public_watches SET state=?4,end_reason=?5,next_due=NULL,ended_at=?6,revision=revision+1
         WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='active'",
        params![t, a, id, state, reason, time],
    )?;
    if changed == 1 && matches!(state, "stopped" | "revoked") {
        suppress_push(c, owner, id, time)?;
    }
    if state == "revoked" {
        purge(c, owner, id)?;
    }
    Ok(())
}

fn advance(c: &Connection, owner: VolumeOwner, w: &Watch, next: i64) -> Result<()> {
    let (t, a, _, _) = own(owner);
    c.execute(
        "UPDATE member_public_watches SET next_due=?4 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='active'",
        params![t, a, w.id, next],
    )?;
    Ok(())
}

/// (latest passed slot, passed slots folded into it, first slot after `time`).
fn occurrence(w: &Watch, time: i64) -> Result<(i64, i64, i64)> {
    let first = w.next_due.ok_or(StoreError::Corrupt)?;
    if w.every <= 0 {
        return Err(StoreError::Corrupt);
    }
    let n = ((time - first) / w.every).max(0);
    let slot = first + n * w.every;
    Ok((slot, n, slot + w.every))
}

/// A workspace id that no browser workspace, human or disposable, has ever used.
fn fresh_workspace(c: &Connection) -> Result<Uuid> {
    for _ in 0..4 {
        let id = Uuid::new_v4();
        let taken: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM personal_browser_workers WHERE workspace_id=?1)
                 OR EXISTS(SELECT 1 FROM personal_browser_jobs WHERE json_extract(view,'$.id')=?1)
                 OR EXISTS(SELECT 1 FROM member_public_watch_checks WHERE workspace_id=?1)",
            [id.to_string()],
            |r| r.get(0),
        )?;
        if !taken {
            return Ok(id);
        }
    }
    Err(StoreError::Conflict)
}

/// Unsettled claims older than [`STALE_CLAIM`] become `unknown`; their workspace is stopped later.
fn reap(c: &Connection, owner: VolumeOwner, time: i64) -> Result<()> {
    let (t, a, v, g) = own(owner);
    c.execute(
        &format!(
            "UPDATE member_public_watch_checks SET state='unknown',cipher=NULL,finished_at=?5
             WHERE state='claimed' AND claimed_at<?6 AND {OWNED_CHECKS}"
        ),
        params![t, a, v, g, time, time - STALE_CLAIM],
    )?;
    Ok(())
}

/// Process start: nothing claimed before the restart is still being read.
pub(crate) fn restart(c: &Connection) -> Result<()> {
    c.execute(
        "UPDATE member_public_watch_checks SET state='unknown',cipher=NULL,finished_at=unixepoch() WHERE state='claimed'",
        [],
    )?;
    Ok(())
}

/// Live-authority check for a watch notification (inbox and push dispatch).
/// `None`: not a public-watch notice.
pub(crate) fn notice_current(c: &Connection, tenant: &str, id: &str) -> Result<Option<bool>> {
    Ok(c.query_row(
        "SELECT w.state!='revoked' AND EXISTS(SELECT 1 FROM accounts a WHERE a.id=w.account_id AND a.tenant_id=w.tenant_id
            AND a.auth_epoch=w.auth_epoch AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)
         FROM local_notifications n JOIN member_public_watches w ON w.tenant_id=n.tenant_id AND w.id=n.schedule_id
         WHERE n.tenant_id=?1 AND n.id=?2",
        params![tenant, id],
        |r| r.get::<_, bool>(0),
    )
    .optional()?)
}

// ---------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------

fn view(
    c: &Connection,
    key: &[u8; 32],
    owner: VolumeOwner,
    w: &Watch,
    time: i64,
) -> Result<PublicWatchView> {
    let (t, a, _, _) = own(owner);
    let evidence_live = w.state == "active" && w.expires > time;
    let rows = c
        .prepare(
            "SELECT due_at,state,cipher FROM member_public_watch_checks
             WHERE tenant_id=?1 AND account_id=?2 AND watch_id=?3 ORDER BY due_at DESC LIMIT ?4",
        )?
        .query_map(params![t, a, w.id, RECENT], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<Vec<u8>>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut checks = Vec::new();
    for (due_at, state, cipher) in rows {
        let excerpt = match cipher {
            Some(blob) if evidence_live => {
                let bytes = providers::decrypt(key, &blob, &aad(owner, &w.id, &w.host, due_at))?;
                Some(
                    std::str::from_utf8(&bytes)
                        .map_err(|_| StoreError::Corrupt)?
                        .to_owned(),
                )
            }
            _ => None,
        };
        checks.push(PublicWatchCheck {
            due_at,
            state,
            excerpt,
        });
    }
    // A lapsed expiry is reported even before the timer has recorded it.
    let lapsed = w.state == "active" && w.expires <= time;
    Ok(PublicWatchView {
        id: w.id.clone(),
        thread_id: w.thread.clone(),
        url: w.url.clone(),
        every_seconds: w.every,
        days: w.days,
        state: if lapsed {
            "expired".into()
        } else {
            w.state.clone()
        },
        revision: w.revision,
        price_condition: w.price,
        checks,
    })
}

fn current_grant(
    c: &Connection,
    owner: VolumeOwner,
    user: &UserContext,
    host_epoch: i64,
    w: Watch,
    time: i64,
) -> Result<Watch> {
    if host_epoch < w.host_epoch {
        return Err(StoreError::Forbidden);
    }
    chat::thread_exists(c, user, &w.thread)?;
    if w.host_epoch != host_epoch || w.auth_epoch != user.epoch {
        end(c, owner, &w.id, "revoked", "session_changed", time)?;
        return load(c, owner, &w.id);
    }
    Ok(w)
}

impl Store {
    /// Creates the explicit grant, or replays the identical earlier request. The first check (the
    /// baseline) is due at the next timer tick; it is never taken inside this request.
    pub async fn create_public_watch(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        request: PublicWatchRequest,
    ) -> Result<PublicWatchView> {
        canonical(&request.request_id)?;
        canonical(&request.thread_id)?;
        if request.message.trim().is_empty()
            || request.message.len() > 8192
            || !matches!(request.every_seconds, 1800 | 3600 | 86400)
            || !(1..=90).contains(&request.days)
        {
            return Err(StoreError::InvalidInput);
        }
        if request.price_condition.is_some_and(|p| !p.valid()) {
            return Err(StoreError::InvalidInput);
        }
        let (url, host) = public_url(&request.url)?;
        let digest = request_digest(&request, &url)?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            let time = now()?;
            if let Some(old) = find(&tx, owner, &request.request_id)? {
                if old.digest != digest {
                    return Err(StoreError::Conflict);
                }
                let current = current_grant(&tx, owner, &user, host_epoch, old, time)?;
                let out = view(&tx, &db.provider_key, owner, &current, time)?;
                tx.commit()?;
                return Ok(out);
            }
            chat::thread_exists(&tx, &user, &request.thread_id)?;
            let (t, a, v, g) = own(owner);
            let active: i64 = tx.query_row(
                &format!("SELECT count(*) FROM member_public_watches w WHERE {OWNED} AND w.state='active' AND w.expires_at>?5"),
                params![t, a, v, g, time],
                |r| r.get(0),
            )?;
            if active >= ACTIVE_LIMIT {
                return Err(StoreError::BudgetExhausted);
            }
            tx.execute(
                "INSERT INTO member_public_watches(tenant_id,account_id,id,volume_id,generation,auth_epoch,host_epoch,thread_id,
                    url,host,every_seconds,days,state,revision,created_at,expires_at,next_due,request_digest,
                    price_won,price_inclusive)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'active',1,?13,?14,?13,?15,?16,?17)",
                params![
                    t,
                    a,
                    request.request_id,
                    v,
                    g,
                    user.epoch,
                    host_epoch,
                    request.thread_id,
                    url,
                    host,
                    request.every_seconds,
                    request.days,
                    time,
                    time + request.days * 86400,
                    digest,
                    request.price_condition.map(|p| p.won),
                    request.price_condition.map(|p| p.inclusive)
                ],
            )?;
            member_work_turn(
                &tx,
                &user,
                &request.thread_id,
                &Uuid::new_v4().to_string(),
                &request.request_id,
                &request.message,
                "member-public-watch",
                {
                    let every = match request.every_seconds {
                        1800 => "30분마다",
                        3600 => "1시간마다",
                        _ => "하루마다",
                    };
                    match request.price_condition {
                        None => format!(
                            "{host} 페이지를 {every} {}일 동안 확인하고, 보이는 내용이 바뀔 때만 알려드릴게요. 로그인 없이 공개 페이지를 읽기만 해요. 확인 기록은 이 대화의 감시 기록에서 볼 수 있어요.",
                            request.days
                        ),
                        Some(p) => format!(
                            "{host} 페이지를 {every} {}일 동안 확인하고, 표시된 객실·상품 가격이 {}일 때 알려드릴게요. 가격이 분명하지 않으면 추측하지 않고 확인 실패로 기록해요. 로그인 없이 공개 페이지를 읽기만 해요. 확인 기록은 이 대화의 감시 기록에서 볼 수 있어요.",
                            request.days,
                            condition_text(p)
                        ),
                    }
                },
            )?;
            let out = view(&tx, &db.provider_key, owner, &load(&tx, owner, &request.request_id)?, time)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    /// The member's watches for one conversation, active ones first.
    pub async fn public_watches(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        thread: Uuid,
    ) -> Result<PublicWatchPage> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            let (t, a, v, g) = own(owner);
            let rows = tx
                .prepare(&format!(
                    "{WATCH} WHERE {OWNED} AND w.thread_id=?6
                     ORDER BY (w.state='active') DESC,w.created_at DESC,w.id LIMIT ?5"
                ))?
                .query_map(
                    params![t, a, v, g, LIST_LIMIT, thread.to_string()],
                    watch_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let time = now()?;
            let mut items = Vec::new();
            for w in rows {
                let current = current_grant(&tx, owner, &user, host_epoch, w, time)?;
                items.push(view(&tx, &db.provider_key, owner, &current, time)?);
            }
            tx.commit()?;
            Ok(PublicWatchPage { items })
        })
        .await
    }

    /// Stops an active watch and cancels its undelivered pushes. A replay, or a request against an
    /// already ended watch, returns its current state; a stale revision on a live one conflicts.
    pub async fn stop_public_watch(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        id: Uuid,
        stop: PublicWatchStop,
    ) -> Result<PublicWatchView> {
        canonical(&stop.request_id)?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            let id = id.to_string();
            let time = now()?;
            let w = current_grant(&tx, owner, &user, host_epoch, load(&tx, owner, &id)?, time)?;
            if w.state == "active" {
                if w.expires <= time {
                    end(&tx, owner, &id, "expired", "end_date", time)?;
                } else {
                    if stop.revision != w.revision {
                        return Err(StoreError::Conflict);
                    }
                    end(&tx, owner, &id, "stopped", "member", time)?;
                    let (t, a, _, _) = own(owner);
                    tx.execute(
                        "UPDATE member_public_watches SET stop_request_id=?4 WHERE tenant_id=?1 AND account_id=?2 AND id=?3",
                        params![t, a, id, stop.request_id],
                    )?;
                }
            }
            let out = view(&tx, &db.provider_key, owner, &load(&tx, owner, &id)?, time)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    // -----------------------------------------------------------------------------------------
    // Timer
    // -----------------------------------------------------------------------------------------

    /// Fence earlier grants even while another timer batch is waiting on the browser.
    pub async fn fence_public_watch_epoch(&self, owner: VolumeOwner, epoch: i64) -> Result<()> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            owner.check_identity(&tx, None)?;
            let (t,a,v,g) = own(owner);
            let ids = tx.prepare(&format!("SELECT w.id FROM member_public_watches w WHERE {OWNED} AND w.state='active' AND w.host_epoch<?5"))?
                .query_map(params![t,a,v,g,epoch], |r| r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            for id in ids { end(&tx, owner, &id, "revoked", "host_changed", now()?)?; }
            tx.commit()?;
            Ok(())
        }).await
    }

    pub async fn public_watch_can_read(&self, lease: PublicWatchLease) -> Result<bool> {
        self.call(move |db| {
            let owner = lease.owner;
            owner.check_identity(&db.conn, None)?;
            let (t,a,v,g) = own(owner);
            Ok(db.conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM member_public_watches w
                JOIN member_public_watch_checks c ON c.tenant_id=w.tenant_id AND c.account_id=w.account_id AND c.watch_id=w.id
                JOIN threads th ON th.tenant_id=w.tenant_id AND th.account_id=w.account_id AND th.id=w.thread_id
                WHERE {OWNED} AND {AUTHORIZED} AND w.state='active' AND w.expires_at>unixepoch() AND th.mission_internal=0
                AND w.id=?5 AND c.due_at=?6 AND c.workspace_id=?7 AND c.state='claimed')"),
                params![t,a,v,g,lease.watch_id.to_string(),lease.due_at,lease.workspace_id.to_string()], |r|r.get(0))?)
        }).await
    }

    /// Timer step 0 (no browser IO). Marks stale claims `unknown`, then returns up to four disposable
    /// workspace ids of finished-but-not-confirmed-closed checks. The caller stops each one and then
    /// calls [`Store::ack_public_watch_workspace`].
    pub async fn public_watch_orphans(&self, owner: VolumeOwner, time: i64) -> Result<Vec<String>> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            owner.check_identity(&tx, None)?;
            reap(&tx, owner, time)?;
            let (t, a, v, g) = own(owner);
            let ids = tx
                .prepare(&format!(
                    "SELECT workspace_id FROM member_public_watch_checks
                     WHERE closed=0 AND state!='claimed' AND {OWNED_CHECKS} ORDER BY claimed_at LIMIT 4"
                ))?
                .query_map(params![t, a, v, g], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tx.commit()?;
            Ok(ids)
        })
        .await
    }

    /// The broker confirmed this disposable workspace is stopped (or never existed).
    pub async fn ack_public_watch_workspace(
        &self,
        owner: VolumeOwner,
        workspace_id: String,
    ) -> Result<()> {
        self.call(move |db| {
            owner.check_identity(&db.conn, None)?;
            let (t, a, v, g) = own(owner);
            db.conn.execute(
                &format!(
                    "UPDATE member_public_watch_checks SET closed=1
                     WHERE workspace_id=?5 AND state!='claimed' AND {OWNED_CHECKS}"
                ),
                params![t, a, v, g, workspace_id],
            )?;
            Ok(())
        })
        .await
    }

    /// Timer step 1 (no browser IO). Revokes watches whose host/auth epoch or thread changed, expires
    /// finished ones, and claims at most [`BATCH`] due slots, each with a fresh disposable workspace id.
    /// Each claim advances `next_due` in the same transaction, so a slot is never claimed twice. Slots
    /// missed while the app was closed are folded into the latest passed one (`skipped`), which is read
    /// now: the read itself is always fresh.
    pub async fn claim_due_public_watches(
        &self,
        owner: VolumeOwner,
        host_epoch: i64,
        time: i64,
    ) -> Result<Vec<PublicWatchLease>> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            owner.check_identity(&tx, None)?;
            let (t, a, v, g) = own(owner);
            let ids = |sql: String, p: &[&dyn rusqlite::ToSql]| -> Result<Vec<String>> {
                Ok(tx
                    .prepare(&sql)?
                    .query_map(p, |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?)
            };
            for id in ids(
                format!("SELECT w.id FROM member_public_watches w WHERE {OWNED} AND w.state='active' AND (w.host_epoch<?5 OR NOT {AUTHORIZED}) ORDER BY w.id LIMIT 64"),
                params![t, a, v, g, host_epoch],
            )? {
                end(&tx, owner, &id, "revoked", "session_changed", time)?;
            }
            for id in ids(
                format!("SELECT w.id FROM member_public_watches w WHERE {OWNED} AND w.state='active'
                         AND NOT EXISTS(SELECT 1 FROM threads th WHERE th.tenant_id=w.tenant_id AND th.id=w.thread_id AND th.account_id=w.account_id) LIMIT 64"),
                params![t, a, v, g],
            )? {
                end(&tx, owner, &id, "revoked", "thread_deleted", time)?;
            }
            for id in ids(
                format!("SELECT w.id FROM member_public_watches w WHERE {OWNED} AND w.state='active' AND w.expires_at<=?5 ORDER BY w.id LIMIT 64"),
                params![t, a, v, g, time],
            )? {
                end(&tx, owner, &id, "expired", "end_date", time)?;
            }
            let due = tx
                .prepare(&format!(
                    "{WATCH} WHERE {OWNED} AND w.state='active' AND w.next_due<=?5 AND w.host_epoch=?6 AND {AUTHORIZED}
                     ORDER BY w.next_due,w.id LIMIT ?7"
                ))?
                .query_map(params![t, a, v, g, time, host_epoch, BATCH], watch_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut leases = Vec::new();
            for w in &due {
                let Ok((slot, skipped, next)) = occurrence(w, time) else {
                    end(&tx, owner, &w.id, "revoked", "schedule_invalid", time)?;
                    continue;
                };
                let workspace = fresh_workspace(&tx)?;
                let claimed = tx.execute(
                    "INSERT OR IGNORE INTO member_public_watch_checks(tenant_id,account_id,watch_id,due_at,state,workspace_id,closed,skipped,claimed_at)
                     VALUES(?1,?2,?3,?4,'claimed',?5,0,?6,?7)",
                    params![t, a, w.id, slot, workspace.to_string(), skipped, time],
                )? == 1;
                advance(&tx, owner, w, next)?;
                if !claimed {
                    // The slot already has a row: it is never processed twice.
                    continue;
                }
                tx.execute(
                    &format!(
                        "DELETE FROM member_public_watch_checks WHERE closed=1 AND state!='claimed' AND watch_id=?5
                           AND tenant_id=?1 AND account_id=?2 AND {}
                           AND due_at<(SELECT due_at FROM member_public_watch_checks
                                       WHERE tenant_id=?1 AND account_id=?2 AND watch_id=?5 ORDER BY due_at DESC LIMIT 1 OFFSET ?6)",
                        "watch_id IN(SELECT id FROM member_public_watches WHERE tenant_id=?1 AND account_id=?2 AND volume_id=?3 AND generation=?4)"
                    ),
                    params![t, a, v, g, w.id, KEEP_CHECKS - 1],
                )?;
                leases.push(PublicWatchLease {
                    owner,
                    watch_id: Uuid::parse_str(&w.id).map_err(|_| StoreError::Corrupt)?,
                    due_at: slot,
                    claimed_at: time,
                    url: w.url.clone(),
                    host: w.host.clone(),
                    workspace_id: workspace,
                });
            }
            tx.commit()?;
            Ok(leases)
        })
        .await
    }

    /// Timer step 3. See [`Store::settle_public_watch_at`].
    pub async fn settle_public_watch(
        &self,
        lease: PublicWatchLease,
        read: PublicRead,
    ) -> Result<()> {
        let time = now()?;
        self.settle_public_watch_at(lease, read, time).await
    }

    /// Settles one claimed slot exactly once (`claimed -> final` is a single guarded transition, so the
    /// alert, the fingerprint and the check commit together or not at all). A stop, revocation or
    /// expiry while the read was in flight, or a slot older than one already settled, discards the
    /// result (`withdrawn`) and announces nothing.
    async fn settle_public_watch_at(
        &self,
        lease: PublicWatchLease,
        read: PublicRead,
        time: i64,
    ) -> Result<()> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let owner = lease.owner;
            owner.check_identity(&tx, None)?;
            let (t, a, _, _) = own(owner);
            let id = lease.watch_id.to_string();
            let workspace = lease.workspace_id.to_string();
            if read.closed {
                tx.execute(
                    "UPDATE member_public_watch_checks SET closed=1
                     WHERE tenant_id=?1 AND account_id=?2 AND watch_id=?3 AND due_at=?4 AND workspace_id=?5",
                    params![t, a, id, lease.due_at, workspace],
                )?;
            }
            let claimed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM member_public_watch_checks
                 WHERE tenant_id=?1 AND account_id=?2 AND watch_id=?3 AND due_at=?4 AND workspace_id=?5 AND state='claimed')",
                params![t, a, id, lease.due_at, workspace],
                |r| r.get(0),
            )?;
            if !claimed {
                // A restart or the stale-claim reaper already settled this slot as `unknown`.
                tx.commit()?;
                return Ok(());
            }
            let finish = |state: &str,
                          reason: Option<&str>,
                          fingerprint: Option<&str>,
                          cipher: Option<Vec<u8>>,
                          notice: Option<&str>|
             -> Result<()> {
                let n = tx.execute(
                    "UPDATE member_public_watch_checks SET state=?6,reason=?7,fingerprint=?8,cipher=?9,notification_id=?10,finished_at=?11
                     WHERE tenant_id=?1 AND account_id=?2 AND watch_id=?3 AND due_at=?4 AND workspace_id=?5 AND state='claimed'",
                    params![t, a, id, lease.due_at, workspace, state, reason, fingerprint, cipher, notice, time],
                )?;
                if n != 1 {
                    return Err(StoreError::Conflict);
                }
                Ok(())
            };
            let w = load(&tx, owner, &id)?;
            let thread_live: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND mission_internal=0)", params![t,a,w.thread], |r| r.get(0))?;
            let deliverable = w.state == "active" && w.expires > time && thread_live;
            if !deliverable
                || !authorized(&tx, &t, &a, w.auth_epoch)?
                || w.last_due.is_some_and(|due| due > lease.due_at)
            {
                finish("withdrawn", None, None, None, None)?;
                tx.commit()?;
                return Ok(());
            }
            let touch = |observed: Option<i64>| -> Result<()> {
                tx.execute(
                    "UPDATE member_public_watches SET last_due=?4,last_observed_at=coalesce(?5,last_observed_at)
                     WHERE tenant_id=?1 AND account_id=?2 AND id=?3",
                    params![t, a, id, lease.due_at, observed],
                )?;
                Ok(())
            };
            // Only a fresh, verified observation of the pinned page can be compared at all.
            let verified = match read.outcome {
                PublicReadOutcome::Failed(code) => Err(code),
                PublicReadOutcome::Page(page) => {
                    let price_watch = w.price.is_some();
                    let text = normalize(&page.text, MAX_TEXT + usize::from(price_watch));
                    if !same_host(&w.host, &page.url) {
                        Err("redirected_off_host")
                    } else if price_watch && !public_url(&page.url).is_ok_and(|(url, _)| url == w.url) {
                        Err("different_price_page")
                    } else if price_watch && (page.truncated || text.chars().count() > MAX_TEXT) {
                        Err("incomplete_price_page")
                    } else if page.observed_at < lease.claimed_at - SKEW || page.observed_at > time + SKEW {
                        Err("stale_observation")
                    } else if text.is_empty() {
                        Err("empty_page")
                    } else {
                        // A price watch needs one unambiguous displayed price; otherwise the read is
                        // `failed` (the band state is untouched), never a guess.
                        match w
                            .price
                            .map(|cond| price::parse(&text).map(|won| (cond, won)))
                            .transpose()
                        {
                            Ok(seen) => Ok((text, page.observed_at, seen)),
                            Err(code) => Err(code),
                        }
                    }
                }
            };
            let (text, observed, seen) = match verified {
                Ok(v) => v,
                Err(code) => {
                    // A failed read is never a change and never touches the last fingerprint.
                    finish("failed", Some(code), None, None, None)?;
                    touch(None)?;
                    tx.commit()?;
                    return Ok(());
                }
            };
            // Ordinary watch: the content fingerprint. Price watch: the in/out-of-band predicate, so that
            // only (re-)entering the band alerts; the first qualifying sample already counts as entering.
            let (digest, state, shown) = match seen {
                None => {
                    let digest = fingerprint(&w.url, &text);
                    let state = match w.fingerprint.as_deref() {
                        None => "baseline",
                        Some(previous) if previous == digest => "unchanged",
                        Some(_) => "changed",
                    };
                    (digest, state, excerpt(&text))
                }
                Some((cond, won)) => {
                    let inside = price::qualifies(won, cond.won, cond.inclusive);
                    let digest = band(&w.url, inside);
                    let state = match w.fingerprint.as_deref() {
                        None if !inside => "baseline",
                        Some(previous) if previous == digest => "unchanged",
                        // Leaving the band is silent (it rearms); entering or first seeing it alerts.
                        Some(_) if !inside => "unchanged",
                        _ => "changed",
                    };
                    (
                        digest,
                        state,
                        excerpt(&format!("가격 {} · {text}", price::display(won))),
                    )
                }
            };
            let cipher = (state != "unchanged")
                .then(|| {
                    providers::encrypt(
                        &db.provider_key,
                        shown.as_bytes(),
                        &aad(owner, &id, &w.host, lease.due_at),
                    )
                })
                .transpose()?;
            let notice = (state == "changed").then(|| notice_id(&id, lease.due_at));
            finish(state, None, Some(&digest), cipher, notice.as_deref())?;
            tx.execute(
                "UPDATE member_public_watches SET fingerprint=?4,last_observed_at=?5,last_due=?6,
                    last_changed_at=CASE WHEN ?7 THEN ?8 ELSE last_changed_at END,alerts=alerts+?7
                 WHERE tenant_id=?1 AND account_id=?2 AND id=?3",
                params![t, a, id, digest, observed, lease.due_at, state == "changed", time],
            )?;
            if let Some(notice) = &notice {
                crate::schedules::direct_notice(
                    &tx,
                    &t,
                    &a,
                    &id,
                    notice,
                    match seen {
                        None => "지켜보던 페이지가 바뀌었어요",
                        Some(_) => "지켜보던 가격이 조건에 맞아요",
                    },
                    &match seen {
                        None => format!(
                            "{} 페이지의 보이는 내용이 바뀌었어요. 대화의 감시 기록에서 확인하세요.",
                            w.host
                        ),
                        Some((cond, won)) => format!(
                            "{} 페이지에 표시된 가격이 {}이에요. 조건: {}. 대화의 감시 기록에서 확인하세요.",
                            w.host,
                            price::display(won),
                            condition_text(cond)
                        ),
                    },
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests;
