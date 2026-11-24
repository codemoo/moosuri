//! Member primary-calendar create/move ledger (schema 96). Tokens stay in the
//! existing `member_google_grants` row (service `calendar`); this module adds only
//! opaque candidate handles and a per-request dispatch ledger. Dispatch is
//! committed *before* provider IO; a lost acknowledgement is resolved by readback.
//! Titles/times are encrypted at rest and never enter chat, memory or model context.
use super::*;
use chrono::{NaiveDate, NaiveTime, TimeZone};
use moosuri_connectors::calendar::{event_id as valid_event_id, member::stable_event_id};
use moosuri_core::member_calendar::MemberCalendarKind as Kind;
use moosuri_core::{CalendarEvent, CalendarFields};

const CANDIDATE_TTL: i64 = 600;
const MAX_CANDIDATES: i64 = 64;
const MAX_ACTIONS: i64 = 1024;
/// Seconds an in-flight dispatch may stay unsettled before readback is allowed.
const DISPATCH_GRACE: i64 = 120;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn aad(u: &UserContext, id: &str, field: &str) -> Vec<u8> {
    format!(
        "moosuri/member-calendar/v1/{}/{}/{id}/{field}",
        u.tenant.0, u.account.id
    )
    .into_bytes()
}

/// Everything a write must reproduce on reconcile. Encrypted in `payload`.
#[derive(Clone, Serialize, Deserialize)]
pub struct MemberCalendarIntent {
    pub request_id: Uuid,
    pub kind: Kind,
    pub event_id: String,
    pub before_etag: Option<String>,
    pub fields: CalendarFields,
}
impl MemberCalendarIntent {
    /// Binds the exact final content and exact source connection generation.
    pub fn digest(&self, connection_id: Uuid, epoch: i64) -> String {
        let bytes = serde_json::to_vec(&(
            "moosuri/member-calendar/digest/v1",
            self.kind.as_str(),
            &self.event_id,
            &self.before_etag,
            &self.fields.title,
            &self.fields.start,
            &self.fields.end,
            &self.fields.time_zone,
            connection_id,
            epoch,
        ))
        .unwrap_or_default();
        hex(&Sha256::digest(bytes))
    }
    /// Private provider-side marker. Equals the request id (36 chars).
    pub fn marker(&self) -> String {
        self.request_id.to_string()
    }
    fn valid(&self) -> bool {
        let f = &self.fields;
        moosuri_connectors::calendar::validate_fields(f).is_ok()
            && f.time_zone.is_some()
            && !self.request_id.is_nil()
            && match self.kind {
                Kind::Create => {
                    self.before_etag.is_none()
                        && self.event_id == stable_event_id(&self.request_id.to_string())
                }
                Kind::Move => {
                    valid_event_id(&self.event_id).is_ok()
                        && self
                            .before_etag
                            .as_ref()
                            .is_some_and(|e| !e.is_empty() && e.len() <= 512)
                }
            }
    }
}

