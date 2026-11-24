//! Member connected morning brief (schema 99): a read-only, explicit, recurring Gmail + primary
//! Calendar briefing.
//!
//! * **Grant** – one explicit request per brief, pinned to the exact Google connection id/epoch/app
//!   revision the member was shown, the account auth epoch, the gateway host epoch, the thread, local
//!   time, IANA zone, interval and end date. A reconnect, disconnect, app change or password reset
//!   changes a pin, so the grant stops working until the member grants again.
//! * **Slots** – the primary key of the run table is the dedup key. A slot is claimed (and the next
//!   due time advanced) in one transaction *before* any provider IO. A restart marks an unsettled claim
//!   `unknown`; nothing is ever read twice and no alert is sent twice.
//! * **Authority** – reads use `member_google::briefs` (pinned background authority, no session).
//! * **Evidence** – only the minimum is kept (<=5 inbox subjects/senders, <=12 event titles/times), encrypted with
//!   the provider key, and shown only while every pinned source is still live. Notifications and the
//!   thread worklog line carry counts only. No model call is made, so mail text is never an instruction.
//! * **No false briefs** – a missing/changed credential records `needs_connection` guidance and ends the
//!   brief; an unreadable provider records `unavailable`/`partial`, never `completed`.
use super::*;
use crate::personal_volume::VolumeOwner;
use chrono::{DateTime, Days, LocalResult, NaiveDate, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};

use super::member_google::briefs as authority;
pub use super::member_google::briefs::{
    MemberBackgroundBinding, MemberBackgroundRefresh, MemberBriefSource,
};

