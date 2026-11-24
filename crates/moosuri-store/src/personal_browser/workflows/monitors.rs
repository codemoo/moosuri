//! Member-owned change-only watches and daily local briefs.
//!
//! * A **watch** re-reads the member's own stored memory or conversation records on a fixed cadence
//!   (30 min, 1 h, 1 day) and notifies only when the verified source references (id + revision)
//!   differ from the last evidence. Unchanged checks write one ledger row and nothing else.
//! * A **brief** summarises the same kind of local records once per local day at a requested time in
//!   a browser-supplied IANA zone or fixed UTC offset. It never reads live mail or calendar.
//!
//! Neither kind contacts an external host. Both ride the existing member timer
//! (`tick_member_workflows` -> [`dispatch`]), so occurrences persist while the app is closed and are
//! committed with their run, notification and next due time in one transaction. Runs are ordinary
//! `member_work_runs`; source text is never stored or pushed, and references are re-read with live
//! authority whenever they are displayed.
//!
//! ## Narrow extension point
//! Evidence comes from `observe`, which only knows the two local scopes accepted by
//! `member_workflows.scope`. An authorized connector adapter (for example a synced, already-consented
//! Drive folder) must add its own scope value plus an adapter that returns verified `(id, revision)`
//! references through the same fingerprint path; it may not add URL probes or generic fetches.
use super::*;
use chrono::{DateTime, Days, LocalResult, NaiveDate, NaiveTime, TimeZone};
use moosuri_core::personal_browser::{WorkSchedule, WorkScope};

mod types;
pub use types::*;

/// Active monitors per member. Stopped, expired and revoked ones do not count.
const ACTIVE_LIMIT: i64 = 8;
/// Monitors checked per timer tick; the rest wait for the next tick.
const BATCH: i64 = 4;
/// A daily brief later than this after its local time is recorded as missed, not delivered stale.
const BRIEF_GRACE: i64 = 6 * 3600;
const KEEP_CHECKS: i64 = 60;
const RECENT_CHECKS: i64 = 8;
const LIST_LIMIT: i64 = 50;
const MAX_CATCHUP: usize = 400;

// ---------------------------------------------------------------------------------------------
// Civil time. Daily slots are local wall-clock instants: a nonexistent local time (spring gap) is
// skipped for that day, an ambiguous one (autumn fold) uses its first occurrence.
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Zone {
    Named(chrono_tz::Tz),
    Fixed(chrono::FixedOffset),
}

fn first_instant<T: TimeZone>(result: LocalResult<DateTime<T>>) -> Option<i64> {
    match result {
        LocalResult::Single(t) => Some(t.timestamp()),
        LocalResult::Ambiguous(a, b) => Some(a.timestamp().min(b.timestamp())),
        LocalResult::None => None,
    }
}

impl Zone {
    fn date(&self, instant: i64) -> Result<NaiveDate> {
        let utc = DateTime::from_timestamp(instant, 0).ok_or(StoreError::InvalidInput)?;
        Ok(match self {
            Zone::Named(z) => utc.with_timezone(z).date_naive(),
            Zone::Fixed(z) => utc.with_timezone(z).date_naive(),
        })
    }
    fn at(&self, date: NaiveDate, time: NaiveTime) -> Option<i64> {
        let local = date.and_time(time);
        match self {
            Zone::Named(z) => first_instant(z.from_local_datetime(&local)),
            Zone::Fixed(z) => first_instant(z.from_local_datetime(&local)),
        }
    }
}