#[derive(Clone, Serialize)]
pub struct MemberCalendarRecord {
    pub request_id: Uuid,
    pub kind: Kind,
    /// Digest of the approved final content + exact source generation.
    pub digest: String,
    /// dispatch_committed | applied | conflict | failed_definite | outcome_unknown | withdrawn
    pub state: String,
    /// `provider_response` or `readback`. Only present when `applied`.
    pub confirmed_by: Option<String>,
    pub provider_etag: Option<String>,
    pub receipt: Option<String>,
    pub error: Option<String>,
    pub event: Option<CalendarEvent>,
    pub created_at: i64,
    pub version: i64,
}
pub enum MemberCalendarAdmission {
    Fresh(MemberCalendarLease),
    Existing(MemberCalendarRecord),
}
pub struct MemberCalendarLease {
    user: UserContext,
    owner: VolumeOwner,
    app: Uuid,
    connection_id: Uuid,
    epoch: i64,
    version: i64,
    pub intent: MemberCalendarIntent,
}
pub enum MemberCalendarSettle {
    Applied {
        event: Box<CalendarEvent>,
        by: &'static str,
    },
    /// Definite no-write: stale ETag / precondition failed.
    Conflict,
    /// Definite no-write: provider rejected (never sent / refused).
    Failed(&'static str),
    /// A write may have landed. Reconcile by readback only.
    Unknown(&'static str),
}

struct Saved {
    kind: Kind,
    volume: String,
    generation: i64,
    auth_epoch: i64,
    session: Vec<u8>,
    app: String,
    connection: String,
    connection_epoch: i64,
    event_id: String,
    before_etag: Option<String>,
    digest: String,
    state: String,
    confirmed_by: Option<String>,
    provider_etag: Option<String>,
    receipt: Option<String>,
    error: Option<String>,
    payload: Option<Vec<u8>>,
    event: Option<Vec<u8>>,
    version: i64,
    created_at: i64,
}
fn load(c: &Connection, u: &UserContext, id: Uuid) -> Result<Option<Saved>> {
    c.query_row("SELECT kind,volume_id,generation,auth_epoch,session_digest,app_revision,connection_id,connection_epoch,event_id,before_etag,digest,state,confirmed_by,provider_etag,receipt,error,payload,event,version,created_at FROM member_calendar_actions WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",
        params![u.tenant.0,u.account.id,id.to_string()],
        |r|Ok(Saved{
            kind:match r.get::<_,String>(0)?.as_str(){"create"=>Kind::Create,"move"=>Kind::Move,_=>return Err(rusqlite::Error::InvalidQuery)},
            volume:r.get(1)?,generation:r.get(2)?,auth_epoch:r.get(3)?,session:r.get(4)?,app:r.get(5)?,connection:r.get(6)?,connection_epoch:r.get(7)?,
            event_id:r.get(8)?,before_etag:r.get(9)?,digest:r.get(10)?,state:r.get(11)?,confirmed_by:r.get(12)?,provider_etag:r.get(13)?,receipt:r.get(14)?,error:r.get(15)?,
            payload:r.get(16)?,event:r.get(17)?,version:r.get(18)?,created_at:r.get(19)?,
        })).optional().map_err(Into::into)
}
/// The ledger row still belongs to this session, owner generation and live grant.
fn session_matches(s: &Saved, u: &UserContext, o: VolumeOwner) -> bool {
    s.session == u.digest
        && s.auth_epoch == u.epoch
        && s.volume == o.volume.to_string()
        && s.generation == o.generation as i64
}
fn source_live(c: &Connection, u: &UserContext, o: VolumeOwner, s: &Saved) -> Result<bool> {
    Ok(row(c, u, Service::Calendar)?.is_some_and(|r| {
        matches(&r, o, r.app)
            && r.app.to_string() == s.app
            && r.status == "active"
            && r.auth_epoch == u.epoch
            && r.id.map(|v| v.to_string()) == Some(s.connection.clone())
            && r.epoch == s.connection_epoch
    }))
}
/// An admitted dispatch whose settle never arrived (interrupted/disconnected
/// without a service restart) becomes `outcome_unknown` once `updated_at` is older
/// than `DISPATCH_GRACE`. Only readback may follow; no new provider write is ever
/// granted. The version bump invalidates a late lease from the original attempt,
/// and callers run this only after the session/grant/digest checks.
fn age_dispatch(c: &Connection, u: &UserContext, id: Uuid, s: &mut Saved) -> Result<()> {
    if s.state != "dispatch_committed" {
        return Ok(());
    }
    let n = c.execute("UPDATE member_calendar_actions SET state='outcome_unknown',error='dispatch_aged',version=version+1,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3 AND state='dispatch_committed' AND updated_at<=unixepoch()-?4",params![u.tenant.0,u.account.id,id.to_string(),DISPATCH_GRACE])?;
    if n == 1 {
        *s = load(c, u, id)?.ok_or(StoreError::Corrupt)?;
    }
    Ok(())
}
fn purge_row(c: &Connection, u: &UserContext, id: Uuid, why: &str) -> Result<()> {
    c.execute("UPDATE member_calendar_actions SET state='withdrawn',confirmed_by=NULL,payload=NULL,event=NULL,provider_etag=NULL,error=?4,version=version+1,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3 AND state!='withdrawn'",params![u.tenant.0,u.account.id,id.to_string(),why])?;
    Ok(())
}
fn record(key: &[u8; 32], u: &UserContext, id: Uuid, s: &Saved) -> Result<MemberCalendarRecord> {
    let event = match (&s.event, s.state.as_str()) {
        (Some(blob), "applied") => {
            let bytes = providers::decrypt(key, blob, &aad(u, &id.to_string(), "event"))?;
            Some(serde_json::from_slice(&bytes).map_err(|_| StoreError::Corrupt)?)
        }
        _ => None,
    };
    Ok(MemberCalendarRecord {
        request_id: id,
        kind: s.kind,
        digest: s.digest.clone(),
        state: s.state.clone(),
        confirmed_by: s.confirmed_by.clone(),
        provider_etag: s.provider_etag.clone(),
        receipt: s.receipt.clone(),
        error: s.error.clone(),
        event,
        created_at: s.created_at,
        version: s.version,
    })
}
/// Read the settled provider event through its original member and current grant.
pub(crate) fn member_calendar_work_result(
    c: &Connection,
    key: &[u8; 32],
    u: &UserContext,
    owner: VolumeOwner,
    id: Uuid,
) -> Result<Option<String>> {
    let Some(saved) = load(c, u, id)? else {
        return Ok(None);
    };
    if saved.state != "applied"
        || saved.auth_epoch != u.epoch
        || saved.volume != owner.volume.to_string()
        || saved.generation != owner.generation as i64
        || !source_live(c, u, owner, &saved)?
    {
        return Ok(None);
    }
    let receipt = record(key, u, id, &saved)?;
    let Some(event) = receipt.event else {
        return Err(StoreError::Corrupt);
    };
    let action = if receipt.kind == Kind::Create {
        "생성"
    } else {
        "이동"
    };
    Ok(Some(format!(
        "Google 캘린더에서 {action} 확인 · {}\n{} ~ {}{}",
        event.fields.title,
        event.fields.start,
        event.fields.end,
        event
            .fields
            .time_zone
            .as_deref()
            .map(|zone| format!(" ({zone})"))
            .unwrap_or_default()
    )))
}

/// Live, exact binding for this session (owner/tenant/volume/generation/auth epoch).
fn bound(c: &Connection, b: &MemberGoogleBinding) -> Result<(UserContext, Row)> {
    if b.service != Service::Calendar {
        return Err(StoreError::Forbidden);
    }
    let u = authority(c, &b.user, b.owner)?;
    let r = live(c, &u, b.owner, b.app, Service::Calendar, true)?;
    // Connection id + epoch pin the exact grant generation. credential_version is
    // deliberately not pinned: a legitimate token refresh bumps it.
    if r.id != Some(b.connection_id) || r.epoch != b.epoch {
        return Err(StoreError::Conflict);
    }
    Ok((u, r))
}
fn new_ref() -> Result<String> {
    let mut b = [0u8; 16];
    OsRng
        .try_fill_bytes(&mut b)
        .map_err(|_| StoreError::Unavailable)?;
    Ok(hex(&b))
}

pub(super) fn restart(c: &Connection) -> Result<()> {
    c.execute("UPDATE member_calendar_actions SET state='outcome_unknown',error='restart',version=version+1,updated_at=unixepoch() WHERE state='dispatch_committed'",[])?;
    Ok(())
}
/// Revocation / app replacement / disconnect: purge handles and private content.
pub(super) fn withdraw(c: &Connection, tenant: &str, account: &str) -> Result<()> {
    c.execute(
        "DELETE FROM member_calendar_candidates WHERE tenant_id=?1 AND account_id=?2",
        params![tenant, account],
    )?;
    c.execute("UPDATE member_calendar_actions SET state='withdrawn',confirmed_by=NULL,payload=NULL,event=NULL,provider_etag=NULL,error='source_changed',version=version+1,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND state!='withdrawn'",params![tenant,account])?;
    Ok(())
}

/// Resolve member-supplied civil values in the requested IANA zone. DST gaps and
/// folds are refused rather than guessed.
pub fn member_calendar_window(
    date: &str,
    time: &str,
    zone: &str,
    minutes: u32,
) -> Result<(String, String)> {
    let tz: chrono_tz::Tz = zone.parse().map_err(|_| StoreError::InvalidInput)?;
    let d = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| StoreError::InvalidInput)?;
    let t = NaiveTime::parse_from_str(time, "%H:%M").map_err(|_| StoreError::InvalidInput)?;
    if !(1..=1440).contains(&minutes) {
        return Err(StoreError::InvalidInput);
    }
    let start = match tz.from_local_datetime(&d.and_time(t)) {
        chrono::LocalResult::Single(v) => v,
        _ => return Err(StoreError::InvalidInput),
    };
    let end = start + chrono::Duration::minutes(i64::from(minutes));
    Ok((start.to_rfc3339(), end.to_rfc3339()))
}
/// Candidate search window for a move: the stated civil day, else `now`..+30 days.
pub fn member_calendar_move_window(
    from_date: Option<&str>,
    zone: &str,
    now: i64,
) -> Result<(String, String)> {
    let tz: chrono_tz::Tz = zone.parse().map_err(|_| StoreError::InvalidInput)?;
    if let Some(day) = from_date {
        let d = NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|_| StoreError::InvalidInput)?;
        let next = d.succ_opt().ok_or(StoreError::InvalidInput)?;
        let at = |d: NaiveDate| {
            tz.from_local_datetime(&d.and_time(NaiveTime::MIN))
                .earliest()
                .ok_or(StoreError::InvalidInput)
        };
        return Ok((at(d)?.to_rfc3339(), at(next)?.to_rfc3339()));
    }
    let start = chrono::DateTime::from_timestamp(now, 0).ok_or(StoreError::InvalidInput)?;
    let end = start + chrono::Duration::days(30);
    Ok((start.to_rfc3339(), end.to_rfc3339()))
}
/// Whole-minute duration of an observed timed event (for a move that keeps it).
pub fn member_calendar_minutes(start: &str, end: &str) -> Result<u32> {
    let a = chrono::DateTime::parse_from_rfc3339(start).map_err(|_| StoreError::InvalidInput)?;
    let b = chrono::DateTime::parse_from_rfc3339(end).map_err(|_| StoreError::InvalidInput)?;
    let m = b.signed_duration_since(a).num_minutes();
    u32::try_from(m)
        .ok()
        .filter(|m| (1..=1440).contains(m))
        .ok_or(StoreError::InvalidInput)
}
/// Exact title match after trimming and case folding. Never fuzzy.
pub fn member_calendar_title_matches(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

impl Store {
    /// Store event id + ETag handles from one listing. Titles never persist here.
    pub async fn member_calendar_put_candidates(
        &self,
        b: &MemberGoogleBinding,
        events: Vec<(String, String)>,
    ) -> Result<Vec<String>> {
        if events.len() > 20
            || events.iter().any(|(id, etag)| {
                valid_event_id(id).is_err() || etag.is_empty() || etag.len() > 512
            })
        {
            return Err(StoreError::InvalidInput);
        }
        let probe = clone_probe(b)?;
        let (owner, app, conn, epoch, version) = (
            probe.owner,
            probe.app,
            probe.connection_id,
            probe.epoch,
            probe.credential_version,
        );
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (u, _) = bound(&tx, &probe)?;
            tx.execute("DELETE FROM member_calendar_candidates WHERE tenant_id=?1 AND account_id=?2 AND (expires_at<=unixepoch() OR consumed_by IS NOT NULL)",params![u.tenant.0,u.account.id])?;
            let have: i64 = tx.query_row("SELECT count(*) FROM member_calendar_candidates WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|r.get(0))?;
            if have + events.len() as i64 > MAX_CANDIDATES {
                return Err(StoreError::Busy);
            }
            let mut refs = Vec::new();
            for (id, etag) in &events {
                let handle = new_ref()?;
                tx.execute("INSERT INTO member_calendar_candidates(tenant_id,account_id,candidate_ref,volume_id,generation,auth_epoch,session_digest,app_revision,connection_id,connection_epoch,credential_version,event_id,etag,created_at,expires_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,unixepoch(),unixepoch()+?14)",
                    params![u.tenant.0,u.account.id,handle,owner.volume.to_string(),owner.generation as i64,u.epoch,u.digest.as_slice(),app.to_string(),conn.to_string(),epoch,version,id,etag,CANDIDATE_TTL])?;
                refs.push(handle);
            }
            tx.commit()?;
            Ok(refs)
        }).await
    }

    /// Resolve one handle to its event id and listing-time ETag. Not consuming;
    /// `request_id` lets an exact duplicate retry see its own consumed handle.
    pub async fn member_calendar_candidate(
        &self,
        b: &MemberGoogleBinding,
        candidate_ref: String,
        request_id: Uuid,
    ) -> Result<(String, String)> {
        let probe = clone_probe(b)?;
        self.call(move |db| {
            let (u, _) = bound(&db.conn, &probe)?;
            candidate_in(&db.conn, &u, &probe, &candidate_ref, request_id)
        })
        .await
    }

    /// Commit dispatch intent *before* provider IO. Exact duplicates never re-dispatch.
    pub async fn begin_member_calendar_action(
        &self,
        b: &MemberGoogleBinding,
        intent: MemberCalendarIntent,
        candidate_ref: Option<String>,
    ) -> Result<MemberCalendarAdmission> {
        if !intent.valid() || (intent.kind == Kind::Create) != candidate_ref.is_none() {
            return Err(StoreError::InvalidInput);
        }
        let probe = clone_probe(b)?;
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (u, _) = bound(&tx, &probe)?;
            let digest = intent.digest(probe.connection_id, probe.epoch);
            let id = intent.request_id;
            if let Some(mut s) = load(&tx, &u, id)? {
                if !session_matches(&s, &u, probe.owner)
                    || s.app != probe.app.to_string()
                    || s.connection != probe.connection_id.to_string()
                    || s.connection_epoch != probe.epoch
                    || s.kind != intent.kind
                    || s.event_id != intent.event_id
                    || s.before_etag != intent.before_etag
                    || s.digest != digest
                {
                    return Err(StoreError::Conflict);
                }
                age_dispatch(&tx, &u, id, &mut s)?;
                let r = record(&db.provider_key, &u, id, &s)?;
                tx.commit()?;
                return Ok(MemberCalendarAdmission::Existing(r));
            }
            let count: i64 = tx.query_row("SELECT count(*) FROM member_calendar_actions WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|r.get(0))?;
            if count >= MAX_ACTIONS {
                return Err(StoreError::Busy);
            }
            if let Some(handle) = &candidate_ref {
                let (event_id, etag) = candidate_in(&tx, &u, &probe, handle, id)?;
                if Some(&etag) != intent.before_etag.as_ref() || event_id != intent.event_id {
                    return Err(StoreError::Conflict);
                }
                let n = tx.execute("UPDATE member_calendar_candidates SET consumed_by=?4 WHERE tenant_id=?1 AND account_id=?2 AND candidate_ref=?3 AND (consumed_by IS NULL OR consumed_by=?4)",params![u.tenant.0,u.account.id,handle,id.to_string()])?;
                if n != 1 {
                    return Err(StoreError::Conflict);
                }
            }
            let payload = providers::encrypt(&db.provider_key, &serde_json::to_vec(&intent).map_err(|_| StoreError::InvalidInput)?, &aad(&u, &id.to_string(), "payload"))?;
            tx.execute("INSERT INTO member_calendar_actions(tenant_id,account_id,request_id,volume_id,generation,auth_epoch,session_digest,app_revision,connection_id,connection_epoch,kind,event_id,before_etag,digest,state,payload,version,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'dispatch_committed',?15,1,unixepoch(),unixepoch())",
                params![u.tenant.0,u.account.id,id.to_string(),probe.owner.volume.to_string(),probe.owner.generation as i64,u.epoch,u.digest.as_slice(),probe.app.to_string(),probe.connection_id.to_string(),probe.epoch,intent.kind.as_str(),intent.event_id,intent.before_etag,digest,payload])?;
            tx.commit()?;
            Ok(MemberCalendarAdmission::Fresh(MemberCalendarLease { user: u, owner: probe.owner, app: probe.app, connection_id: probe.connection_id, epoch: probe.epoch, version: 1, intent }))
        }).await
    }

    /// Lease for a read-only reconcile of an `outcome_unknown` request.
    pub async fn claim_member_calendar_reconcile(
        &self,
        b: &MemberGoogleBinding,
        request_id: Uuid,
    ) -> Result<MemberCalendarLease> {
        let probe = clone_probe(b)?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (u, _) = bound(&tx, &probe)?;
            let mut s = load(&tx, &u, request_id)?.ok_or(StoreError::NotFound)?;
            if !session_matches(&s, &u, probe.owner)
                || s.app != probe.app.to_string()
                || s.connection != probe.connection_id.to_string()
                || s.connection_epoch != probe.epoch
            {
                return Err(StoreError::Conflict);
            }
            age_dispatch(&tx, &u, request_id, &mut s)?;
            if s.state != "outcome_unknown" {
                // A young dispatch is still owned by its original lease. The aging
                // update (if any) is kept so a refused claim cannot lose it.
                tx.commit()?;
                return Err(StoreError::Conflict);
            }
            let bytes = providers::decrypt(
                &db.provider_key,
                s.payload.as_deref().ok_or(StoreError::Corrupt)?,
                &aad(&u, &request_id.to_string(), "payload"),
            )?;
            let intent: MemberCalendarIntent =
                serde_json::from_slice(&bytes).map_err(|_| StoreError::Corrupt)?;
            if !intent.valid()
                || intent.request_id != request_id
                || intent.digest(probe.connection_id, probe.epoch) != s.digest
            {
                return Err(StoreError::Corrupt);
            }
            tx.commit()?;
            Ok(MemberCalendarLease {
                user: u,
                owner: probe.owner,
                app: probe.app,
                connection_id: probe.connection_id,
                epoch: probe.epoch,
                version: s.version,
                intent,
            })
        })
        .await
    }

    /// Record the provider outcome. A revoked/changed source withdraws the row and
    /// drops the private content instead of storing a late result.
    pub async fn settle_member_calendar_action(
        &self,
        lease: MemberCalendarLease,
        outcome: MemberCalendarSettle,
    ) -> Result<MemberCalendarRecord> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let id = lease.intent.request_id;
            lease.owner.check_identity(&tx, None)?;
            let s = load(&tx, &lease.user, id)?.ok_or(StoreError::NotFound)?;
            if s.state == "withdrawn" {
                let r = record(&db.provider_key, &lease.user, id, &s)?;
                tx.commit()?;
                return Ok(r);
            }
            let u = match authority(&tx, &lease.user, lease.owner) {
                Ok(u) => u,
                Err(e) => {
                    // Session ended mid-flight: keep the encrypted intent for a later reconcile.
                    tx.execute("UPDATE member_calendar_actions SET state='outcome_unknown',error='session_ended',version=version+1,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3 AND state='dispatch_committed'",params![lease.user.tenant.0,lease.user.account.id,id.to_string()])?;
                    tx.commit()?;
                    return Err(e);
                }
            };
            if !session_matches(&s, &u, lease.owner)
                || s.version != lease.version
                || !matches!(s.state.as_str(), "dispatch_committed" | "outcome_unknown")
                || s.connection != lease.connection_id.to_string()
                || s.connection_epoch != lease.epoch
                || s.app != lease.app.to_string()
            {
                return Err(StoreError::Conflict);
            }
            if !source_live(&tx, &u, lease.owner, &s)? {
                purge_row(&tx, &u, id, "source_changed")?;
                let s = load(&tx, &u, id)?.ok_or(StoreError::Corrupt)?;
                let r = record(&db.provider_key, &u, id, &s)?;
                tx.commit()?;
                return Ok(r);
            }
            let (state, by, etag, receipt, error, event): (&str, Option<&str>, Option<String>, Option<&str>, Option<&str>, Option<Vec<u8>>) = match &outcome {
                MemberCalendarSettle::Applied { event, by } => {
                    if event.id != lease.intent.event_id || !matches!(*by, "provider_response" | "readback") {
                        return Err(StoreError::InvalidInput);
                    }
                    let blob = providers::encrypt(&db.provider_key, &serde_json::to_vec(&**event).map_err(|_| StoreError::InvalidInput)?, &aad(&u, &id.to_string(), "event"))?;
                    ("applied", Some(*by), Some(event.etag.clone()), Some(if lease.intent.kind == Kind::Create { "created" } else { "moved" }), None, Some(blob))
                }
                MemberCalendarSettle::Conflict => ("conflict", None, None, None, Some("stale_etag"), None),
                MemberCalendarSettle::Failed(code) => ("failed_definite", None, None, None, Some(*code), None),
                MemberCalendarSettle::Unknown(code) => ("outcome_unknown", None, None, None, Some(*code), None),
            };
            // Definite outcomes no longer need the private intent.
            let keep_payload = state == "outcome_unknown";
            let n = tx.execute("UPDATE member_calendar_actions SET state=?4,confirmed_by=?5,provider_etag=?6,receipt=?7,error=?8,event=?9,payload=CASE WHEN ?10 THEN payload ELSE NULL END,version=version+1,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3 AND version=?11",
                params![u.tenant.0,u.account.id,id.to_string(),state,by,etag,receipt,error,event,keep_payload,lease.version])?;
            if n != 1 {
                return Err(StoreError::Conflict);
            }
            let s = load(&tx, &u, id)?.ok_or(StoreError::Corrupt)?;
            let r = record(&db.provider_key, &u, id, &s)?;
            tx.commit()?;
            Ok(r)
        }).await
    }

    /// The caller's own request. A changed/revoked source withdraws it on read.
    pub async fn member_calendar_record(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        request_id: Uuid,
    ) -> Result<Option<MemberCalendarRecord>> {
        let u = u.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = authority(&tx, &u, o)?;
            let Some(mut s) = load(&tx, &u, request_id)? else {
                return Ok(None);
            };
            if !session_matches(&s, &u, o) {
                return Err(StoreError::Conflict);
            }
            if s.state != "withdrawn" && !source_live(&tx, &u, o, &s)? {
                purge_row(&tx, &u, request_id, "source_changed")?;
                s = load(&tx, &u, request_id)?.ok_or(StoreError::Corrupt)?;
            }
            age_dispatch(&tx, &u, request_id, &mut s)?;
            let r = record(&db.provider_key, &u, request_id, &s)?;
            tx.commit()?;
            Ok(Some(r))
        })
        .await
    }
}

