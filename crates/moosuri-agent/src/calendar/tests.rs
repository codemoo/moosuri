//! Real Store with private counted typed IO; connector wire contracts are tested
//! in moosuri-connectors separately. No network or provider credential is used.
use super::*;
use crate::Signals;
use moosuri_connectors::gmail::{OAuthClient, TokenSet};
use moosuri_store::GoogleAppPin;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
};
use tokio::sync::{Notify, Semaphore, watch};

const PASSWORD: &str = "Synthetic-calendar-service-password";
const REDIRECT: &str = "https://assistant.example.test/api/v1/mail/oauth/callback";

#[derive(Clone)]
struct WriteRecord {
    id: String,
    event_id: String,
    digest: String,
    mutation: CalendarMutation,
}
pub(super) struct FixtureObservation {
    pub(super) event: CalendarEvent,
    operation: Option<String>,
    digest: Option<String>,
}
impl FixtureObservation {
    pub(super) fn matches(
        &self,
        event: &str,
        operation: &str,
        digest: &str,
        mutation: &CalendarMutation,
    ) -> bool {
        self.event.id == event
            && self.operation.as_deref() == Some(operation)
            && self.digest.as_deref() == Some(digest)
            && moosuri_connectors::calendar::matches_fields(&self.event, mutation)
    }
}
pub(super) struct FixtureIo {
    primary_calls: AtomicUsize,
    event_calls: Mutex<Vec<String>>,
    writes: Mutex<Vec<WriteRecord>>,
    observed: Mutex<Option<CalendarEvent>>,
    //0 success,1 timeout (including applied-but-ACK-lost),2 definite412.
    write_mode: AtomicU8,
    //0 matching current event,1 absent,2 wrong operation marker,3 peer edit.
    lookup_mode: AtomicU8,
    hold_primary: AtomicBool,
    hold_events: AtomicBool,
    hold_mutation: AtomicBool,
    entered: Semaphore,
    release: Semaphore,
}
impl FixtureIo {
    fn new() -> Self {
        Self {
            primary_calls: AtomicUsize::new(0),
            event_calls: Mutex::new(vec![]),
            writes: Mutex::new(vec![]),
            observed: Mutex::new(None),
            write_mode: AtomicU8::new(0),
            lookup_mode: AtomicU8::new(0),
            hold_primary: AtomicBool::new(false),
            hold_events: AtomicBool::new(false),
            hold_mutation: AtomicBool::new(false),
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }
    async fn hold(&self, enabled: bool) {
        if enabled {
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
        }
    }
    async fn reached(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
    fn write_count(&self) -> usize {
        self.writes.lock().unwrap().len()
    }
    pub(super) async fn primary(&self) -> Result<CalendarInfo, GmailError> {
        self.primary_calls.fetch_add(1, Ordering::SeqCst);
        self.hold(self.hold_primary.load(Ordering::SeqCst)).await;
        Ok(primary())
    }
    pub(super) fn calendars(&self, page: Option<&str>) -> Result<CalendarList, GmailError> {
        assert_eq!(page, Some("calendar-page"));
        Ok(CalendarList {
            items: vec![primary()],
            next_page_token: Some("next-calendar-page".into()),
            partial: true,
        })
    }
    pub(super) async fn events(
        &self,
        primary_id: &str,
        range: &CalendarRange,
    ) -> Result<CalendarEvents, GmailError> {
        assert_eq!(primary_id, primary().id);
        if range.page_token.is_none() {
            let mut f = fields();
            f.title = "Synthetic meeting".into();
            f.start = range.start.clone();
            f.end = range.start.clone();
            let hour = range.start[11..13].parse::<u8>().unwrap() + 1;
            assert!(hour < 24, "bounded synthetic fixture range");
            f.end.replace_range(11..13, &format!("{hour:02}"));
            let event = event("synthetic1", f);
            *self.observed.lock().unwrap() = Some(event.clone());
            return Ok(CalendarEvents {
                events: vec![event],
                next_page_token: None,
                partial: false,
                primary_id: primary_id.into(),
            });
        }
        assert_eq!(range.start, fields().start);
        assert_eq!(range.end, fields().end);
        assert_eq!(range.page_token.as_deref(), Some("event-page"));
        self.hold(self.hold_events.load(Ordering::SeqCst)).await;
        Ok(CalendarEvents {
            events: vec![event("synthetic1", fields())],
            next_page_token: Some("next-event-page".into()),
            partial: true,
            primary_id: primary_id.into(),
        })
    }
    pub(super) fn event(&self, primary_id: &str, id: &str) -> Result<Observation, GmailError> {
        assert_eq!(primary_id, primary().id);
        self.event_calls.lock().unwrap().push(id.into());
        if self.lookup_mode.load(Ordering::SeqCst) == 1 {
            return Err(GmailError::Http(404));
        }
        let record = self.writes.lock().unwrap().last().cloned();
        let f = record
            .as_ref()
            .and_then(|r| match &r.mutation {
                CalendarMutation::Create { fields } | CalendarMutation::Update { fields, .. } => {
                    Some(fields.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| {
                self.observed
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|v| v.fields.clone())
                    .unwrap_or_else(fields)
            });
        let mut e = event(id, f);
        if self.lookup_mode.load(Ordering::SeqCst) == 3 {
            e.fields.title = "Peer changed title".into()
        }
        Ok(Observation::Fixture(Box::new(FixtureObservation {
            event: e,
            operation: record.as_ref().map(|r| {
                if self.lookup_mode.load(Ordering::SeqCst) == 2 {
                    "wrong-operation".into()
                } else {
                    r.id.clone()
                }
            }),
            digest: record.map(|r| r.digest),
        })))
    }
    pub(super) async fn mutate(
        &self,
        d: &CalendarDispatch,
        prior: Option<&Observation>,
    ) -> Result<MutationResult, GmailError> {
        assert_eq!(d.primary_id, primary().id);
        match &d.change.mutation {
            CalendarMutation::Create { .. } => assert!(prior.is_none()),
            CalendarMutation::Update { event_id, etag, .. }
            | CalendarMutation::Delete { event_id, etag } => {
                let p = prior.unwrap().event();
                assert_eq!(&p.id, event_id);
                assert_eq!(&p.etag, etag);
            }
        }
        self.writes.lock().unwrap().push(WriteRecord {
            id: d.change.request_id.clone(),
            event_id: d.event_id.clone(),
            digest: d.digest.clone(),
            mutation: d.change.mutation.clone(),
        });
        self.hold(self.hold_mutation.load(Ordering::SeqCst)).await;
        match self.write_mode.load(Ordering::SeqCst) {
            1 => Err(GmailError::Timeout),
            2 => Err(GmailError::Http(412)),
            _ => match &d.change.mutation {
                CalendarMutation::Create { fields } | CalendarMutation::Update { fields, .. } => {
                    Ok(MutationResult {
                        status: if prior.is_some() { 200 } else { 201 },
                        event: Some(event(&d.event_id, fields.clone())),
                    })
                }
                CalendarMutation::Delete { .. } => Ok(MutationResult {
                    status: 204,
                    event: None,
                }),
            },
        }
    }
}
fn primary() -> CalendarInfo {
    CalendarInfo {
        id: "calendar-owner@example.test".into(),
        title: "Synthetic primary".into(),
        time_zone: "Asia/Seoul".into(),
        primary: true,
        access_role: "owner".into(),
        email_notifications: false,
    }
}
fn fields() -> CalendarFields {
    CalendarFields {
        title: "Synthetic meeting".into(),
        description: "Synthetic only".into(),
        location: "Test room".into(),
        start: "2026-11-15T10:00:00+09:00".into(),
        end: "2026-11-15T11:00:00+09:00".into(),
        time_zone: Some("Asia/Seoul".into()),
    }
}
fn event(id: &str, fields: CalendarFields) -> CalendarEvent {
    CalendarEvent {
        id: id.into(),
        etag: "\"etag-prior\"".into(),
        status: "confirmed".into(),
        fields,
        html_link: None,
        writable: true,
        all_day: false,
        updated: None,
    }
}
fn pin() -> GoogleAppPin {
    GoogleAppPin {
        revision: None,
        client_id: "synthetic-calendar.apps.googleusercontent.com".into(),
        redirect_uri: REDIRECT.into(),
    }
}
fn secret(v: &str) -> SecretString {
    SecretString::new(v.into()).unwrap()
}
async fn fixture() -> (
    tempfile::TempDir,
    CalendarService,
    UserContext,
    CalendarConnection,
    Arc<FixtureIo>,
) {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path()).unwrap();
    store
        .bootstrap(
            "Synthetic owner".into(),
            "owner@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let owner = store
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let begin = store.start_calendar_oauth(&owner, pin()).await.unwrap();
    let handle = store
        .capture_calendar_oauth(begin.state, Some(secret("synthetic-code")))
        .await
        .unwrap()
        .unwrap();
    let exchange = store
        .claim_calendar_oauth(&owner, handle, pin())
        .await
        .unwrap();
    let connection = store
        .complete_calendar_oauth(
            exchange,
            TokenSet {
                access_token: secret("synthetic-access"),
                refresh_token: Some(secret("synthetic-refresh")),
                expires_in: 3600,
                scopes: Some(
                    moosuri_connectors::calendar::SCOPES
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
            },
            primary(),
        )
        .await
        .unwrap();
    let (changes, _) = watch::channel(0);
    let signals = Signals {
        stop: CancellationToken::new(),
        wake: Arc::new(Notify::new()),
        changes,
        delivery: Arc::new(tokio::sync::RwLock::new(())),
    };
    let mail = MailService::new(
        store.clone(),
        Some(Arc::new(OAuthClient {
            client_id: pin().client_id,
            client_secret: secret("synthetic-client-secret"),
            redirect_uri: REDIRECT.into(),
        })),
        signals,
    );
    let io = Arc::new(FixtureIo::new());
    let service = CalendarService {
        store,
        mail,
        io: Arc::new(CalendarIo::Fixture(io.clone())),
    };
    (d, service, owner, connection, io)
}
fn create(c: &CalendarConnection) -> CalendarChange {
    CalendarChange {
        request_id: uuid::Uuid::new_v4().to_string(),
        connection_id: c.id.clone(),
        expected_epoch: c.epoch,
        expected_revision: c.revision,
        mutation: CalendarMutation::Create { fields: fields() },
    }
}
fn range(c: &CalendarConnection) -> CalendarRange {
    CalendarRange {
        connection_id: c.id.clone(),
        start: fields().start,
        end: fields().end,
        page_token: Some("event-page".into()),
    }
}

#[tokio::test]
async fn duplicate_and_conflicting_payload_never_repeat_any_service_io() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    for mode in [0, 1] {
        let (_d, s, u, c, io) = fixture().await;
        io.write_mode.store(mode, Ordering::SeqCst);
        let change = create(&c);
        let first = s.change(&u, change.clone()).await.unwrap();
        assert_eq!(
            first.state,
            if mode == 0 {
                "applied"
            } else {
                "outcome_unknown"
            }
        );
        assert_eq!(io.write_count(), 1);
        let primary_calls = io.primary_calls.load(Ordering::SeqCst);
        let retry = s.change(&u, change.clone()).await.unwrap();
        assert_eq!(retry.id, first.id);
        assert_eq!(retry.state, first.state);
        let mut conflicting = change;
        conflicting.mutation = CalendarMutation::Create {
            fields: CalendarFields {
                title: "Different request".into(),
                ..fields()
            },
        };
        assert!(matches!(
            s.change(&u, conflicting).await,
            Err(StoreError::Conflict)
        ));
        assert_eq!(io.write_count(), 1);
        assert_eq!(io.primary_calls.load(Ordering::SeqCst), primary_calls);
        let record = io.writes.lock().unwrap()[0].clone();
        assert_eq!(record.id, first.id);
        assert_eq!(record.event_id, format!("m{}", first.id.replace('-', "")));
    }
}
#[tokio::test]
async fn unknown_lookup_checks_exact_original_operation_without_resending() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let (_d, s, u, c, io) = fixture().await;
    io.write_mode.store(1, Ordering::SeqCst);
    let request = create(&c);
    let unknown = s.change(&u, request.clone()).await.unwrap();
    for mode in [2, 3] {
        io.lookup_mode.store(mode, Ordering::SeqCst);
        assert_eq!(
            s.lookup(&u, unknown.id.clone()).await.unwrap().state,
            "outcome_unknown"
        );
    }
    io.lookup_mode.store(0, Ordering::SeqCst);
    let found = s.lookup(&u, unknown.id.clone()).await.unwrap();
    assert_eq!(found.state, "applied");
    assert_eq!(found.id, request.request_id);
    assert_eq!(io.write_count(), 1);
    assert_eq!(
        *io.event_calls.lock().unwrap(),
        vec![unknown.event_id.clone(); 3]
    );
    assert_eq!(s.change(&u, request).await.unwrap().state, "applied");
    assert_eq!(io.write_count(), 1);
}
#[tokio::test]
async fn unknown_delete_missing_lookup_is_current_absence_without_causal_success() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let (_d, s, u, c, io) = fixture().await;
    io.write_mode.store(1, Ordering::SeqCst);
    let mut request = create(&c);
    request.mutation = CalendarMutation::Delete {
        event_id: "synthetic1".into(),
        etag: event("synthetic1", fields()).etag,
    };
    let unknown = s.change(&u, request.clone()).await.unwrap();
    assert_eq!(unknown.state, "outcome_unknown");
    io.lookup_mode.store(1, Ordering::SeqCst);
    let found = s.lookup(&u, unknown.id).await.unwrap();
    assert_eq!(found.state, "observed_absent");
    assert_eq!(
        found.receipt.as_deref(),
        Some("current_absence_observed_causality_unknown")
    );
    assert!(found.event.is_none());
    assert_eq!(io.write_count(), 1);
    assert_eq!(
        s.change(&u, request).await.unwrap().state,
        "observed_absent"
    );
    assert_eq!(io.write_count(), 1);
}
#[tokio::test]
async fn withdrawal_while_primary_read_is_held_prevents_dispatch() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let (_d, s, u, c, io) = fixture().await;
    io.hold_primary.store(true, Ordering::SeqCst);
    let worker = s.clone();
    let owner = u.clone();
    let request = create(&c);
    let task = tokio::spawn(async move { worker.change(&owner, request).await });
    io.reached().await;
    s.store.disconnect_calendar(&u, c.id).await.unwrap();
    io.release.add_permits(1);
    assert!(matches!(task.await.unwrap(), Err(StoreError::Conflict)));
    assert_eq!(io.write_count(), 0);
    assert!(s.store.calendar_actions(&u).await.unwrap().is_empty());
}
#[tokio::test]
async fn committed_late_write_keeps_receipt_after_disconnect_or_logout_without_private_result() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    for disconnect in [true, false] {
        let (_d, s, u, c, io) = fixture().await;
        io.hold_mutation.store(true, Ordering::SeqCst);
        let request = create(&c);
        let retry = request.clone();
        let worker = s.clone();
        let owner = u.clone();
        let task = tokio::spawn(async move { worker.change(&owner, request).await });
        io.reached().await;
        assert_eq!(
            s.store.calendar_actions(&u).await.unwrap()[0].state,
            "dispatch_committed"
        );
        let reader = if disconnect {
            s.store.disconnect_calendar(&u, c.id).await.unwrap();
            u.clone()
        } else {
            s.store.logout(&u).await.unwrap();
            s.store
                .login("owner@example.test".into(), PASSWORD.into())
                .await
                .unwrap()
                .user
        };
        io.release.add_permits(1);
        assert!(matches!(task.await.unwrap(), Err(StoreError::Unauthorized)));
        let rows = s.store.calendar_actions(&reader).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, "applied");
        assert!(rows[0].receipt.is_some());
        assert!(rows[0].event.is_none());
        assert_eq!(io.write_count(), 1);
        assert_eq!(s.change(&reader, retry).await.unwrap().state, "applied");
        assert_eq!(io.write_count(), 1);
    }
}
#[tokio::test]
async fn stale_etag_and_provider_conflict_are_definite_without_repair_or_retry() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let (_d, s, u, c, io) = fixture().await;
    let mut request = create(&c);
    request.mutation = CalendarMutation::Update {
        event_id: "synthetic1".into(),
        etag: "stale-etag".into(),
        fields: fields(),
    };
    assert!(matches!(
        s.change(&u, request.clone()).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(io.write_count(), 0);
    request.mutation = CalendarMutation::Update {
        event_id: "synthetic1".into(),
        etag: event("synthetic1", fields()).etag,
        fields: fields(),
    };
    io.write_mode.store(2, Ordering::SeqCst);
    assert_eq!(
        s.change(&u, request.clone()).await.unwrap().state,
        "conflict"
    );
    assert_eq!(s.change(&u, request).await.unwrap().state, "conflict");
    assert_eq!(io.write_count(), 1);
}
#[tokio::test]
async fn partial_pages_survive_projection_and_late_read_is_withdrawn() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let (_d, s, u, c, io) = fixture().await;
    let calendars = s
        .calendars(&u, c.id.clone(), Some("calendar-page".into()))
        .await
        .unwrap();
    assert!(calendars.partial);
    assert_eq!(
        calendars.next_page_token.as_deref(),
        Some("next-calendar-page")
    );
    let events = s.events(&u, range(&c)).await.unwrap();
    assert!(events.partial);
    assert_eq!(events.next_page_token.as_deref(), Some("next-event-page"));
    io.hold_events.store(true, Ordering::SeqCst);
    let worker = s.clone();
    let owner = u.clone();
    let query = range(&c);
    let task = tokio::spawn(async move { worker.events(&owner, query).await });
    io.reached().await;
    s.store.disconnect_calendar(&u, c.id).await.unwrap();
    io.release.add_permits(1);
    assert!(matches!(task.await.unwrap(), Err(StoreError::Conflict)));
    assert_eq!(io.write_count(), 0);
}
#[test]
fn missing_event_is_unknown_for_create_and_only_observed_absence_for_delete() {
    for status in [404, 410] {
        let create = CalendarMutation::Create { fields: fields() };
        assert_eq!(
            lookup_result(
                &create,
                "synthetic1",
                "request",
                "digest",
                Err(GmailError::Http(status))
            )
            .0,
            "outcome_unknown"
        );
        let delete = CalendarMutation::Delete {
            event_id: "synthetic1".into(),
            etag: "etag".into(),
        };
        let result = lookup_result(
            &delete,
            "synthetic1",
            "request",
            "digest",
            Err(GmailError::Http(status)),
        );
        assert_eq!(result.0, "observed_absent");
        assert_eq!(result.2, "current_absence_observed_causality_unknown");
    }
}

#[path = "../tests/calendar.rs"]
mod conversation;