/// `Asia/Seoul`, `UTC`, or a fixed offset such as `+09:00`, `+9`, `UTC+09:00`, `GMT-5`.
/// Returns the zone and its canonical spelling, which is what is stored and compared on replay.
fn parse_zone(raw: &str) -> Result<(Zone, String)> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 64 || raw.chars().any(char::is_control) {
        return Err(StoreError::InvalidInput);
    }
    let offset = raw
        .strip_prefix("UTC")
        .or_else(|| raw.strip_prefix("GMT"))
        .unwrap_or(raw);
    if let Some(sign) = offset
        .strip_prefix('+')
        .map(|_| 1i32)
        .or_else(|| offset.strip_prefix('-').map(|_| -1i32))
    {
        let body = &offset[1..];
        let (hours, minutes) = body.split_once(':').unwrap_or((body, "0"));
        let digits = |s: &str, max: usize| {
            (!s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_digit()))
                .then(|| s.parse::<i32>().ok())
                .flatten()
        };
        let h = digits(hours, 2).ok_or(StoreError::InvalidInput)?;
        let m = digits(minutes, 2).ok_or(StoreError::InvalidInput)?;
        let total = sign * (h * 60 + m);
        if m > 59 || !(-12 * 60..=14 * 60).contains(&total) {
            return Err(StoreError::InvalidInput);
        }
        let fixed = chrono::FixedOffset::east_opt(total * 60).ok_or(StoreError::InvalidInput)?;
        let canonical = format!(
            "{}{:02}:{:02}",
            if total < 0 { '-' } else { '+' },
            total.abs() / 60,
            total.abs() % 60
        );
        return Ok((Zone::Fixed(fixed), canonical));
    }
    let named: chrono_tz::Tz = raw.parse().map_err(|_| StoreError::InvalidInput)?;
    Ok((Zone::Named(named), named.name().to_owned()))
}

fn parse_clock(text: &str) -> Result<NaiveTime> {
    let time = NaiveTime::parse_from_str(text, "%H:%M").map_err(|_| StoreError::InvalidInput)?;
    (time.format("%H:%M").to_string() == text)
        .then_some(time)
        .ok_or(StoreError::InvalidInput)
}

/// First daily slot strictly after `after`.
fn slot_after(zone: &Zone, time: NaiveTime, after: i64) -> Result<i64> {
    let mut date = zone.date(after)?;
    for _ in 0..4 {
        if let Some(instant) = zone.at(date, time)
            && instant > after
        {
            return Ok(instant);
        }
        date = date
            .checked_add_days(Days::new(1))
            .ok_or(StoreError::InvalidInput)?;
    }
    Err(StoreError::InvalidInput)
}

// ---------------------------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------------------------

struct Monitor {
    id: String,
    kind: MonitorKind,
    title: String,
    scope: WorkScope,
    query: String,
    every: Option<i64>,
    local_time: Option<String>,
    zone: Option<String>,
    days: i64,
    state: String,
    revision: i64,
    created_at: i64,
    expires_at: i64,
    next_due: Option<i64>,
    fingerprint: Option<String>,
    last_run: Option<String>,
    last_checked: Option<i64>,
    last_changed: Option<i64>,
    alerts: i64,
}

const MONITOR: &str = "SELECT m.id,m.kind,w.title,w.scope,w.query,m.every_seconds,m.local_time,m.zone,m.days,m.state,m.revision,
 m.created_at,m.expires_at,m.next_due,m.fingerprint,m.last_run_id,m.last_checked_at,m.last_changed_at,m.alerts
 FROM member_work_monitors m JOIN member_workflows w ON w.id=m.id";
const OWNED: &str = "m.tenant_id=?1 AND m.account_id=?2 AND m.volume_id=?3 AND m.generation=?4";
const AUTHORIZED: &str = "EXISTS(SELECT 1 FROM accounts a WHERE a.id=m.account_id AND a.tenant_id=m.tenant_id
    AND a.auth_epoch=m.auth_epoch AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)";

fn bad(column: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, what.into())
}