const ACTIVE_LIMIT: i64 = 4;
const BATCH: i64 = 2;
/// A slot later than this after its local time is recorded as missed, not delivered stale.
const GRACE: i64 = 3 * 3600;
const STALE_CLAIM: i64 = 600;
const KEEP_RUNS: i64 = 30;
const MAX_CATCHUP: usize = 400;
pub const MEMBER_BRIEF_MAX_MESSAGES: usize = 5;
pub const MEMBER_BRIEF_MAX_EVENTS: usize = 12;
const TEXT: usize = 120;
const LIST_LIMIT: i64 = 30;
const RECENT: i64 = 7;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberBriefPin {
    pub connection_id: String,
    pub epoch: i64,
}
/// `consent` must be true: the member pressed the grant button for exactly this body.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberBriefRequest {
    pub request_id: String,
    pub thread_id: String,
    pub consent: bool,
    #[serde(default)]
    pub message: Option<String>,
    pub gmail: Option<MemberBriefPin>,
    pub calendar: Option<MemberBriefPin>,
    pub at: String,
    pub timezone: String,
    pub interval_days: i64,
    pub days: i64,
    /// Explicit member choice: read only Gmail `IMPORTANT`+`INBOX` mail. Requires a Gmail pin.
    /// Absent means the ordinary recent-inbox brief (unchanged behaviour).
    #[serde(default)]
    pub important_only: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberBriefStop {
    pub request_id: String,
    pub revision: i64,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemberBriefMessage {
    pub subject: String,
    pub from: String,
    pub received_at_ms: i64,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemberBriefEvent {
    pub title: String,
    pub start: String,
    pub end: String,
    pub all_day: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MemberBriefDetail {
    pub captured_at: i64,
    pub messages: Option<Vec<MemberBriefMessage>>,
    pub events: Option<Vec<MemberBriefEvent>>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberBriefRun {
    pub due_at: i64,
    /// claimed | completed | partial | unavailable | needs_connection | missed | unknown | withdrawn
    pub state: String,
    /// `live` for a fresh provider read. Anything else must be labelled by the UI.
    pub freshness: Option<String>,
    pub guidance: Option<String>,
    pub skipped: i64,
    pub gmail_count: Option<i64>,
    pub calendar_count: Option<i64>,
    pub finished_at: Option<i64>,
    pub detail: Option<MemberBriefDetail>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberBriefView {
    pub id: String,
    pub thread_id: String,
    pub sources: Vec<String>,
    pub at: String,
    pub timezone: String,
    pub interval_days: i64,
    pub state: String,
    pub end_reason: Option<String>,
    pub revision: i64,
    pub created_at: i64,
    pub expires_at: i64,
    pub next_due: Option<i64>,
    pub read_only: bool,
    /// Gmail part reads only mail Google labels `IMPORTANT` (in the inbox); never our own judgement.
    pub important_only: bool,
    pub runs: Vec<MemberBriefRun>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberBriefPage {
    pub items: Vec<MemberBriefView>,
    pub active_limit: i64,
}

/// What the reader obtained. `None` for a requested section means that read failed.
pub struct MemberBriefEvidence {
    pub messages: Option<Vec<MemberBriefMessage>>,
    pub events: Option<Vec<MemberBriefEvent>>,
}
pub enum MemberBriefOutcome {
    Evidence(MemberBriefEvidence),
    Unavailable(&'static str),
}

/// One claimed slot. Carries no secrets; the reader opens pinned authority through the store.
#[derive(Clone)]
pub struct MemberBriefLease {
    pub(in crate::mail) owner: VolumeOwner,
    pub(in crate::mail) auth_epoch: i64,
    pub brief_id: Uuid,
    pub due_at: i64,
    pub thread_id: String,
    pub gmail: Option<MemberBriefSource>,
    pub calendar: Option<MemberBriefSource>,
    /// Pinned from the grant: select Gmail `IMPORTANT`+`INBOX` only. Never true without `gmail`.
    pub important_only: bool,
    /// Inbox messages older than this are not relevant to this slot.
    pub since_ms: i64,
    /// Local calendar day of the slot as RFC 3339 `[start, end)`.
    pub window: (String, String),
}

// ---------------------------------------------------------------------------------------------
// Civil time (IANA zones only). A nonexistent local time is skipped for that day; an ambiguous one
// uses its first occurrence.
// ---------------------------------------------------------------------------------------------

fn zone(name: &str) -> Result<chrono_tz::Tz> {
    if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
        return Err(StoreError::InvalidInput);
    }
    name.parse().map_err(|_| StoreError::InvalidInput)
}
fn clock(text: &str) -> Result<NaiveTime> {
    let time = NaiveTime::parse_from_str(text, "%H:%M").map_err(|_| StoreError::InvalidInput)?;
    (time.format("%H:%M").to_string() == text)
        .then_some(time)
        .ok_or(StoreError::InvalidInput)
}
fn local_date(tz: chrono_tz::Tz, instant: i64) -> Result<NaiveDate> {
    Ok(DateTime::from_timestamp(instant, 0)
        .ok_or(StoreError::InvalidInput)?
        .with_timezone(&tz)
        .date_naive())
}
fn at(tz: chrono_tz::Tz, date: NaiveDate, time: NaiveTime) -> Option<i64> {
    match tz.from_local_datetime(&date.and_time(time)) {
        LocalResult::Single(t) => Some(t.timestamp()),
        LocalResult::Ambiguous(a, b) => Some(a.timestamp().min(b.timestamp())),
        LocalResult::None => None,
    }
}
/// First slot on `date` or the next days that has a valid local time.
fn slot_from(tz: chrono_tz::Tz, time: NaiveTime, mut date: NaiveDate) -> Result<i64> {
    for _ in 0..4 {
        if let Some(i) = at(tz, date, time) {
            return Ok(i);
        }
        date = date
            .checked_add_days(Days::new(1))
            .ok_or(StoreError::InvalidInput)?;
    }
    Err(StoreError::InvalidInput)
}
fn first_slot_after(tz: chrono_tz::Tz, time: NaiveTime, after: i64) -> Result<i64> {
    let mut date = local_date(tz, after)?;
    for _ in 0..4 {
        if let Some(i) = at(tz, date, time).filter(|i| *i > after) {
            return Ok(i);
        }
        date = date
            .checked_add_days(Days::new(1))
            .ok_or(StoreError::InvalidInput)?;
    }
    Err(StoreError::InvalidInput)
}
fn next_slot(tz: chrono_tz::Tz, time: NaiveTime, prev: i64, interval: i64) -> Result<i64> {
    let date = local_date(tz, prev)?
        .checked_add_days(Days::new(interval as u64))
        .ok_or(StoreError::InvalidInput)?;
    slot_from(tz, time, date)
}
/// Local calendar day containing `slot`, as `[midnight, next midnight)`.
fn day_window(tz: chrono_tz::Tz, slot: i64) -> Result<(String, String)> {
    let date = local_date(tz, slot)?;
    let midnight = |d: NaiveDate| {
        tz.from_local_datetime(&d.and_time(NaiveTime::MIN))
            .earliest()
            .ok_or(StoreError::InvalidInput)
    };
    let next = date
        .checked_add_days(Days::new(1))
        .ok_or(StoreError::InvalidInput)?;
    Ok((midnight(date)?.to_rfc3339(), midnight(next)?.to_rfc3339()))
}

// ---------------------------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------------------------

struct Brief {
    id: String,
    host_epoch: i64,
    auth_epoch: i64,
    thread: String,
    gmail: Option<MemberBriefSource>,
    calendar: Option<MemberBriefSource>,
    at: String,
    zone: String,
    interval: i64,
    state: String,
    reason: Option<String>,
    revision: i64,
    created: i64,
    expires: i64,
    next_due: Option<i64>,
    digest: Vec<u8>,
    important_only: bool,
}
const BRIEF: &str = "SELECT id,auth_epoch,thread_id,gmail_connection,gmail_epoch,gmail_app,calendar_connection,calendar_epoch,calendar_app,
 local_time,zone,interval_days,state,end_reason,revision,created_at,expires_at,next_due,request_digest,important_only,host_epoch FROM member_connected_briefs b";
const OWNED: &str = "b.tenant_id=?1 AND b.account_id=?2 AND b.volume_id=?3 AND b.generation=?4";
const AUTHORIZED: &str = "EXISTS(SELECT 1 FROM accounts a WHERE a.id=b.account_id AND a.tenant_id=b.tenant_id
    AND a.auth_epoch=b.auth_epoch AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)";

fn uuid_col(s: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&s).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn source_cols(
    service: MemberGoogleService,
    r: &rusqlite::Row<'_>,
    i: usize,
) -> rusqlite::Result<Option<MemberBriefSource>> {
    let connection: Option<String> = r.get(i)?;
    let epoch: Option<i64> = r.get(i + 1)?;
    let app: Option<String> = r.get(i + 2)?;
    match (connection, epoch, app) {
        (Some(c), Some(e), Some(a)) => Ok(Some(MemberBriefSource {
            service,
            connection_id: uuid_col(c)?,
            epoch: e,
            app_revision: uuid_col(a)?,
        })),
        (None, None, None) => Ok(None),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}
fn brief_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Brief> {
    Ok(Brief {
        id: r.get(0)?,
        auth_epoch: r.get(1)?,
        thread: r.get(2)?,
        gmail: source_cols(MemberGoogleService::Gmail, r, 3)?,
        calendar: source_cols(MemberGoogleService::Calendar, r, 6)?,
        at: r.get(9)?,
        zone: r.get(10)?,
        interval: r.get(11)?,
        state: r.get(12)?,
        reason: r.get(13)?,
        revision: r.get(14)?,
        created: r.get(15)?,
        expires: r.get(16)?,
        next_due: r.get(17)?,
        digest: r.get(18)?,
        important_only: r.get(19)?,
        host_epoch: r.get(20)?,
    })
}
fn own(o: VolumeOwner) -> (String, String, String, i64) {
    (
        o.tenant.to_string(),
        o.account.to_string(),
        o.volume.to_string(),
        o.generation as i64,
    )
}
fn load(c: &Connection, o: VolumeOwner, id: &str) -> Result<Brief> {
    let (t, a, v, g) = own(o);
    c.query_row(
        &format!("{BRIEF} WHERE {OWNED} AND b.id=?5"),
        params![t, a, v, g, id],
        brief_row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}
fn canonical(value: &str) -> Result<()> {
    Uuid::parse_str(value)
        .ok()
        .filter(|id| !id.is_nil() && id.to_string() == value)
        .map(|_| ())
        .ok_or(StoreError::InvalidInput)
}
fn authorize(c: &Connection, o: VolumeOwner, user: &UserContext) -> Result<UserContext> {
    o.check_identity(c, None)?;
    let u = crate::validate(c, user, true, false)?;
    if u.tenant.0 != o.tenant.to_string() || u.account.id != o.account.to_string() || u.helper {
        return Err(StoreError::Forbidden);
    }
    Ok(u)
}
fn source_names(b: &Brief) -> Vec<String> {
    let mut v = Vec::new();
    if b.gmail.is_some() {
        v.push("gmail".into());
    }
    if b.calendar.is_some() {
        v.push("calendar".into());
    }
    v
}
fn sources(b: &Brief) -> impl Iterator<Item = &MemberBriefSource> {
    b.gmail.iter().chain(b.calendar.iter())
}
fn live(c: &Connection, o: VolumeOwner, b: &Brief) -> bool {
    sources(b).all(|s| authority::source_live(c, o, b.auth_epoch, s))
}
fn missing(c: &Connection, o: VolumeOwner, b: &Brief) -> Vec<&'static str> {
    sources(b)
        .filter(|s| !authority::source_live(c, o, b.auth_epoch, s))
        .map(|s| {
            if s.service == MemberGoogleService::Gmail {
                "gmail"
            } else {
                "calendar"
            }
        })
        .collect()
}

fn aad(o: VolumeOwner, b: &Brief, due: i64) -> Vec<u8> {
    let pin = |s: &Option<MemberBriefSource>| {
        s.map(|s| format!("{}:{}:{}", s.connection_id, s.epoch, s.app_revision))
            .unwrap_or_default()
    };
    format!(
        "moosuri/member-brief/v1/{}/{}/{}/{}/{}/{due}/{}/{}/{}",
        o.tenant,
        o.account,
        o.volume,
        o.generation,
        b.id,
        pin(&b.gmail),
        pin(&b.calendar),
        b.important_only
    )
    .into_bytes()
}
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(TEXT)
        .collect()
}
fn notice_id(brief: &str, due: i64) -> String {
    let mut h = Sha256::new();
    h.update(b"moosuri/member-brief/notice/v1\0");
    h.update(brief.as_bytes());
    h.update(due.to_be_bytes());
    let hash = h.finalize();
    let mut id: [u8; 16] = hash[..16].try_into().unwrap_or([1; 16]);
    id[6] = (id[6] & 15) | 0x50;
    id[8] = (id[8] & 63) | 0x80;
    Uuid::from_bytes(id).to_string()
}
fn notify(
    c: &Connection,
    o: VolumeOwner,
    brief: &str,
    id: &str,
    title: &str,
    message: &str,
) -> Result<()> {
    let (t, a, _, _) = own(o);
    crate::schedules::direct_notice(c, &t, &a, brief, id, title, message)
}

// ---------------------------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------------------------

fn suppress_push(c: &Connection, o: VolumeOwner, id: &str, time: i64) -> Result<()> {
    let t = o.tenant.to_string();
    c.execute(
        "UPDATE local_notifications SET push_suppressed=1 WHERE tenant_id=?1 AND schedule_id=?2 AND push_suppressed=0",
        params![t, id],
    )?;
    c.execute(
        "UPDATE push_queue SET state='suppressed',nonce=NULL,deadline=NULL,finished_at=?3,reason='brief_stopped'
         WHERE tenant_id=?1 AND state IN('queued','claimed')
           AND notification_id IN(SELECT id FROM local_notifications WHERE tenant_id=?1 AND schedule_id=?2)",
        params![t, id, time],
    )?;
    Ok(())
}
fn purge(c: &Connection, o: VolumeOwner, id: &str) -> Result<()> {
    let (t, a, _, _) = own(o);
    c.execute(
        "UPDATE member_connected_brief_runs SET state='withdrawn',cipher=NULL,freshness=NULL WHERE tenant_id=?1 AND account_id=?2 AND brief_id=?3 AND (cipher IS NOT NULL OR state='claimed')",
        params![t, a, id],
    )?;
    Ok(())
}
/// Final state for an active brief. Revocation also drops private evidence and cancels pushes.
fn end(
    c: &Connection,
    o: VolumeOwner,
    id: &str,
    state: &str,
    reason: &str,
    time: i64,
) -> Result<()> {
    let (t, a, _, _) = own(o);
    let changed = c.execute(
        "UPDATE member_connected_briefs SET state=?4,end_reason=?5,next_due=NULL,ended_at=?6,revision=revision+1 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='active'",
        params![t, a, id, state, reason, time],
    )?;
    if changed == 1 && matches!(state, "stopped" | "revoked") {
        suppress_push(c, o, id, time)?;
    }
    if state == "revoked" {
        purge(c, o, id)?;
    }
    Ok(())
}
fn advance(c: &Connection, o: VolumeOwner, b: &Brief, next: i64) -> Result<()> {
    // Past the end date there is no further slot, but the brief stays `active` until `expires_at`
    // so a slot still in flight can be stopped and is settled/withdrawn against an active grant.
    let next = next.min(b.expires);
    let (t, a, _, _) = own(o);
    c.execute(
        "UPDATE member_connected_briefs SET next_due=?4 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='active'",
        params![t, a, b.id, next],
    )?;
    Ok(())
}
/// Dedup point. `false` means the slot already has a row and must not be processed again.
#[allow(clippy::too_many_arguments)]
fn claim_slot(
    c: &Connection,
    o: VolumeOwner,
    id: &str,
    due: i64,
    state: &str,
    guidance: Option<&str>,
    skipped: i64,
    time: i64,
) -> Result<bool> {
    let (t, a, _, _) = own(o);
    let done = (state != "claimed").then_some(time);
    Ok(c.execute(
        "INSERT OR IGNORE INTO member_connected_brief_runs(tenant_id,account_id,brief_id,due_at,state,guidance,skipped,claimed_at,finished_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![t, a, id, due, state, guidance, skipped, time, done],
    )? == 1)
}
fn prune(c: &Connection, o: VolumeOwner, id: &str) -> Result<()> {
    let (t, a, _, _) = own(o);
    c.execute(
        "DELETE FROM member_connected_brief_runs WHERE tenant_id=?1 AND account_id=?2 AND brief_id=?3 AND due_at<
         (SELECT due_at FROM member_connected_brief_runs WHERE tenant_id=?1 AND account_id=?2 AND brief_id=?3 ORDER BY due_at DESC LIMIT 1 OFFSET ?4)",
        params![t, a, id, KEEP_RUNS - 1],
    )?;
    Ok(())
}

/// (latest passed slot, passed slots skipped before it, first slot after `time`).
fn occurrence(b: &Brief, time: i64) -> Result<(i64, i64, i64)> {
    let tz = zone(&b.zone).map_err(|_| StoreError::Corrupt)?;
    let clock = clock(&b.at).map_err(|_| StoreError::Corrupt)?;
    let (mut slot, mut skipped) = (b.next_due.ok_or(StoreError::Corrupt)?, 0);
    for _ in 0..MAX_CATCHUP {
        let next = next_slot(tz, clock, slot, b.interval)?;
        if next > time {
            return Ok((slot, skipped, next));
        }
        slot = next;
        skipped += 1;
    }
    Err(StoreError::Corrupt)
}

pub(super) fn restart(c: &Connection) -> Result<()> {
    c.execute(
        "UPDATE member_connected_brief_runs SET state='unknown',cipher=NULL,finished_at=unixepoch() WHERE state='claimed'",
        [],
    )?;
    Ok(())
}
/// Disconnect / app replacement. `None` withdraws everything for the account.
pub(super) fn withdraw(
    c: &Connection,
    tenant: &str,
    account: &str,
    service: Option<MemberGoogleService>,
) -> Result<()> {
    let column = match service {
        Some(MemberGoogleService::Gmail) => "gmail_connection IS NOT NULL",
        Some(MemberGoogleService::Calendar) => "calendar_connection IS NOT NULL",
        _ => "1=1",
    };
    let ids = c
        .prepare(&format!("SELECT id,volume_id,generation FROM member_connected_briefs WHERE tenant_id=?1 AND account_id=?2 AND {column}"))?
        .query_map(params![tenant, account], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let time = now()?;
    for (id, volume, generation) in ids {
        let o = VolumeOwner {
            account: Uuid::parse_str(account).map_err(|_| StoreError::Corrupt)?,
            tenant: Uuid::parse_str(tenant).map_err(|_| StoreError::Corrupt)?,
            volume: Uuid::parse_str(&volume).map_err(|_| StoreError::Corrupt)?,
            generation: generation as u64,
        };
        end(c, o, &id, "revoked", "source_changed", time)?;
        purge(c, o, &id)?;
    }
    Ok(())
}
/// Live-authority check for a brief notification (inbox and push dispatch). `None`: not a brief notice.
pub(crate) fn notice_current(c: &Connection, tenant: &str, id: &str) -> Result<Option<bool>> {
    Ok(c.query_row(
        "SELECT b.state!='revoked' AND EXISTS(SELECT 1 FROM accounts a WHERE a.id=b.account_id AND a.tenant_id=b.tenant_id
            AND a.auth_epoch=b.auth_epoch AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)
         FROM local_notifications n JOIN member_connected_briefs b ON b.tenant_id=n.tenant_id AND b.id=n.schedule_id
         WHERE n.tenant_id=?1 AND n.id=?2",
        params![tenant, id],
        |r| r.get::<_, bool>(0),
    )
    .optional()?)
}

// ---------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------

fn check_brief(
    c: &Connection,
    owner: VolumeOwner,
    user: &UserContext,
    host_epoch: i64,
    b: Brief,
) -> Result<Brief> {
    if host_epoch < b.host_epoch {
        return Err(StoreError::Forbidden);
    }
    crate::chat::thread_exists(c, user, &b.thread)?;
    if b.state == "active" && (host_epoch != b.host_epoch || user.epoch != b.auth_epoch) {
        end(c, owner, &b.id, "revoked", "session_changed", now()?)?;
        return load(c, owner, &b.id);
    }
    Ok(b)
}

fn view(
    c: &Connection,
    key: &[u8; 32],
    o: VolumeOwner,
    b: Brief,
    recent: i64,
) -> Result<MemberBriefView> {
    let (t, a, _, _) = own(o);
    // Private evidence is shown only while every pinned source is still the one that was granted.
    if b.state != "active" || b.expires <= now()? || !live(c, o, &b) {
        c.execute(
            "UPDATE member_connected_brief_runs SET state='withdrawn',cipher=NULL,freshness=NULL WHERE tenant_id=?1 AND account_id=?2 AND brief_id=?3 AND cipher IS NOT NULL",
            params![t, a, b.id],
        )?;
    }
    let rows = c
        .prepare(
            "SELECT due_at,state,freshness,guidance,skipped,gmail_count,calendar_count,cipher,finished_at FROM member_connected_brief_runs
             WHERE tenant_id=?1 AND account_id=?2 AND brief_id=?3 ORDER BY due_at DESC LIMIT ?4",
        )?
        .query_map(params![t, a, b.id, recent], |r| {
            Ok((
                MemberBriefRun {
                    due_at: r.get(0)?,
                    state: r.get(1)?,
                    freshness: r.get(2)?,
                    guidance: r.get(3)?,
                    skipped: r.get(4)?,
                    gmail_count: r.get(5)?,
                    calendar_count: r.get(6)?,
                    finished_at: r.get(8)?,
                    detail: None,
                },
                r.get::<_, Option<Vec<u8>>>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut runs = Vec::new();
    for (mut run, cipher) in rows {
        if let Some(blob) = cipher {
            let bytes = providers::decrypt(key, &blob, &aad(o, &b, run.due_at))?;
            run.detail = Some(serde_json::from_slice(&bytes).map_err(|_| StoreError::Corrupt)?);
        }
        runs.push(run);
    }
    let lapsed = b.state == "active" && b.expires <= now()?;
    Ok(MemberBriefView {
        sources: source_names(&b),
        state: if lapsed { "expired".into() } else { b.state },
        next_due: if lapsed { None } else { b.next_due },
        important_only: b.important_only,
        id: b.id,
        thread_id: b.thread,
        at: b.at,
        timezone: b.zone,
        interval_days: b.interval,
        end_reason: b.reason,
        revision: b.revision,
        created_at: b.created,
        expires_at: b.expires,
        read_only: true,
        runs,
    })
}

fn request_digest(r: &MemberBriefRequest) -> Result<Vec<u8>> {
    let pin = |p: &Option<MemberBriefPin>| p.as_ref().map(|p| (p.connection_id.clone(), p.epoch));
    let bytes = serde_json::to_vec(&(
        "moosuri/member-brief/request/v1",
        &r.thread_id,
        &r.message,
        pin(&r.gmail),
        pin(&r.calendar),
        &r.at,
        &r.timezone,
        r.interval_days,
        r.days,
        r.important_only,
    ))
    .map_err(|_| StoreError::InvalidInput)?;
    Ok(Sha256::digest(bytes).to_vec())
}

impl Store {
    /// Creates the explicit grant, or replays the identical earlier request. Pins must equal the
    /// member's *current* connection id + epoch for every requested source.
    pub async fn create_member_brief(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        request: MemberBriefRequest,
    ) -> Result<MemberBriefView> {
        canonical(&request.request_id)?;
        canonical(&request.thread_id)?;
        if request
            .message
            .as_ref()
            .is_some_and(|m| m.trim().is_empty() || m.len() > 8192)
            || !request.consent
            || !(1..=7).contains(&request.interval_days)
            || !(1..=90).contains(&request.days)
            || (request.gmail.is_none() && request.calendar.is_none())
            || (request.important_only && request.gmail.is_none())
        {
            return Err(StoreError::InvalidInput);
        }
        let (tz, time_of_day) = (zone(&request.timezone)?, clock(&request.at)?);
        let digest = request_digest(&request)?;
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authorize(&tx, owner, &user)?;
            let (t, a, v, g) = own(owner);
            if let Some(old) = tx
                .query_row(&format!("{BRIEF} WHERE {OWNED} AND b.id=?5"), params![t, a, v, g, request.request_id], brief_row)
                .optional()?
            {
                if old.digest != digest {
                    return Err(StoreError::Conflict);
                }
                let old = check_brief(&tx, owner, &u, host_epoch, old)?;
                let out = view(&tx, &db.provider_key, owner, old, RECENT)?;
                tx.commit()?;
                return Ok(out);
            }
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND id=?2 AND account_id=?3 AND mission_internal=0)",
                params![t, request.thread_id, a],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(StoreError::NotFound);
            }
            let time = now()?;
            let active: i64 = tx.query_row(
                &format!("SELECT count(*) FROM member_connected_briefs b WHERE {OWNED} AND b.state='active' AND b.expires_at>?5"),
                params![t, a, v, g, time],
                |r| r.get(0),
            )?;
            if active >= ACTIVE_LIMIT {
                return Err(StoreError::BudgetExhausted);
            }
            let pin = |service, shown: &Option<MemberBriefPin>| -> Result<Option<MemberBriefSource>> {
                let Some(p) = shown else { return Ok(None) };
                canonical(&p.connection_id)?;
                let id = Uuid::parse_str(&p.connection_id).map_err(|_| StoreError::InvalidInput)?;
                if p.epoch <= 0 {
                    return Err(StoreError::InvalidInput);
                }
                authority::capture(&tx, &u, owner, service, id, p.epoch).map(Some)
            };
            let gmail = pin(MemberGoogleService::Gmail, &request.gmail)?;
            let calendar = pin(MemberGoogleService::Calendar, &request.calendar)?;
            let next = first_slot_after(tz, time_of_day, time)?;
            let cols = |s: &Option<MemberBriefSource>| (s.map(|s| s.connection_id.to_string()), s.map(|s| s.epoch), s.map(|s| s.app_revision.to_string()));
            let (gc, ge, ga) = cols(&gmail);
            let (cc, ce, ca) = cols(&calendar);
            tx.execute(
                "INSERT INTO member_connected_briefs(tenant_id,account_id,id,volume_id,generation,auth_epoch,host_epoch,thread_id,
                    gmail_connection,gmail_epoch,gmail_app,calendar_connection,calendar_epoch,calendar_app,
                    local_time,zone,interval_days,state,revision,created_at,expires_at,next_due,request_digest,important_only)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,'active',1,?18,?19,?20,?21,?22)",
                params![t, a, request.request_id, v, g, u.epoch, host_epoch, request.thread_id, gc, ge, ga, cc, ce, ca,
                    request.at, request.timezone, request.interval_days, time, time + request.days * 86400, next, digest, request.important_only],
            )?;
            let message = request.message.clone().unwrap_or_else(|| format!("매일 {}에 {} 브리핑", request.at, request.timezone));
            crate::personal_browser::member_work_turn(&tx, &u, &request.thread_id, &request.request_id,
                &request.request_id, &message, "member-connected-brief", "요청한 시간에 브리핑을 시작합니다. 회차별 결과는 이 대화의 브리핑 기록에서 확인할 수 있습니다.".into())?;
            let out = view(&tx, &db.provider_key, owner, load(&tx, owner, &request.request_id)?, RECENT)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    pub async fn member_briefs(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        thread: Option<Uuid>,
    ) -> Result<MemberBriefPage> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize(&tx, owner, &user)?;
            let (t, a, v, g) = own(owner);
            let rows = tx
                .prepare(&format!("{BRIEF} WHERE {OWNED} AND b.host_epoch<=?7 AND EXISTS(SELECT 1 FROM threads th WHERE th.id=b.thread_id AND th.account_id=b.account_id AND th.tenant_id=b.tenant_id AND th.mission_internal=0) AND (?6 IS NULL OR b.thread_id=?6) ORDER BY (b.state='active') DESC,b.created_at DESC,b.id LIMIT ?5"))?
                .query_map(params![t, a, v, g, LIST_LIMIT, thread.map(|id|id.to_string()), host_epoch], brief_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut items = Vec::new();
            for b in rows {
                let b = check_brief(&tx, owner, &user, host_epoch, b)?;
                items.push(view(&tx, &db.provider_key, owner, b, RECENT)?);
            }
            tx.commit()?;
            Ok(MemberBriefPage { items, active_limit: ACTIVE_LIMIT })
        })
        .await
    }

    pub async fn member_brief(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        id: Uuid,
    ) -> Result<MemberBriefView> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize(&tx, owner, &user)?;
            let b = check_brief(
                &tx,
                owner,
                &user,
                host_epoch,
                load(&tx, owner, &id.to_string())?,
            )?;
            let out = view(&tx, &db.provider_key, owner, b, 30)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    /// Stops an active brief. A replay or a request against an ended brief returns its state; a stale
    /// revision on a live one conflicts.
    pub async fn stop_member_brief(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        id: Uuid,
        stop: MemberBriefStop,
    ) -> Result<MemberBriefView> {
        canonical(&stop.request_id)?;
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize(&tx, owner, &user)?;
            let id = id.to_string();
            let time = now()?;
            let b = check_brief(&tx, owner, &user, host_epoch, load(&tx, owner, &id)?)?;
            if b.state == "active" {
                if b.expires <= time {
                    end(&tx, owner, &id, "expired", "end_date", time)?;
                } else {
                    if stop.revision != b.revision {
                        return Err(StoreError::Conflict);
                    }
                    end(&tx, owner, &id, "stopped", "member", time)?;
                    let (t, a, _, _) = own(owner);
                    tx.execute(
                        "UPDATE member_connected_briefs SET stop_request_id=?4 WHERE tenant_id=?1 AND account_id=?2 AND id=?3",
                        params![t, a, id, stop.request_id],
                    )?;
                }
            }
            let out = view(&tx, &db.provider_key, owner, load(&tx, owner, &id)?, 30)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    /// Cheap per-tick fence (no provider IO, no batch gate). The parent calls it on EVERY authenticated
    /// timer tick, even when the reader batch gate is busy. Revokes active grants of this member volume
    /// pinned to an *earlier* host epoch (withdrawing claimed/in-flight evidence, cancelling pushes).
    /// Grants pinned to the same or a future epoch are never touched, so an old tick cannot revoke them.
    pub async fn fence_member_brief_epoch(
        &self,
        owner: VolumeOwner,
        host_epoch: i64,
    ) -> Result<()> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            owner.check_identity(&tx, None)?;
            let (t, a, v, g) = own(owner);
            let ids = tx
                .prepare(&format!("SELECT b.id FROM member_connected_briefs b WHERE {OWNED} AND b.state='active' AND b.host_epoch<?5 LIMIT 64"))?
                .query_map(params![t, a, v, g, host_epoch], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let time = now()?;
            for id in ids {
                end(&tx, owner, &id, "revoked", "session_changed", time)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Timer step 1 (no provider IO). Revokes grants whose host/auth epoch or thread changed, expires
    /// finished ones, records `missed`/`needs_connection` slots, and claims at most `BATCH` live slots.
    /// Each claim advances `next_due` in the same transaction, so a slot is never claimed twice.
    pub async fn claim_due_member_briefs(
        &self,
        owner: VolumeOwner,
        host_epoch: i64,
        time: i64,
    ) -> Result<Vec<MemberBriefLease>> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            owner.check_identity(&tx, None)?;
            let (t, a, v, g) = own(owner);
            tx.execute(
                "UPDATE member_connected_brief_runs SET state='unknown',cipher=NULL,finished_at=?3
                 WHERE tenant_id=?1 AND account_id=?2 AND state='claimed' AND claimed_at<?4",
                params![t, a, time, time - STALE_CLAIM],
            )?;
            let ids = |sql: String, p: &[&dyn rusqlite::ToSql]| -> Result<Vec<String>> {
                Ok(tx.prepare(&sql)?.query_map(p, |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
            };
            for id in ids(
                format!("SELECT b.id FROM member_connected_briefs b WHERE {OWNED} AND b.state='active' AND (b.host_epoch<?5 OR NOT {AUTHORIZED}) ORDER BY b.id LIMIT 64"),
                params![t, a, v, g, host_epoch],
            )? {
                end(&tx, owner, &id, "revoked", "session_changed", time)?;
            }
            for id in ids(
                format!("SELECT b.id FROM member_connected_briefs b WHERE {OWNED} AND b.state='active'
                         AND NOT EXISTS(SELECT 1 FROM threads th WHERE th.tenant_id=b.tenant_id AND th.id=b.thread_id AND th.account_id=b.account_id) LIMIT 64"),
                params![t, a, v, g],
            )? {
                end(&tx, owner, &id, "revoked", "thread_deleted", time)?;
            }
            for id in ids(
                format!("SELECT b.id FROM member_connected_briefs b WHERE {OWNED} AND b.state='active' AND b.expires_at<=?5 ORDER BY b.id LIMIT 64"),
                params![t, a, v, g, time],
            )? {
                end(&tx, owner, &id, "expired", "end_date", time)?;
            }
            let due = tx
                .prepare(&format!("{BRIEF} WHERE {OWNED} AND b.state='active' AND b.next_due<=?5 AND b.host_epoch=?6 AND {AUTHORIZED} ORDER BY b.next_due,b.id LIMIT ?7"))?
                .query_map(params![t, a, v, g, time, host_epoch, BATCH], brief_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut leases = Vec::new();
            for b in &due {
                let Ok((slot, skipped, next)) = occurrence(b, time) else {
                    end(&tx, owner, &b.id, "revoked", "schedule_invalid", time)?;
                    continue;
                };
                if time - slot > GRACE {
                    claim_slot(&tx, owner, &b.id, slot, "missed", None, skipped, time)?;
                    advance(&tx, owner, b, next)?;
                    continue;
                }
                let gone = missing(&tx, owner, b);
                if !gone.is_empty() {
                    let guidance = gone.join(",");
                    if claim_slot(&tx, owner, &b.id, slot, "needs_connection", Some(&guidance), skipped, time)? {
                        let notice = notice_id(&b.id, slot);
                        notify(&tx, owner, &b.id, &notice, "Google 연결이 필요해요",
                            "브리핑에 필요한 Gmail 또는 캘린더 연결이 끊기거나 바뀌었어요. 대화에서 다시 연결한 뒤 브리핑을 새로 허용해 주세요. 내용은 읽지 않았어요.")?;
                        tx.execute(
                            "UPDATE member_connected_brief_runs SET notification_id=?5 WHERE tenant_id=?1 AND account_id=?2 AND brief_id=?3 AND due_at=?4",
                            params![t, a, b.id, slot, notice],
                        )?;
                    }
                    end(&tx, owner, &b.id, "revoked", "needs_connection", time)?;
                    continue;
                }
                if !claim_slot(&tx, owner, &b.id, slot, "claimed", None, skipped, time)? {
                    advance(&tx, owner, b, next)?;
                    continue;
                }
                advance(&tx, owner, b, next)?;
                prune(&tx, owner, &b.id)?;
                let tz = zone(&b.zone).map_err(|_| StoreError::Corrupt)?;
                leases.push(MemberBriefLease {
                    owner,
                    auth_epoch: b.auth_epoch,
                    brief_id: Uuid::parse_str(&b.id).map_err(|_| StoreError::Corrupt)?,
                    due_at: slot,
                    thread_id: b.thread.clone(),
                    gmail: b.gmail,
                    calendar: b.calendar,
                    important_only: b.important_only && b.gmail.is_some(),
                    since_ms: (slot - b.interval.max(1) * 86400).saturating_mul(1000),
                    window: day_window(tz, slot)?,
                });
            }
            tx.commit()?;
            Ok(leases)
        })
        .await
    }

    /// Timer step 3. Evidence is kept only if the brief is still active and every pinned source is still
    /// the granted one; otherwise the result is discarded (`withdrawn`) and nothing is announced.
    pub async fn settle_member_brief(
        &self,
        lease: MemberBriefLease,
        outcome: MemberBriefOutcome,
    ) -> Result<()> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let owner = lease.owner;
            owner.check_identity(&tx, None)?;
            let (t, a, _, _) = own(owner);
            let id = lease.brief_id.to_string();
            let claimed: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM member_connected_brief_runs WHERE tenant_id=?1 AND account_id=?2 AND brief_id=?3 AND due_at=?4 AND state='claimed')",
                params![t, a, id, lease.due_at],
                |r| r.get(0),
            )?;
            if !claimed {
                // A restart or the stale-claim reaper already settled this slot as `unknown`.
                return Ok(());
            }
            let finish = |state: &str, guidance: Option<&str>, gmail: Option<i64>, calendar: Option<i64>, cipher: Option<Vec<u8>>, notice: Option<&str>| -> Result<()> {
                let n = tx.execute(
                    "UPDATE member_connected_brief_runs SET state=?5,freshness=?6,guidance=?7,gmail_count=?8,calendar_count=?9,cipher=?10,notification_id=?11,finished_at=unixepoch()
                     WHERE tenant_id=?1 AND account_id=?2 AND brief_id=?3 AND due_at=?4 AND state='claimed'",
                    params![t, a, id, lease.due_at, state, cipher.as_ref().map(|_| "live"), guidance, gmail, calendar, cipher, notice],
                )?;
                if n != 1 {
                    return Err(StoreError::Conflict);
                }
                Ok(())
            };
            let b = load(&tx, owner, &id)?;
            // The brief stays active until `expires_at`, including for the final slot. A stop, revocation,
            // deleted thread, or lost account authority while the read was in flight discards the result
            // and announces nothing (re-checked here, not merely at claim time).
            let thread_ok: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM threads th WHERE th.tenant_id=?1 AND th.id=?2 AND th.account_id=?3 AND th.mission_internal=0)",
                params![t, b.thread, a],
                |r| r.get(0),
            )?;
            let authorized: bool = tx.query_row(
                &format!("SELECT {AUTHORIZED} FROM member_connected_briefs b WHERE {OWNED} AND b.id=?5"),
                params![t, a, owner.volume.to_string(), owner.generation as i64, id],
                |r| r.get(0),
            )?;
            if b.state != "active" || b.expires <= now()? || !thread_ok || !authorized || !live(&tx, owner, &b) {
                finish("withdrawn", None, None, None, None, None)?;
                tx.commit()?;
                return Ok(());
            }
            let (state, guidance, detail, copy): (&str, Option<&str>, Option<MemberBriefDetail>, (&str, String)) = match outcome {
                MemberBriefOutcome::Unavailable(code @ ("needs_reauth" | "needs_connection")) => ("unavailable", Some(code), None, ("Google 다시 연결이 필요해요",
                    "Google 연결을 확인할 수 없어 브리핑을 만들지 못했어요. 대화에서 Gmail·캘린더를 다시 연결한 뒤 브리핑을 새로 허용해 주세요. 내용은 읽지 않았어요.".into())),
                MemberBriefOutcome::Unavailable(code) => ("unavailable", Some(code), None, ("브리핑을 만들지 못했어요", "Google을 지금 읽을 수 없었어요. 내용은 만들지 않았고 다음 일정에 다시 시도해요.".into())),
                MemberBriefOutcome::Evidence(e) => {
                    let messages = b.gmail.and(e.messages).map(|v| v.into_iter().take(MEMBER_BRIEF_MAX_MESSAGES).map(|m| MemberBriefMessage {
                        subject: clean(&m.subject), from: clean(&m.from), received_at_ms: m.received_at_ms.max(0),
                    }).collect::<Vec<_>>());
                    let events = b.calendar.and(e.events).map(|v| v.into_iter().take(MEMBER_BRIEF_MAX_EVENTS).map(|x| MemberBriefEvent {
                        title: clean(&x.title), start: clean(&x.start), end: clean(&x.end), all_day: x.all_day,
                    }).collect::<Vec<_>>());
                    let failed = (b.gmail.is_some() && messages.is_none()) || (b.calendar.is_some() && events.is_none());
                    let nothing = messages.is_none() && events.is_none();
                    let part = |label: &str, n: Option<usize>| n.map(|n| format!("{label} {n}건"));
                    let counts = [part(if b.important_only { "중요 메일" } else { "메일" }, messages.as_ref().map(Vec::len)), part("일정", events.as_ref().map(Vec::len))]
                        .into_iter().flatten().collect::<Vec<_>>().join(", ");
                    if nothing {
                        ("unavailable", Some("read_failed"), None, ("브리핑을 만들지 못했어요", "Google을 지금 읽을 수 없었어요. 내용은 만들지 않았고 다음 일정에 다시 시도해요.".into()))
                    } else if failed {
                        ("partial", Some("partial_read"), Some(MemberBriefDetail { captured_at: now()?, messages, events }),
                            ("브리핑을 일부만 확인했어요", format!("{counts}을 읽기 전용으로 확인했고, 나머지는 읽지 못했어요. 대화의 브리핑 기록을 확인하세요.")))
                    } else {
                        ("completed", None, Some(MemberBriefDetail { captured_at: now()?, messages, events }),
                            ("오늘의 메일·일정 브리핑이 준비되었어요", format!("{counts}을 읽기 전용으로 확인했어요. 내용은 대화의 브리핑 기록에서 확인하세요.")))
                    }
                }
            };
            let counts = detail.as_ref().map(|d| (d.messages.as_ref().map(|v| v.len() as i64), d.events.as_ref().map(|v| v.len() as i64)));
            let cipher = detail
                .as_ref()
                .map(|d| providers::encrypt(&db.provider_key, &serde_json::to_vec(d).map_err(|_| StoreError::Corrupt)?, &aad(owner, &b, lease.due_at)))
                .transpose()?;
            let notice = notice_id(&id, lease.due_at);
            finish(state, guidance, counts.and_then(|c| c.0), counts.and_then(|c| c.1), cipher, Some(&notice))?;
            // Same transaction and guarded by the single `claimed -> final` transition: one alert per slot.
            notify(&tx, owner, &id, &notice, copy.0, &copy.1)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests;
