//! Member Calendar route handlers: one primary timed event create or move.
//! Authenticated member session only (`Google::user`). Reuses the member Google
//! grant for service `calendar` (same encrypted token location, owner/volume/
//! generation/auth-epoch/app pin, refresh and revocation). Provider IO goes
//! through the typed `CalendarProvider` seam; nothing is enabled until main passes
//! `live_provider()` to `routes`. No model call is made here: the browser sends
//! structured civil values, the member approves the exact final content
//! (`prepare` → local preview, `apply` → one provider write + readback).
use super::*;
use async_trait::async_trait;
use moosuri_connectors::model::CancellationToken;
use moosuri_connectors::{
    calendar::member::{MemberCalendar, MemberWriteError, Readback, stable_event_id},
    gmail::GmailError,
};
use moosuri_core::member_calendar::*;
use moosuri_core::{CalendarEvent, CalendarEvents, CalendarFields};
use moosuri_store::{
    MemberCalendarAdmission, MemberCalendarIntent, MemberCalendarRecord, MemberCalendarSettle,
    MemberGoogleBinding, member_calendar_minutes, member_calendar_move_window,
    member_calendar_title_matches, member_calendar_window,
};

pub(super) const STATUS: &str = "/api/v1/personal/google/calendar/status";
pub(super) const PREPARE: &str = "/api/v1/personal/google/calendar/prepare";
pub(super) const APPLY: &str = "/api/v1/personal/google/calendar/apply";