fn monitor_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Monitor> {
    Ok(Monitor {
        id: r.get(0)?,
        kind: match r.get::<_, String>(1)?.as_str() {
            "watch" => MonitorKind::Watch,
            "brief" => MonitorKind::Brief,
            _ => return Err(bad(1, "kind")),
        },
        title: r.get(2)?,
        scope: match r.get::<_, String>(3)?.as_str() {
            "memory" => WorkScope::Memory,
            "conversation" => WorkScope::Conversation,
            _ => return Err(bad(3, "scope")),
        },
        query: r.get(4)?,
        every: r.get(5)?,
        local_time: r.get(6)?,
        zone: r.get(7)?,
        days: r.get(8)?,
        state: r.get(9)?,
        revision: r.get(10)?,
        created_at: r.get(11)?,
        expires_at: r.get(12)?,
        next_due: r.get(13)?,
        fingerprint: r.get(14)?,
        last_run: r.get(15)?,
        last_checked: r.get(16)?,
        last_changed: r.get(17)?,
        alerts: r.get(18)?,
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

fn load(c: &Connection, owner: VolumeOwner, id: &str) -> Result<Monitor> {
    let (t, a, v, g) = own(owner);
    c.query_row(
        &format!("{MONITOR} WHERE {OWNED} AND m.id=?5"),
        params![t, a, v, g, id],
        monitor_row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

fn scope_text(scope: &WorkScope) -> &'static str {
    match scope {
        WorkScope::Memory => "memory",
        WorkScope::Conversation => "conversation",
    }
}

fn basis(scope: &WorkScope) -> String {
    format!(
        "내가 저장한 {}만 확인합니다. 외부 사이트, 메일, 캘린더는 읽지 않습니다.",
        match scope {
            WorkScope::Memory => "기억",
            WorkScope::Conversation => "대화 기록",
        }
    )
}

// ---------------------------------------------------------------------------------------------
// Request validation
// ---------------------------------------------------------------------------------------------

struct Spec {
    clock: Option<String>,
    zone: Option<String>,
}

fn validate(request: &MonitorRequest) -> Result<Spec> {
    canonical(&request.request_id)?;
    if !text_valid(&request.title, 240)
        || !text_valid(&request.query, 128)
        || !(1..=90).contains(&request.days)
    {
        return Err(StoreError::InvalidInput);
    }
    match request.kind {
        MonitorKind::Watch => {
            if !matches!(request.every_seconds, Some(1800 | 3600 | 86400))
                || request.at.is_some()
                || request.timezone.is_some()
            {
                return Err(StoreError::InvalidInput);
            }
            Ok(Spec {
                clock: None,
                zone: None,
            })
        }
        MonitorKind::Brief => {
            let (Some(at), Some(zone), None) =
                (&request.at, &request.timezone, request.every_seconds)
            else {
                return Err(StoreError::InvalidInput);
            };
            parse_clock(at)?;
            Ok(Spec {
                clock: Some(at.clone()),
                zone: Some(parse_zone(zone)?.1),
            })
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Evidence and occurrences
// ---------------------------------------------------------------------------------------------

struct Observed {
    sources: Vec<SourceRef>,
    limited: bool,
    fingerprint: String,
    started: i64,
    searched: i64,
}

/// Bounded local read through the same search and live source check as every other member run.
fn observe(c: &Connection, owner: VolumeOwner, m: &Monitor) -> Result<Observed> {
    let schedule = WorkSchedule {
        id: m.id.clone(),
        title: m.title.clone(),
        scope: m.scope.clone(),
        query: m.query.clone(),
        period: 0,
        next_due: None,
        enabled: false,
        revision: m.revision,
    };
    let started = now()?;
    let (found, limited) = search(c, owner, &schedule, &[])?;
    let searched = now()?;
    let mut sources = Vec::new();
    for mut source in found {
        if read_source(c, owner, &schedule.scope, &source)?.is_some() {
            source.read_at = now()?;
            sources.push(source);
        }
    }
    Ok(Observed {
        fingerprint: fingerprint(&m.scope, &m.query, limited, &sources),
        sources,
        limited,
        started,
        searched,
    })
}

/// Identity of the evidence: which records, at which revisions. Text never enters the digest input.
fn fingerprint(scope: &WorkScope, query: &str, limited: bool, sources: &[SourceRef]) -> String {
    let mut parts: Vec<String> = sources
        .iter()
        .map(|s| format!("{}:{}", s.id, s.revision))
        .collect();
    parts.sort();
    let text = format!(
        "{}\n{}\n{}\n{}",
        scope_text(scope),
        query,
        u8::from(limited),
        parts.join(",")
    );
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn insert_run(
    c: &Connection,
    workflow: &str,
    request: Option<&str>,
    due: Option<i64>,
    o: &Observed,
) -> Result<String> {
    let id = Uuid::new_v4().to_string();
    let refs = serde_json::to_string(&o.sources).map_err(|_| StoreError::Corrupt)?;
    c.execute(
        "INSERT INTO member_work_runs(id,workflow_id,request_id,due_at,created_at,admitted_order,sources,limited,searched_at,finished_at)
         VALUES(?1,?2,?3,?4,?8,?5,?6,?7,?9,unixepoch())",
        params![id, workflow, request, due, chat::next_order(c)?, refs, o.limited, o.started, o.searched],
    )?;
    Ok(id)
}

struct Occurrence {
    /// The latest due instant that has passed; earlier ones are only counted.
    slot: i64,
    skipped: i64,
    /// First instant strictly after `time`.
    next: i64,
}

fn occurrence(m: &Monitor, time: i64) -> Result<Occurrence> {
    let first = m.next_due.ok_or(StoreError::Corrupt)?;
    if let Some(period) = m.every {
        let n = ((time - first) / period).max(0);
        let slot = first + n * period;
        return Ok(Occurrence {
            slot,
            skipped: n,
            next: slot + period,
        });
    }
    let (zone, clock) = brief_parts(m)?;
    let (mut slot, mut skipped) = (first, 0);
    for _ in 0..MAX_CATCHUP {
        let next = slot_after(&zone, clock, slot)?;
        if next > time {
            return Ok(Occurrence {
                slot,
                skipped,
                next,
            });
        }
        slot = next;
        skipped += 1;
    }
    Err(StoreError::Corrupt)
}

fn brief_parts(m: &Monitor) -> Result<(Zone, NaiveTime)> {
    let clock = parse_clock(m.local_time.as_deref().ok_or(StoreError::Corrupt)?)
        .map_err(|_| StoreError::Corrupt)?;
    let (zone, _) = parse_zone(m.zone.as_deref().ok_or(StoreError::Corrupt)?)
        .map_err(|_| StoreError::Corrupt)?;
    Ok((zone, clock))
}

// ---------------------------------------------------------------------------------------------
// Ledger, notifications and lifecycle
// ---------------------------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn record(
    c: &Connection,
    monitor: &str,
    due: i64,
    at: i64,
    outcome: &str,
    count: usize,
    skipped: i64,
    run: Option<&str>,
    notice: Option<&str>,
) -> Result<()> {
    c.execute(
        "INSERT INTO member_work_monitor_checks(monitor_id,due_at,checked_at,outcome,source_count,skipped,run_id,notification_id)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![monitor, due, at, outcome, count as i64, skipped, run, notice],
    )?;
    c.execute(
        "DELETE FROM member_work_monitor_checks WHERE monitor_id=?1 AND due_at<
         (SELECT due_at FROM member_work_monitor_checks WHERE monitor_id=?1 ORDER BY due_at DESC LIMIT 1 OFFSET ?2)",
        params![monitor, KEEP_CHECKS - 1],
    )?;
    Ok(())
}

/// Inbox/push notice through the shared quiet-hours and mute rules. The text is generic on purpose:
/// it carries the member's own label and a count, never any source text.
fn notify(
    c: &Connection,
    owner: VolumeOwner,
    id: &str,
    source: &str,
    title: &str,
    message: &str,
) -> Result<()> {
    let (t, a, _, _) = own(owner);
    crate::schedules::direct_notice(c, &t, &a, source, id, title, message)
}

fn label_of(scope: &WorkScope) -> &'static str {
    match scope {
        WorkScope::Memory => "기억",
        WorkScope::Conversation => "대화",
    }
}

fn short(title: &str) -> String {
    title.chars().take(40).collect()
}

fn suppress_push(c: &Connection, owner: VolumeOwner, id: &str, time: i64) -> Result<()> {
    let (t, _, _, _) = own(owner);
    c.execute(
        "UPDATE local_notifications SET push_suppressed=1 WHERE tenant_id=?1 AND schedule_id=?2 AND push_suppressed=0",
        params![t, id],
    )?;
    c.execute(
        "UPDATE push_queue SET state='suppressed',nonce=NULL,deadline=NULL,finished_at=?3,reason='monitor_stopped'
         WHERE tenant_id=?1 AND state IN('queued','claimed')
           AND notification_id IN(SELECT id FROM local_notifications WHERE tenant_id=?1 AND schedule_id=?2)",
        params![t, id, time],
    )?;
    Ok(())
}

/// Moves an active monitor to a final state. Stopping or revoking also cancels undelivered pushes.
fn end(c: &Connection, owner: VolumeOwner, id: &str, state: &str, time: i64) -> Result<()> {
    let changed = c.execute(
        "UPDATE member_work_monitors SET state=?2,next_due=NULL,ended_at=?3,revision=revision+1 WHERE id=?1 AND state='active'",
        params![id, state, time],
    )?;
    if changed == 1 && matches!(state, "stopped" | "revoked") {
        suppress_push(c, owner, id, time)?;
    }
    Ok(())
}

fn advance(c: &Connection, owner: VolumeOwner, m: &Monitor, next: i64, time: i64) -> Result<()> {
    if next > m.expires_at {
        end(c, owner, &m.id, "expired", time)
    } else {
        c.execute(
            "UPDATE member_work_monitors SET next_due=?2 WHERE id=?1",
            params![m.id, next],
        )?;
        Ok(())
    }
}

/// One occurrence. Everything it writes commits or rolls back together (caller's savepoint).
fn step(
    c: &Connection,
    owner: VolumeOwner,
    m: &Monitor,
    occ: &Occurrence,
    time: i64,
) -> Result<()> {
    if m.kind == MonitorKind::Brief && time - occ.slot > BRIEF_GRACE {
        // The member's day has moved on; do not deliver a stale morning brief.
        record(
            c,
            &m.id,
            occ.slot,
            time,
            "missed",
            0,
            occ.skipped,
            None,
            None,
        )?;
        return advance(c, owner, m, occ.next, time);
    }
    let observed = observe(c, owner, m)?;
    let count = observed.sources.len();
    let mark = |c: &Connection, changed: bool, run: Option<&str>| -> Result<()> {
        c.execute(
            "UPDATE member_work_monitors SET fingerprint=?2,source_count=?3,last_checked_at=?4,
                last_changed_at=CASE WHEN ?5 THEN ?4 ELSE last_changed_at END,
                last_run_id=coalesce(?6,last_run_id),alerts=alerts+?5 WHERE id=?1",
            params![m.id, observed.fingerprint, count as i64, time, changed, run],
        )?;
        Ok(())
    };
    match m.kind {
        MonitorKind::Watch => match m.fingerprint.as_deref() {
            Some(previous) if previous == observed.fingerprint => {
                record(
                    c,
                    &m.id,
                    occ.slot,
                    time,
                    "unchanged",
                    count,
                    occ.skipped,
                    None,
                    None,
                )?;
                mark(c, false, None)?;
            }
            None => {
                let run = insert_run(c, &m.id, None, Some(occ.slot), &observed)?;
                record(
                    c,
                    &m.id,
                    occ.slot,
                    time,
                    "baseline",
                    count,
                    occ.skipped,
                    Some(run.as_str()),
                    None,
                )?;
                mark(c, false, Some(run.as_str()))?;
            }
            Some(_) => {
                let run = insert_run(c, &m.id, None, Some(occ.slot), &observed)?;
                let notice = Uuid::new_v4().to_string();
                notify(
                    c,
                    owner,
                    &notice,
                    &m.id,
                    "감시 중인 자료가 바뀌었어요",
                    &format!(
                        "‘{}’ 감시에서 변화가 확인되었어요. 현재 확인된 {} {count}건을 업무 기록에서 확인하세요.",
                        short(&m.title),
                        label_of(&m.scope)
                    ),
                )?;
                record(
                    c,
                    &m.id,
                    occ.slot,
                    time,
                    "changed",
                    count,
                    occ.skipped,
                    Some(run.as_str()),
                    Some(notice.as_str()),
                )?;
                mark(c, true, Some(run.as_str()))?;
            }
        },
        MonitorKind::Brief => {
            let run = insert_run(c, &m.id, None, Some(occ.slot), &observed)?;
            let notice = Uuid::new_v4().to_string();
            notify(
                c,
                owner,
                &notice,
                &m.id,
                "오늘의 브리핑이 준비되었어요",
                &format!(
                    "‘{}’ 관련 저장된 {} {count}건을 확인했어요. 메일과 캘린더는 읽지 않았어요. 업무 기록에서 확인하세요.",
                    short(&m.title),
                    label_of(&m.scope)
                ),
            )?;
            record(
                c,
                &m.id,
                occ.slot,
                time,
                "briefed",
                count,
                occ.skipped,
                Some(run.as_str()),
                Some(notice.as_str()),
            )?;
            mark(c, false, Some(run.as_str()))?;
        }
    }
    advance(c, owner, m, occ.next, time)
}

/// Timer entry point, called inside the member tick's own transaction with an explicit clock.
/// Revokes monitors granted under an older host epoch or auth epoch, expires finished ones, then
/// checks a bounded batch of due monitors. A failing check records `failed` and never alerts.
pub(in crate::personal_browser) fn dispatch(
    tx: &Connection,
    owner: VolumeOwner,
    host_epoch: i64,
    time: i64,
) -> Result<usize> {
    let (t, a, v, g) = own(owner);
    let revoked = tx
        .prepare(&format!(
            "SELECT m.id FROM member_work_monitors m WHERE {OWNED} AND m.state='active'
             AND (m.host_epoch<?5 OR NOT {AUTHORIZED}) ORDER BY m.id LIMIT 64"
        ))?
        .query_map(params![t, a, v, g, host_epoch], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in &revoked {
        end(tx, owner, id, "revoked", time)?;
    }
    let expired = tx
        .prepare(&format!(
            "SELECT m.id FROM member_work_monitors m WHERE {OWNED} AND m.state='active' AND m.expires_at<=?5
             ORDER BY m.id LIMIT 64"
        ))?
        .query_map(params![t, a, v, g, time], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for id in &expired {
        end(tx, owner, id, "expired", time)?;
    }
    let due = tx
        .prepare(&format!(
            "{MONITOR} WHERE {OWNED} AND m.state='active' AND m.next_due<=?5 AND m.host_epoch=?6
             AND {AUTHORIZED} ORDER BY m.next_due,m.id LIMIT ?7"
        ))?
        .query_map(params![t, a, v, g, time, host_epoch, BATCH], monitor_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for m in &due {
        let Ok(occ) = occurrence(m, time) else {
            end(tx, owner, &m.id, "revoked", time)?;
            continue;
        };
        tx.execute_batch("SAVEPOINT member_monitor")?;
        match step(tx, owner, m, &occ, time) {
            Ok(()) => tx.execute_batch("RELEASE member_monitor")?,
            Err(_) => {
                tx.execute_batch("ROLLBACK TO member_monitor; RELEASE member_monitor")?;
                record(
                    tx,
                    &m.id,
                    occ.slot,
                    time,
                    "failed",
                    0,
                    occ.skipped,
                    None,
                    None,
                )?;
                advance(tx, owner, m, occ.next, time)?;
            }
        }
    }
    Ok(due.len())
}

/// Live-authority check for a monitor's notification, shared with inbox and push dispatch.
/// `None` means the notification does not belong to a member monitor.
pub(crate) fn notice_current(c: &Connection, tenant: &str, id: &str) -> Result<Option<bool>> {
    Ok(c.query_row(
        "SELECT m.state!='revoked' AND EXISTS(SELECT 1 FROM accounts a WHERE a.id=m.account_id AND a.tenant_id=m.tenant_id
            AND a.auth_epoch=m.auth_epoch AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)
         FROM local_notifications n JOIN member_work_monitors m ON m.tenant_id=n.tenant_id AND m.id=n.schedule_id
         WHERE n.tenant_id=?1 AND n.id=?2",
        params![tenant, id],
        |r| r.get::<_, bool>(0),
    )
    .optional()?)
}

// ---------------------------------------------------------------------------------------------
// Views
// ---------------------------------------------------------------------------------------------

fn evidence(c: &Connection, owner: VolumeOwner, run: &str) -> Result<Option<MonitorEvidence>> {
    let ev = match run_evidence(c, owner, run) {
        Ok(ev) => ev,
        Err(StoreError::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut sources = Vec::new();
    for source in &ev.sources {
        let current = read_source(c, owner, &ev.scope, source)?;
        sources.push(MonitorSourceRef {
            id: source.id.clone(),
            revision: source.revision,
            available: current.is_some(),
            // A withdrawn or changed source returns neither title nor text.
            title: current.map(|(title, _)| title),
        });
    }
    Ok(Some(MonitorEvidence {
        run_id: run.to_owned(),
        read_at: ev.finished_at,
        limited: ev.limited,
        sources,
    }))
}

fn checks(c: &Connection, id: &str, limit: i64) -> Result<Vec<MonitorCheck>> {
    Ok(c.prepare(
        "SELECT due_at,checked_at,outcome,source_count,skipped,run_id,notification_id IS NOT NULL
         FROM member_work_monitor_checks WHERE monitor_id=?1 ORDER BY due_at DESC LIMIT ?2",
    )?
    .query_map(params![id, limit], |r| {
        Ok(MonitorCheck {
            due_at: r.get(0)?,
            checked_at: r.get(1)?,
            outcome: r.get(2)?,
            source_count: r.get(3)?,
            skipped: r.get(4)?,
            run_id: r.get(5)?,
            notified: r.get(6)?,
        })
    })?
    .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn view(
    c: &Connection,
    owner: VolumeOwner,
    m: Monitor,
    time: i64,
    recent: i64,
) -> Result<MonitorView> {
    // A lapsed expiry is reported even before the timer has recorded it.
    let lapsed = m.state == "active" && m.expires_at <= time;
    let latest = match &m.last_run {
        Some(run) => evidence(c, owner, run)?,
        None => None,
    };
    Ok(MonitorView {
        recent: checks(c, &m.id, recent)?,
        latest,
        basis: basis(&m.scope),
        state: if lapsed { "expired".into() } else { m.state },
        next_due: if lapsed { None } else { m.next_due },
        id: m.id,
        kind: m.kind,
        title: m.title,
        scope: m.scope,
        query: m.query,
        every_seconds: m.every,
        at: m.local_time,
        timezone: m.zone,
        days: m.days,
        revision: m.revision,
        created_at: m.created_at,
        expires_at: m.expires_at,
        last_checked_at: m.last_checked,
        last_changed_at: m.last_changed,
        alerts: m.alerts,
    })
}

impl Store {
    /// Creates a monitor, or replays the identical earlier request. A watch records its baseline
    /// immediately so the first scheduled check can already tell a change from the starting state.
    pub async fn create_member_monitor(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        request: MonitorRequest,
    ) -> Result<MonitorView> {
        let spec = validate(&request)?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            if let Some(thread) = &request.thread_id {
                canonical(thread)?;
                chat::thread_exists(&tx, &user, thread)?;
                if !request.message.as_ref().is_some_and(|m| !m.trim().is_empty() && m.len() <= 8192) {
                    return Err(StoreError::InvalidInput);
                }
            } else if request.message.is_some() { return Err(StoreError::InvalidInput); }
            let (t, a, v, g) = own(owner);
            let scope = scope_text(&request.scope);
            let same = tx
                .query_row(
                    "SELECT w.tenant_id=?2 AND w.account_id=?3 AND w.volume_id=?4 AND w.generation=?5
                        AND w.title=?6 AND w.scope=?7 AND w.query=?8 AND w.thread_id IS ?14 AND w.message IS ?15 AND w.period=0
                        AND m.kind=?9 AND m.every_seconds IS ?10 AND m.local_time IS ?11 AND m.zone IS ?12 AND m.days=?13
                     FROM member_workflows w LEFT JOIN member_work_monitors m ON m.id=w.id WHERE w.id=?1",
                    params![
                        request.request_id,
                        t,
                        a,
                        v,
                        g,
                        request.title,
                        scope,
                        request.query,
                        match request.kind {
                            MonitorKind::Watch => "watch",
                            MonitorKind::Brief => "brief",
                        },
                        request.every_seconds,
                        spec.clock,
                        spec.zone,
                        request.days, request.thread_id, request.message
                    ],
                    |r| r.get::<_, Option<bool>>(0),
                )
                .optional()?;
            let time = now()?;
            match same {
                Some(Some(true)) => {}
                Some(_) => return Err(StoreError::Conflict),
                None => {
                    let active: i64 = tx.query_row(
                        &format!("SELECT count(*) FROM member_work_monitors m WHERE {OWNED} AND m.state='active' AND m.expires_at>?5"),
                        params![t, a, v, g, time],
                        |r| r.get(0),
                    )?;
                    if active >= ACTIVE_LIMIT {
                        return Err(StoreError::BudgetExhausted);
                    }
                    let next = match request.kind {
                        MonitorKind::Watch => time + request.every_seconds.unwrap_or(0),
                        MonitorKind::Brief => {
                            let (zone, _) = parse_zone(spec.zone.as_deref().unwrap_or(""))?;
                            slot_after(&zone, parse_clock(spec.clock.as_deref().unwrap_or(""))?, time)?
                        }
                    };
                    tx.execute(
                        "INSERT INTO member_workflows(id,tenant_id,account_id,volume_id,generation,auth_epoch,title,scope,query,
                            period,first_due,next_due,enabled,thread_id,message,host_epoch)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,0,NULL,NULL,0,?11,?12,?10)",
                        params![request.request_id, t, a, v, g, user.epoch, request.title, scope, request.query, host_epoch, request.thread_id, request.message],
                    )?;
                    if let (Some(thread), Some(message)) = (&request.thread_id, &request.message) {
                        member_work_turn(&tx, &user, thread, &Uuid::new_v4().to_string(), &request.request_id,
                            message, "member-monitor", format!("{}을 등록했습니다. 맡긴 일에서 확인할 수 있습니다.", request.title))?;
                    }
                    tx.execute(
                        "INSERT INTO member_work_monitors(id,tenant_id,account_id,volume_id,generation,auth_epoch,host_epoch,kind,
                            every_seconds,local_time,zone,days,state,created_at,expires_at,next_due)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,'active',?13,?14,?15)",
                        params![
                            request.request_id,
                            t,
                            a,
                            v,
                            g,
                            user.epoch,
                            host_epoch,
                            match request.kind {
                                MonitorKind::Watch => "watch",
                                MonitorKind::Brief => "brief",
                            },
                            request.every_seconds,
                            spec.clock,
                            spec.zone,
                            request.days,
                            time,
                            time + request.days * 86400,
                            next
                        ],
                    )?;
                    if request.kind == MonitorKind::Watch {
                        let m = load(&tx, owner, &request.request_id)?;
                        let observed = observe(&tx, owner, &m)?;
                        let run = insert_run(&tx, &m.id, Some(&m.id), None, &observed)?;
                        record(&tx, &m.id, time, time, "baseline", observed.sources.len(), 0, Some(run.as_str()), None)?;
                        tx.execute(
                            "UPDATE member_work_monitors SET fingerprint=?2,source_count=?3,last_checked_at=?4,last_run_id=?5 WHERE id=?1",
                            params![m.id, observed.fingerprint, observed.sources.len() as i64, time, run],
                        )?;
                    }
                }
            }
            let out = view(&tx, owner, load(&tx, owner, &request.request_id)?, time, RECENT_CHECKS)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    pub async fn member_monitors(
        &self,
        owner: VolumeOwner,
        user: UserContext,
    ) -> Result<MonitorPage> {
        self.call(move |db| {
            let tx = db.conn.transaction()?;
            authorize_member(&tx, owner, &user)?;
            let (t, a, v, g) = own(owner);
            let rows = tx
                .prepare(&format!(
                    "{MONITOR} WHERE {OWNED} ORDER BY (m.state='active') DESC,m.created_at DESC,m.id LIMIT ?5"
                ))?
                .query_map(params![t, a, v, g, LIST_LIMIT], monitor_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let time = now()?;
            let mut items = Vec::new();
            for m in rows {
                items.push(view(&tx, owner, m, time, 3)?);
            }
            tx.commit()?;
            Ok(MonitorPage {
                items,
                active_limit: ACTIVE_LIMIT,
            })
        })
        .await
    }

    pub async fn member_monitor(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        id: Uuid,
    ) -> Result<MonitorView> {
        self.call(move |db| {
            let tx = db.conn.transaction()?;
            authorize_member(&tx, owner, &user)?;
            let out = view(
                &tx,
                owner,
                load(&tx, owner, &id.to_string())?,
                now()?,
                RECENT_CHECKS,
            )?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }

    /// Stops an active monitor and cancels its undelivered pushes. Replays and requests against an
    /// already ended monitor return its current state; a stale revision on a live one conflicts.
    pub async fn stop_member_monitor(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        id: Uuid,
        stop: MonitorStop,
    ) -> Result<MonitorView> {
        canonical(&stop.request_id)?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            let id = id.to_string();
            let time = now()?;
            let m = load(&tx, owner, &id)?;
            if m.state == "active" {
                if m.expires_at <= time {
                    end(&tx, owner, &id, "expired", time)?;
                } else {
                    if stop.revision != m.revision {
                        return Err(StoreError::Conflict);
                    }
                    end(&tx, owner, &id, "stopped", time)?;
                    tx.execute(
                        "UPDATE member_work_monitors SET stop_request_id=?2 WHERE id=?1",
                        params![id, stop.request_id],
                    )?;
                }
            }
            let out = view(&tx, owner, load(&tx, owner, &id)?, time, RECENT_CHECKS)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
}

#[cfg(test)]
mod tests;