fn clone_probe(b: &MemberGoogleBinding) -> Result<MemberGoogleBinding> {
    Ok(MemberGoogleBinding {
        service: b.service,
        access_token: SecretString::new("x".into()).map_err(|_| StoreError::Corrupt)?,
        access_binding: String::new(),
        mailbox: b.mailbox.clone(),
        connection_id: b.connection_id,
        epoch: b.epoch,
        credential_version: b.credential_version,
        expires_at: b.expires_at,
        needs_refresh: false,
        user: b.user.clone(),
        owner: b.owner,
        app: b.app,
        app_version: b.app_version,
    })
}
/// The access token is deliberately not cloned into the closure: only identity is.
fn candidate_in(
    c: &Connection,
    u: &UserContext,
    b: &MemberGoogleBinding,
    handle: &str,
    request_id: Uuid,
) -> Result<(String, String)> {
    if handle.len() != 32 {
        return Err(StoreError::InvalidInput);
    }
    c.query_row("SELECT event_id,etag FROM member_calendar_candidates WHERE tenant_id=?1 AND account_id=?2 AND candidate_ref=?3 AND volume_id=?4 AND generation=?5 AND auth_epoch=?6 AND session_digest=?7 AND app_revision=?8 AND connection_id=?9 AND connection_epoch=?10 AND expires_at>unixepoch() AND (consumed_by IS NULL OR consumed_by=?11)",
        params![u.tenant.0,u.account.id,handle,b.owner.volume.to_string(),b.owner.generation as i64,u.epoch,u.digest.as_slice(),b.app.to_string(),b.connection_id.to_string(),b.epoch,request_id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?))).optional()?.ok_or(StoreError::Conflict)
}

#[cfg(test)]
mod tests;