/// Typed provider operations. The only implementation that reaches Google is
/// `GoogleProvider`; tests supply a deterministic fixture.
#[async_trait]
pub(super) trait CalendarProvider: Send + Sync {
    async fn list(
        &self,
        t: &SecretString,
        primary: &str,
        start: &str,
        end: &str,
    ) -> Result<CalendarEvents, GmailError>;
    async fn read(
        &self,
        t: &SecretString,
        primary: &str,
        id: &str,
    ) -> Result<CalendarEvent, GmailError>;
    #[allow(clippy::too_many_arguments)]
    async fn create(
        &self,
        t: &SecretString,
        primary: &str,
        id: &str,
        f: &CalendarFields,
        op: &str,
        digest: &str,
    ) -> Result<CalendarEvent, MemberWriteError>;
    #[allow(clippy::too_many_arguments)]
    async fn move_event(
        &self,
        t: &SecretString,
        primary: &str,
        id: &str,
        etag: &str,
        f: &CalendarFields,
        op: &str,
        digest: &str,
    ) -> Result<CalendarEvent, MemberWriteError>;
    async fn readback(
        &self,
        t: &SecretString,
        primary: &str,
        id: &str,
        op: &str,
        digest: &str,
        f: &CalendarFields,
    ) -> Result<Readback, GmailError>;
}
/// Real Google Calendar API (fixed origin, existing connector transport).
pub(super) struct GoogleProvider(MemberCalendar);
/// Live dispatch exists only for the local backend, and only through that member's
/// existing loopback Google proxy (fixed `www.googleapis.com:443` CONNECT). Broker,
/// fixture or an unusable proxy get the denying provider; there is no direct egress.
pub(super) fn provider_for(backend: &Backend) -> Arc<dyn CalendarProvider> {
    match backend {
        Backend::Local { proxy, .. } => match MemberCalendar::google_proxy(proxy.address()) {
            Ok(client) => Arc::new(GoogleProvider(client)),
            Err(_) => disabled_provider(),
        },
        _ => disabled_provider(),
    }
}
/// Default for any build/test that has not explicitly enabled Google: every call is denied.
pub(super) struct DisabledProvider;
pub(super) fn disabled_provider() -> Arc<dyn CalendarProvider> {
    Arc::new(DisabledProvider)
}
#[async_trait]
impl CalendarProvider for GoogleProvider {
    async fn list(
        &self,
        t: &SecretString,
        p: &str,
        s: &str,
        e: &str,
    ) -> Result<CalendarEvents, GmailError> {
        self.0.list(t, p, s, e, &CancellationToken::new()).await
    }
    async fn read(&self, t: &SecretString, p: &str, id: &str) -> Result<CalendarEvent, GmailError> {
        self.0.read(t, p, id, &CancellationToken::new()).await
    }
    async fn create(
        &self,
        t: &SecretString,
        p: &str,
        id: &str,
        f: &CalendarFields,
        op: &str,
        d: &str,
    ) -> Result<CalendarEvent, MemberWriteError> {
        self.0
            .create(t, p, id, f, op, d, &CancellationToken::new())
            .await
    }
    async fn move_event(
        &self,
        t: &SecretString,
        p: &str,
        id: &str,
        etag: &str,
        f: &CalendarFields,
        op: &str,
        d: &str,
    ) -> Result<CalendarEvent, MemberWriteError> {
        self.0
            .move_event(t, p, id, etag, f, op, d, &CancellationToken::new())
            .await
    }
    async fn readback(
        &self,
        t: &SecretString,
        p: &str,
        id: &str,
        op: &str,
        d: &str,
        f: &CalendarFields,
    ) -> Result<Readback, GmailError> {
        self.0
            .readback(t, p, id, op, d, f, &CancellationToken::new())
            .await
    }
}
#[async_trait]
impl CalendarProvider for DisabledProvider {
    async fn list(
        &self,
        _: &SecretString,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<CalendarEvents, GmailError> {
        Err(GmailError::DestinationDenied)
    }
    async fn read(&self, _: &SecretString, _: &str, _: &str) -> Result<CalendarEvent, GmailError> {
        Err(GmailError::DestinationDenied)
    }
    async fn create(
        &self,
        _: &SecretString,
        _: &str,
        _: &str,
        _: &CalendarFields,
        _: &str,
        _: &str,
    ) -> Result<CalendarEvent, MemberWriteError> {
        Err(MemberWriteError::BeforeWrite(GmailError::DestinationDenied))
    }
    async fn move_event(
        &self,
        _: &SecretString,
        _: &str,
        _: &str,
        _: &str,
        _: &CalendarFields,
        _: &str,
        _: &str,
    ) -> Result<CalendarEvent, MemberWriteError> {
        Err(MemberWriteError::BeforeWrite(GmailError::DestinationDenied))
    }
    async fn readback(
        &self,
        _: &SecretString,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &CalendarFields,
    ) -> Result<Readback, GmailError> {
        Err(GmailError::DestinationDenied)
    }
}

#[derive(Clone)]
struct Calendar {
    g: Google,
    provider: Arc<dyn CalendarProvider>,
    now: fn() -> i64,
}
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
/// `g` is the member's shared `Google` state (same store/owner/backend/serial lock).
pub(super) fn routes(g: Google, provider: Arc<dyn CalendarProvider>) -> Router {
    router(g, provider, unix_now)
}
fn router(g: Google, provider: Arc<dyn CalendarProvider>, now: fn() -> i64) -> Router {
    Router::new()
        .route(STATUS, get(status))
        .route(PREPARE, post(prepare))
        .route(APPLY, post(apply))
        .with_state(Calendar {
            g: Google {
                service: Service::Calendar,
                ..g
            },
            provider,
            now,
        })
}

/// Gateway allowlist for these three paths: reconstruct each public DTO.
/// `None` means the path is not a Member Calendar path.
pub fn public_response(
    path: &str,
    method: &Method,
    status: StatusCode,
    bytes: &[u8],
) -> Option<Result<Vec<u8>, StatusCode>> {
    let bad = Err(StatusCode::BAD_GATEWAY);
    let expected = match path {
        STATUS => Method::GET,
        PREPARE | APPLY => Method::POST,
        _ => return None,
    };
    if *method != expected || status != StatusCode::OK || bytes.len() > 64 * 1024 {
        return Some(bad);
    }
    let out = match path {
        STATUS => decode::<Status>(bytes)
            .ok()
            .filter(|v| v.connection.as_ref().is_none_or(Connection::valid))
            .and_then(|v| encode(&v).ok()),
        PREPARE => decode::<MemberCalendarPrepared>(bytes)
            .ok()
            .filter(prepared_valid)
            .and_then(|v| encode(&v).ok()),
        _ => decode::<MemberCalendarReceipt>(bytes)
            .ok()
            .and_then(|v| encode(&v).ok()),
    };
    Some(out.ok_or(StatusCode::BAD_GATEWAY))
}
fn prepared_valid(v: &MemberCalendarPrepared) -> bool {
    match v {
        MemberCalendarPrepared::NeedsSelection { candidates, .. } => candidates.len() <= 20,
        MemberCalendarPrepared::Preview { preview } => hex64(&preview.digest),
        _ => true,
    }
}

/// Early exit that is a normal answer (needs connection) vs an HTTP failure.
enum Early {
    Needs(String),
    Fail(Failure),
}
impl From<Failure> for Early {
    fn from(f: Failure) -> Self {
        Early::Fail(f)
    }
}
impl From<StoreError> for Early {
    fn from(e: StoreError) -> Self {
        Early::Fail(e.into())
    }
}
fn unavailable() -> Failure {
    Failure(StatusCode::BAD_GATEWAY, "google_unavailable")
}
/// Provider read/list failure: a refused token is "needs connection", not an outage.
fn provider_error(e: GmailError) -> Early {
    match e {
        GmailError::Http(401 | 403) => Early::Needs("calendar_permission_denied".into()),
        _ => Early::Fail(unavailable()),
    }
}
fn json_ok<T: Serialize>(path: &str, method: Method, value: &T) -> Result<Response, Failure> {
    let bytes = encode(value).map_err(|s| Failure(s, "unavailable"))?;
    let bytes = public_response(path, &method, StatusCode::OK, &bytes)
        .ok_or(Failure(StatusCode::SERVICE_UNAVAILABLE, "unavailable"))?
        .map_err(|s| Failure(s, "unavailable"))?;
    Ok(response(StatusCode::OK, bytes))
}

struct Ctx {
    binding: MemberGoogleBinding,
    primary: String,
}
/// Exact source: the browser names the connection id + epoch it was shown.
async fn enter(
    c: &Calendar,
    user: &UserContext,
    connection_id: &str,
    epoch: i64,
) -> Result<Ctx, Early> {
    let g = &c.g;
    let app = g.describe().await?;
    let current = g
        .store
        .member_service_status(user, g.owner, Service::Calendar, app.app_revision)
        .await?;
    if !current.connected {
        // absent | disconnected | needs_reauth | app_changed: all need (re)consent.
        return Err(Early::Needs(match current.status.as_str() {
            "absent" => "not_connected".into(),
            other => other.into(),
        }));
    }
    let refresh = current.needs_refresh;
    let chosen = Selected {
        connection_id: Uuid::parse_str(connection_id).map_err(|_| StoreError::InvalidInput)?,
        epoch,
    };
    selected(&chosen, connection(current)?.as_ref())?;
    let binding = read_binding(g, user, app.app_revision, &chosen, refresh).await?;
    // Every network dispatch (list/read/write/readback) starts here: the broker/local
    // adapter must verify the signed access binding for this exact owner, app,
    // purpose `calendar:access`, token and expiry before the token leaves the process.
    g.verify_access(app.app_revision, &binding).await?;
    g.store.check_member_google_read(&binding).await?;
    // A refused/narrow grant never reaches here: consent requests only the writable
    // owned-events scope and the store rejects any other granted scope set.
    let primary = binding.mailbox.to_ascii_lowercase();
    Ok(Ctx { binding, primary })
}

enum Target {
    Needs(MemberCalendarPrepared),
    Ready {
        intent: MemberCalendarIntent,
        current: Option<CalendarEvent>,
        handle: Option<String>,
    },
}
fn fields(
    title: &str,
    start: String,
    end: String,
    zone: &str,
    description: &str,
    location: &str,
) -> CalendarFields {
    CalendarFields {
        title: title.into(),
        description: description.into(),
        location: location.into(),
        start,
        end,
        time_zone: Some(zone.into()),
    }
}
fn view(c: &CalendarEvent, handle: &str) -> MemberCalendarCandidate {
    MemberCalendarCandidate {
        candidate_ref: handle.into(),
        title: c.fields.title.clone(),
        start: c.fields.start.clone(),
        end: c.fields.end.clone(),
        time_zone: c.fields.time_zone.clone(),
    }
}
/// Build the exact intent for this request. Move resolves current candidates; a
/// missing explicit selection among several is returned, never guessed.
async fn target(
    c: &Calendar,
    ctx: &Ctx,
    req: &MemberCalendarRequest,
    request_id: Uuid,
) -> Result<Target, Early> {
    let t = &ctx.binding.access_token;
    if req.kind == MemberCalendarKind::Create {
        let (start, end) = member_calendar_window(
            &req.date,
            &req.time,
            &req.time_zone,
            req.duration_minutes.unwrap_or(0),
        )?;
        return Ok(Target::Ready {
            intent: MemberCalendarIntent {
                request_id,
                kind: MemberCalendarKind::Create,
                event_id: stable_event_id(&request_id.to_string()),
                before_etag: None,
                fields: fields(req.title.trim(), start, end, &req.time_zone, "", ""),
            },
            current: None,
            handle: None,
        });
    }
    let handle = if let Some(h) = &req.candidate_ref {
        h.clone()
    } else {
        let (from, to) =
            member_calendar_move_window(req.from_date.as_deref(), &req.time_zone, (c.now)())?;
        let page = c
            .provider
            .list(t, &ctx.primary, &from, &to)
            .await
            .map_err(provider_error)?;
        let mut hits: Vec<&CalendarEvent> = page
            .events
            .iter()
            .filter(|e| {
                e.writable
                    && !e.all_day
                    && member_calendar_title_matches(&e.fields.title, &req.title)
            })
            .collect();
        if hits.is_empty() {
            return Ok(Target::Needs(MemberCalendarPrepared::NotFound {
                partial: page.partial,
            }));
        }
        let truncated = hits.len() > 20;
        hits.truncate(20);
        let refs =
            c.g.store
                .member_calendar_put_candidates(
                    &ctx.binding,
                    hits.iter()
                        .map(|e| (e.id.clone(), e.etag.clone()))
                        .collect(),
                )
                .await?;
        if hits.len() > 1 || page.partial || truncated {
            let candidates = hits.iter().zip(&refs).map(|(e, r)| view(e, r)).collect();
            return Ok(Target::Needs(MemberCalendarPrepared::NeedsSelection {
                candidates,
                partial: page.partial || truncated,
            }));
        }
        refs[0].clone()
    };
    let stale = || Ok(Target::Needs(MemberCalendarPrepared::Stale));
    let (event_id, etag) = match c
        .g
        .store
        .member_calendar_candidate(&ctx.binding, handle.clone(), request_id)
        .await
    {
        Ok(v) => v,
        Err(StoreError::Conflict) => return stale(),
        Err(e) => return Err(e.into()),
    };
    let current = match c.provider.read(t, &ctx.primary, &event_id).await {
        Ok(v) => v,
        Err(GmailError::Http(404 | 410)) => return stale(),
        Err(e) => return Err(provider_error(e)),
    };
    if current.etag != etag || !current.writable || current.all_day {
        return stale();
    }
    let minutes = match req.duration_minutes {
        Some(m) => m,
        None => member_calendar_minutes(&current.fields.start, &current.fields.end)?,
    };
    let (start, end) = member_calendar_window(&req.date, &req.time, &req.time_zone, minutes)?;
    // A move changes only the time: title, description and location are kept.
    let f = fields(
        &current.fields.title,
        start,
        end,
        &req.time_zone,
        &current.fields.description,
        &current.fields.location,
    );
    Ok(Target::Ready {
        intent: MemberCalendarIntent {
            request_id,
            kind: MemberCalendarKind::Move,
            event_id,
            before_etag: Some(etag),
            fields: f,
        },
        current: Some(current),
        handle: Some(handle),
    })
}

async fn status(State(c): State<Calendar>, headers: HeaderMap) -> Result<Response, Failure> {
    let g = &c.g;
    let user = g.user(&headers).await?;
    let status = if let Backend::Local { config, .. } = &g.backend {
        let (revision, _, _) = registered_client(config).map_err(configuration_failure)?;
        g.store
            .member_service_status(&user, g.owner, Service::Calendar, revision)
            .await?
    } else {
        g.store
            .member_service_status_local(&user, g.owner, Service::Calendar)
            .await?
    };
    json_ok(
        STATUS,
        Method::GET,
        &Status {
            connection: connection(status)?,
        },
    )
}

async fn prepare(
    State(c): State<Calendar>,
    headers: HeaderMap,
    Json(req): Json<MemberCalendarRequest>,
) -> Result<Response, Failure> {
    if !req.well_formed() {
        return Err(StoreError::InvalidInput.into());
    }
    let user = c.g.user(&headers).await?;
    let _serial =
        c.g.serial
            .try_lock()
            .map_err(|_| Failure(StatusCode::CONFLICT, "conflict"))?;
    if let Some(thread) = &req.thread_id {
        let thread = Uuid::parse_str(thread).map_err(|_| StoreError::InvalidInput)?;
        let id = Uuid::parse_str(&req.request_id).map_err(|_| StoreError::InvalidInput)?;
        c.g.store
            .link_member_calendar(
                c.g.owner,
                &user,
                thread,
                id,
                format!(
                    "일정 {} · {}",
                    if req.kind == MemberCalendarKind::Create {
                        "생성"
                    } else {
                        "이동"
                    },
                    req.title
                ),
            )
            .await?;
    }
    let out = match prepare_inner(&c, &user, &req).await {
        Ok(v) => v,
        Err(Early::Needs(reason)) => MemberCalendarPrepared::NeedsConnection { reason },
        Err(Early::Fail(f)) => return Err(f),
    };
    json_ok(PREPARE, Method::POST, &out)
}
async fn prepare_inner(
    c: &Calendar,
    user: &UserContext,
    req: &MemberCalendarRequest,
) -> Result<MemberCalendarPrepared, Early> {
    let ctx = enter(c, user, &req.connection_id, req.epoch).await?;
    let request_id = Uuid::parse_str(&req.request_id).map_err(|_| StoreError::InvalidInput)?;
    match target(c, &ctx, req, request_id).await? {
        Target::Needs(v) => Ok(v),
        Target::Ready {
            intent,
            current,
            handle,
        } => Ok(MemberCalendarPrepared::Preview {
            preview: MemberCalendarPreview {
                kind: req.kind,
                digest: intent.digest(ctx.binding.connection_id, ctx.binding.epoch),
                title: intent.fields.title.clone(),
                start: intent.fields.start.clone(),
                end: intent.fields.end.clone(),
                time_zone: req.time_zone.clone(),
                current: current.zip(handle).map(|(e, h)| view(&e, &h)),
            },
        }),
    }
}

async fn apply(
    State(c): State<Calendar>,
    headers: HeaderMap,
    Json(input): Json<MemberCalendarApply>,
) -> Result<Response, Failure> {
    let req = &input.request;
    if !req.well_formed()
        || !hex64(&input.approved_digest)
        || (req.kind == MemberCalendarKind::Move && req.candidate_ref.is_none())
    {
        return Err(StoreError::InvalidInput.into());
    }
    let user = c.g.user(&headers).await?;
    let _serial =
        c.g.serial
            .try_lock()
            .map_err(|_| Failure(StatusCode::CONFLICT, "conflict"))?;
    let out = match apply_inner(&c, &user, &input).await {
        Ok(v) => v,
        Err(Early::Needs(reason)) => MemberCalendarReceipt::NeedsConnection { reason },
        Err(Early::Fail(f)) => return Err(f),
    };
    json_ok(APPLY, Method::POST, &out)
}
async fn apply_inner(
    c: &Calendar,
    user: &UserContext,
    input: &MemberCalendarApply,
) -> Result<MemberCalendarReceipt, Early> {
    let req = &input.request;
    let ctx = enter(c, user, &req.connection_id, req.epoch).await?;
    let request_id = Uuid::parse_str(&req.request_id).map_err(|_| StoreError::InvalidInput)?;
    // Duplicate recovery first: an exact retry never re-targets or re-sends.
    if let Some(rec) =
        c.g.store
            .member_calendar_record(user, c.g.owner, request_id)
            .await?
    {
        if rec.digest != input.approved_digest {
            return Err(Failure(StatusCode::CONFLICT, "approval_mismatch").into());
        }
        return existing(c, &ctx, rec).await;
    }
    let (intent, handle) = match target(c, &ctx, req, request_id).await? {
        Target::Ready { intent, handle, .. } => (intent, handle),
        // The approved version is gone: definite, nothing was written.
        Target::Needs(MemberCalendarPrepared::Stale) => return Ok(MemberCalendarReceipt::Conflict),
        Target::Needs(_) => return Err(Failure(StatusCode::CONFLICT, "conflict").into()),
    };
    if intent.digest(ctx.binding.connection_id, ctx.binding.epoch) != input.approved_digest {
        return Err(Failure(StatusCode::CONFLICT, "approval_mismatch").into());
    }
    match c
        .g
        .store
        .begin_member_calendar_action(&ctx.binding, intent, handle)
        .await?
    {
        MemberCalendarAdmission::Existing(rec) => existing(c, &ctx, rec).await,
        MemberCalendarAdmission::Fresh(lease) => dispatch(c, &ctx, lease).await,
    }
}
async fn existing(
    c: &Calendar,
    ctx: &Ctx,
    rec: MemberCalendarRecord,
) -> Result<MemberCalendarReceipt, Early> {
    if rec.state == "outcome_unknown" {
        return reconcile(c, ctx, rec.request_id).await;
    }
    Ok(receipt(&rec))
}
/// Exactly one provider write, after the dispatch row is committed.
async fn dispatch(
    c: &Calendar,
    ctx: &Ctx,
    lease: moosuri_store::MemberCalendarLease,
) -> Result<MemberCalendarReceipt, Early> {
    let (t, p) = (&ctx.binding.access_token, ctx.primary.as_str());
    let i = lease.intent.clone();
    let (op, digest) = (
        i.marker(),
        i.digest(ctx.binding.connection_id, ctx.binding.epoch),
    );
    let sent = match i.kind {
        MemberCalendarKind::Create => {
            c.provider
                .create(t, p, &i.event_id, &i.fields, &op, &digest)
                .await
        }
        MemberCalendarKind::Move => {
            c.provider
                .move_event(
                    t,
                    p,
                    &i.event_id,
                    i.before_etag.as_deref().unwrap_or_default(),
                    &i.fields,
                    &op,
                    &digest,
                )
                .await
        }
    };
    let settle = match sent {
        Ok(event) => MemberCalendarSettle::Applied {
            event: Box::new(event),
            by: "provider_response",
        },
        Err(MemberWriteError::Stale(_)) => MemberCalendarSettle::Conflict,
        Err(MemberWriteError::NotWritable) => MemberCalendarSettle::Failed("not_writable"),
        Err(MemberWriteError::BeforeWrite(e)) => MemberCalendarSettle::Failed(match e {
            GmailError::Http(401 | 403) => "permission_denied",
            GmailError::Http(404 | 410) => "event_missing",
            _ => "provider_unavailable",
        }),
        Err(MemberWriteError::Write(e)) => match e {
            GmailError::Http(401 | 403) => MemberCalendarSettle::Failed("permission_denied"),
            GmailError::Http(400) => MemberCalendarSettle::Failed("provider_rejected"),
            GmailError::Http(404 | 410) => MemberCalendarSettle::Failed("event_missing"),
            GmailError::Http(409) => MemberCalendarSettle::Unknown("id_exists"),
            // Timeout/transport/5xx/malformed ack: the write may have landed.
            _ => MemberCalendarSettle::Unknown("ack_lost"),
        },
    };
    let unknown = matches!(settle, MemberCalendarSettle::Unknown(_));
    let rec =
        c.g.store
            .settle_member_calendar_action(lease, settle)
            .await?;
    if unknown && rec.state == "outcome_unknown" {
        // Lost acknowledgement: one read-only reconciliation, never a second write.
        return reconcile(c, ctx, rec.request_id).await;
    }
    Ok(receipt(&rec))
}
async fn reconcile(
    c: &Calendar,
    ctx: &Ctx,
    request_id: Uuid,
) -> Result<MemberCalendarReceipt, Early> {
    let lease =
        c.g.store
            .claim_member_calendar_reconcile(&ctx.binding, request_id)
            .await?;
    let i = lease.intent.clone();
    let (op, digest) = (
        i.marker(),
        i.digest(ctx.binding.connection_id, ctx.binding.epoch),
    );
    let settle = match c
        .provider
        .readback(
            &ctx.binding.access_token,
            &ctx.primary,
            &i.event_id,
            &op,
            &digest,
            &i.fields,
        )
        .await
    {
        Ok(Readback::Applied(event)) => MemberCalendarSettle::Applied {
            event,
            by: "readback",
        },
        Ok(Readback::Different(_)) => MemberCalendarSettle::Unknown("not_applied_on_readback"),
        Err(GmailError::Http(404 | 410)) => MemberCalendarSettle::Unknown("absent_on_readback"),
        Err(_) => MemberCalendarSettle::Unknown("readback_failed"),
    };
    let rec =
        c.g.store
            .settle_member_calendar_action(lease, settle)
            .await?;
    Ok(receipt(&rec))
}
fn receipt(r: &MemberCalendarRecord) -> MemberCalendarReceipt {
    let code = || r.error.clone().unwrap_or_else(|| "unknown".into());
    match r.state.as_str() {
        "applied" => match (&r.event, &r.confirmed_by, &r.receipt) {
            (Some(e), Some(by), Some(kind)) => MemberCalendarReceipt::Confirmed {
                confirmed_by: by.clone(),
                receipt: kind.clone(),
                event: MemberCalendarEventView {
                    id: e.id.clone(),
                    title: e.fields.title.clone(),
                    start: e.fields.start.clone(),
                    end: e.fields.end.clone(),
                    time_zone: e.fields.time_zone.clone(),
                    html_link: e.html_link.clone(),
                },
            },
            _ => MemberCalendarReceipt::Unknown {
                code: "receipt_unavailable".into(),
            },
        },
        "conflict" => MemberCalendarReceipt::Conflict,
        "failed_definite" if r.error.as_deref() == Some("permission_denied") => {
            MemberCalendarReceipt::NeedsConnection {
                reason: "calendar_permission_denied".into(),
            }
        }
        "failed_definite" => MemberCalendarReceipt::Failed { code: code() },
        "outcome_unknown" => MemberCalendarReceipt::Unknown { code: code() },
        "dispatch_committed" => MemberCalendarReceipt::InProgress,
        _ => MemberCalendarReceipt::Withdrawn,
    }
}

#[cfg(test)]
mod tests;
