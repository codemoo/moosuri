//! Named fixture scenarios. Real router, real member store/session/grant, no network:
//! the provider is a deterministic in-memory `CalendarProvider`.
use super::*;
use crate::personal_google::tests::{Fake, transport_for};
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst},
    },
};
use tower::ServiceExt;

type Marker = Option<(String, String)>;
#[derive(Default)]
struct Cal {
    events: Mutex<HashMap<String, (CalendarEvent, Marker)>>,
    seq: AtomicUsize,
    creates: AtomicUsize,
    moves: AtomicUsize,
    lists: AtomicUsize,
    drop_ack: AtomicBool,
    deny: AtomicBool,
    race: AtomicBool,
    no_readback: AtomicBool,
}
fn event(id: &str, etag: String, title: &str, start: &str, end: &str, zone: &str) -> CalendarEvent {
    CalendarEvent {
        id: id.into(),
        etag,
        status: "confirmed".into(),
        fields: CalendarFields {
            title: title.into(),
            description: "keep me".into(),
            location: "room 4".into(),
            start: start.into(),
            end: end.into(),
            time_zone: Some(zone.into()),
        },
        html_link: None,
        writable: true,
        all_day: false,
        updated: None,
    }
}
impl Cal {
    fn etag(&self) -> String {
        format!("\"v{}\"", self.seq.fetch_add(1, SeqCst) + 1)
    }
    fn seed(&self, id: &str, title: &str, start: &str, end: &str) {
        let e = event(id, self.etag(), title, start, end, "Asia/Seoul");
        self.events.lock().unwrap().insert(id.into(), (e, None));
    }
    /// A concurrent writer changes the event after the member's preview.
    fn concurrent_edit(&self, id: &str) {
        let etag = self.etag();
        self.events.lock().unwrap().get_mut(id).unwrap().0.etag = etag;
    }
    fn get(&self, id: &str) -> CalendarEvent {
        self.events.lock().unwrap()[id].0.clone()
    }
}
#[async_trait]
impl CalendarProvider for Cal {
    async fn list(
        &self,
        _: &SecretString,
        primary: &str,
        _: &str,
        _: &str,
    ) -> Result<CalendarEvents, GmailError> {
        self.lists.fetch_add(1, SeqCst);
        let mut events: Vec<_> = self
            .events
            .lock()
            .unwrap()
            .values()
            .map(|(e, _)| e.clone())
            .collect();
        events.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(CalendarEvents {
            events,
            next_page_token: None,
            partial: false,
            primary_id: primary.into(),
        })
    }
    async fn read(&self, _: &SecretString, _: &str, id: &str) -> Result<CalendarEvent, GmailError> {
        self.events
            .lock()
            .unwrap()
            .get(id)
            .map(|(e, _)| e.clone())
            .ok_or(GmailError::Http(404))
    }
    async fn create(
        &self,
        _: &SecretString,
        _: &str,
        id: &str,
        f: &CalendarFields,
        op: &str,
        d: &str,
    ) -> Result<CalendarEvent, MemberWriteError> {
        self.creates.fetch_add(1, SeqCst);
        if self.deny.load(SeqCst) {
            return Err(MemberWriteError::Write(GmailError::Http(403)));
        }
        let mut e = event(
            id,
            self.etag(),
            &f.title,
            &f.start,
            &f.end,
            f.time_zone.as_deref().unwrap(),
        );
        e.fields.description = f.description.clone();
        self.events
            .lock()
            .unwrap()
            .insert(id.into(), (e.clone(), Some((op.into(), d.into()))));
        if self.drop_ack.load(SeqCst) {
            return Err(MemberWriteError::Write(GmailError::Transport));
        }
        Ok(e)
    }
    async fn move_event(
        &self,
        _: &SecretString,
        _: &str,
        id: &str,
        etag: &str,
        f: &CalendarFields,
        op: &str,
        d: &str,
    ) -> Result<CalendarEvent, MemberWriteError> {
        let current = self.get(id);
        if current.etag != etag || self.race.load(SeqCst) {
            return Err(MemberWriteError::Stale(Box::new(current)));
        }
        self.moves.fetch_add(1, SeqCst);
        let mut e = current;
        e.etag = self.etag();
        e.fields.start = f.start.clone();
        e.fields.end = f.end.clone();
        e.fields.title = f.title.clone();
        e.fields.description = f.description.clone();
        e.fields.location = f.location.clone();
        self.events
            .lock()
            .unwrap()
            .insert(id.into(), (e.clone(), Some((op.into(), d.into()))));
        if self.drop_ack.load(SeqCst) {
            return Err(MemberWriteError::Write(GmailError::Timeout));
        }
        Ok(e)
    }
    async fn readback(
        &self,
        _: &SecretString,
        _: &str,
        id: &str,
        op: &str,
        d: &str,
        _: &CalendarFields,
    ) -> Result<Readback, GmailError> {
        if self.no_readback.load(SeqCst) {
            return Err(GmailError::Transport);
        }
        match self.events.lock().unwrap().get(id) {
            None => Err(GmailError::Http(404)),
            Some((e, Some((o, g)))) if o == op && g == d => {
                Ok(Readback::Applied(Box::new(e.clone())))
            }
            Some((e, _)) => Ok(Readback::Different(Box::new(e.clone()))),
        }
    }
}

struct World {
    app: Router,
    g: Google,
    cal: Arc<Cal>,
    token: String,
    conn: Value,
    _dir: tempfile::TempDir,
}
async fn member(root: &std::path::Path) -> (Store, VolumeOwner, String) {
    fs::create_dir(root).unwrap();
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    let owner = VolumeOwner {
        account: Uuid::new_v4(),
        tenant: Uuid::new_v4(),
        volume: Uuid::new_v4(),
        generation: 1,
    };
    owner
        .initialize(
            root,
            "Synthetic member".into(),
            "a@example.test".into(),
            "Synthetic-member-password-42".into(),
        )
        .await
        .unwrap();
    let store = owner.open_verified(root).await.unwrap();
    let login = store
        .login(
            "a@example.test".into(),
            "Synthetic-member-password-42".into(),
        )
        .await
        .unwrap();
    let login = store
        .change_password(&login.user, "Changed-member-password-42".into())
        .await
        .unwrap();
    (store, owner, login.token)
}
async fn send(
    app: &Router,
    owner: VolumeOwner,
    token: &str,
    path: &str,
    method: &str,
    body: Value,
) -> (StatusCode, Value) {
    let reply = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "personal.internal")
                .header("origin", "https://personal.internal")
                .header("x-moosuri-account", owner.account.to_string())
                .header("cookie", format!("moosuri_session={token}"))
                .header("content-type", "application/json")
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = reply.status();
    let bytes = to_bytes(reply.into_body(), 131072).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
/// Real consent bookkeeping through the store, with the fixture broker transport.
async fn connect(g: &Google, token: &str) -> Value {
    let user = g.store.authenticate(token.to_string()).await.unwrap();
    let rev = match g.describe().await {
        Ok(v) => v.app_revision,
        Err(_) => panic!("describe"),
    };
    let start = g
        .store
        .start_member_service_oauth(&user, g.owner, Service::Calendar, rev)
        .await
        .unwrap();
    let handle = g
        .store
        .capture_member_service_oauth(
            Service::Calendar,
            start.state.clone(),
            Some(SecretString::new("synthetic-code".into()).unwrap()),
        )
        .await
        .unwrap();
    let exchange = g
        .store
        .claim_member_service_oauth(&user, g.owner, Service::Calendar, rev, handle.unwrap())
        .await
        .unwrap();
    let id = exchange.member_google_request_id().unwrap();
    let raw = match g
        .raw(
            id,
            Some(rev),
            Operation::Exchange {
                code: Secret(&exchange.code),
                verifier: Secret(&exchange.verifier),
            },
        )
        .await
    {
        Ok(v) => v,
        Err(_) => panic!("exchange"),
    };
    let grant = moosuri_store::MemberGoogleGrant::from_broker_reply(id, raw).unwrap();
    let st = g
        .store
        .complete_member_google_oauth(exchange, grant)
        .await
        .unwrap();
    json!({"connection_id":st.connection_id,"epoch":st.epoch})
}
async fn world(provider: Option<Arc<Cal>>) -> World {
    let dir = tempfile::tempdir().unwrap();
    let (store, owner, token) = member(&dir.path().join("member")).await;
    let transport = transport_for(
        Arc::new(Fake::default()),
        Uuid::new_v4(),
        "a@example.test".into(),
        "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
    );
    let g = Google {
        service: Service::Calendar,
        store,
        owner,
        backend: Backend::Fixture(transport),
        serial: Default::default(),
    };
    let cal = provider.unwrap_or_default();
    let app = router(g.clone(), cal.clone(), || 1_795_000_000);
    let conn = connect(&g, &token).await;
    World {
        app,
        g,
        cal,
        token,
        conn,
        _dir: dir,
    }
}
fn create_body(w: &World, request: Uuid) -> Value {
    json!({"request_id":request,"connection_id":w.conn["connection_id"],"epoch":w.conn["epoch"],"kind":"create","title":"팀 회의","date":"2026-11-26","time":"15:00","time_zone":"Asia/Seoul","duration_minutes":60})
}
fn move_body(w: &World, request: Uuid, title: &str, candidate: Option<&str>) -> Value {
    let mut v = json!({"request_id":request,"connection_id":w.conn["connection_id"],"epoch":w.conn["epoch"],"kind":"move","title":title,"date":"2026-11-28","time":"16:00","time_zone":"Asia/Seoul","from_date":"2026-11-26"});
    if let Some(c) = candidate {
        v["candidate_ref"] = json!(c);
    }
    v
}
async fn prepare(w: &World, body: Value) -> (StatusCode, Value) {
    send(&w.app, w.g.owner, &w.token, PREPARE, "POST", body).await
}
async fn apply(w: &World, body: Value, digest: &str) -> (StatusCode, Value) {
    send(
        &w.app,
        w.g.owner,
        &w.token,
        APPLY,
        "POST",
        json!({"request":body,"approved_digest":digest}),
    )
    .await
}

#[tokio::test]
async fn scenario_create_with_local_preview_then_confirmed_provider_receipt() {
    let _f = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let w = world(None).await;
    let request = Uuid::new_v4();
    let user = w.g.store.authenticate(w.token.clone()).await.unwrap();
    let thread =
        w.g.store
            .create_thread(&user, "Calendar receipt".into())
            .await
            .unwrap()
            .id;
    let mut body = create_body(&w, request);
    body["thread_id"] = json!(thread);
    let (st, p) = prepare(&w, body.clone()).await;
    assert_eq!((st, p["state"].as_str()), (StatusCode::OK, Some("preview")));
    assert_eq!(p["preview"]["start"], "2026-11-26T15:00:00+09:00");
    assert_eq!(p["preview"]["end"], "2026-11-26T16:00:00+09:00");
    // The preview is local: nothing reached the provider.
    assert_eq!(
        (w.cal.creates.load(SeqCst), w.cal.lists.load(SeqCst)),
        (0, 0)
    );
    let digest = p["preview"]["digest"].as_str().unwrap().to_owned();
    let (st, r) = apply(&w, body.clone(), &digest).await;
    assert_eq!(st, StatusCode::OK);
    assert_eq!(
        (
            r["state"].as_str(),
            r["confirmed_by"].as_str(),
            r["receipt"].as_str()
        ),
        (
            Some("confirmed"),
            Some("provider_response"),
            Some("created")
        )
    );
    assert_eq!(r["event"]["title"], "팀 회의");
    assert_eq!(w.cal.creates.load(SeqCst), 1);
    // Duplicate request (browser retry): same receipt, no second create.
    let (_, again) = apply(&w, body.clone(), &digest).await;
    assert_eq!(again, r);
    assert_eq!(w.cal.creates.load(SeqCst), 1);
    assert_eq!(w.cal.events.lock().unwrap().len(), 1);
    let page =
        w.g.store
            .personal_task_page(w.g.owner, user.clone(), None)
            .await
            .unwrap();
    let task = page.items.iter().find(|t| t.kind == "calendar").unwrap();
    let detail =
        w.g.store
            .personal_task_detail(w.g.owner, user.clone(), Uuid::parse_str(&task.id).unwrap())
            .await
            .unwrap();
    let origin = Uuid::parse_str(detail.origin_turn_id.as_deref().unwrap()).unwrap();
    let reopened =
        w.g.store
            .personal_task_detail(w.g.owner, user, origin)
            .await
            .unwrap();
    assert_eq!(reopened.task.id, task.id);
    assert_eq!(reopened.task.thread_id, thread);
    assert_eq!(reopened.task.state, "applied");
    assert!(reopened.result.as_deref().unwrap().contains("팀 회의"));
    assert!(
        reopened
            .result
            .unwrap()
            .contains("2026-11-26T15:00:00+09:00")
    );
    assert_eq!(w.cal.creates.load(SeqCst), 1);
}

#[tokio::test]
async fn scenario_ambiguous_title_needs_explicit_selection_then_move_keeps_other_fields() {
    let _f = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let w = world(None).await;
    w.cal.seed(
        "evt1aaaaaa",
        "팀 회의",
        "2026-11-26T10:00:00+09:00",
        "2026-11-26T11:30:00+09:00",
    );
    w.cal.seed(
        "evt2bbbbbb",
        "팀 회의",
        "2026-11-26T14:00:00+09:00",
        "2026-11-26T15:00:00+09:00",
    );
    w.cal.seed(
        "evt3cccccc",
        "점심",
        "2026-11-26T12:00:00+09:00",
        "2026-11-26T13:00:00+09:00",
    );
    let request = Uuid::new_v4();
    let (_, p) = prepare(&w, move_body(&w, request, "팀 회의", None)).await;
    assert_eq!(p["state"], "needs_selection");
    let candidates = p["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2, "same-title candidates only");
    assert_eq!(w.cal.moves.load(SeqCst), 0);
    let second = candidates
        .iter()
        .find(|c| c["start"] == "2026-11-26T14:00:00+09:00")
        .unwrap();
    let handle = second["candidate_ref"].as_str().unwrap().to_owned();
    let (_, p) = prepare(&w, move_body(&w, request, "팀 회의", Some(&handle))).await;
    assert_eq!(p["state"], "preview");
    assert_eq!(
        p["preview"]["current"]["start"],
        "2026-11-26T14:00:00+09:00"
    );
    assert_eq!(p["preview"]["start"], "2026-11-28T16:00:00+09:00");
    assert_eq!(
        p["preview"]["end"], "2026-11-28T17:00:00+09:00",
        "observed 1h duration is kept"
    );
    let digest = p["preview"]["digest"].as_str().unwrap().to_owned();
    // A move is not accepted without the explicit selection.
    assert_eq!(
        apply(&w, move_body(&w, request, "팀 회의", None), &digest)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let (_, r) = apply(
        &w,
        move_body(&w, request, "팀 회의", Some(&handle)),
        &digest,
    )
    .await;
    assert_eq!(
        (r["state"].as_str(), r["receipt"].as_str()),
        (Some("confirmed"), Some("moved"))
    );
    let moved = w.cal.get("evt2bbbbbb");
    assert_eq!(moved.fields.start, "2026-11-28T16:00:00+09:00");
    assert_eq!(
        (
            moved.fields.description.as_str(),
            moved.fields.location.as_str()
        ),
        ("keep me", "room 4")
    );
    assert_eq!(
        w.cal.get("evt1aaaaaa").fields.start,
        "2026-11-26T10:00:00+09:00"
    );
    assert_eq!(w.cal.moves.load(SeqCst), 1);
    // A single title match needs no extra choice but still previews before any write.
    let (_, one) = prepare(&w, move_body(&w, Uuid::new_v4(), "점심", None)).await;
    assert_eq!(one["state"], "preview");
    assert_eq!(w.cal.moves.load(SeqCst), 1);
}

#[tokio::test]
async fn scenario_stale_concurrent_version_is_a_definite_conflict_with_no_write() {
    let _f = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let w = world(None).await;
    w.cal.seed(
        "evt1aaaaaa",
        "보고",
        "2026-11-26T10:00:00+09:00",
        "2026-11-26T11:00:00+09:00",
    );
    let request = Uuid::new_v4();
    let (_, p) = prepare(&w, move_body(&w, request, "보고", None)).await;
    let digest = p["preview"]["digest"].as_str().unwrap().to_owned();
    let handle = p["preview"]["current"]["candidate_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    // 1. Someone else edits the event after the member's preview.
    w.cal.concurrent_edit("evt1aaaaaa");
    let (_, r) = apply(&w, move_body(&w, request, "보고", Some(&handle)), &digest).await;
    assert_eq!(r["state"], "conflict");
    assert_eq!(w.cal.moves.load(SeqCst), 0);
    // 2. The edit lands between the pre-flight read and the If-Match PATCH.
    let second = Uuid::new_v4();
    let (_, p) = prepare(&w, move_body(&w, second, "보고", None)).await;
    let digest = p["preview"]["digest"].as_str().unwrap().to_owned();
    let handle = p["preview"]["current"]["candidate_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    w.cal.race.store(true, SeqCst);
    let (_, r) = apply(&w, move_body(&w, second, "보고", Some(&handle)), &digest).await;
    assert_eq!(r["state"], "conflict");
    let (_, again) = apply(&w, move_body(&w, second, "보고", Some(&handle)), &digest).await;
    assert_eq!(
        again["state"], "conflict",
        "recorded outcome, not a new attempt"
    );
    assert_eq!(w.cal.moves.load(SeqCst), 0);
    assert_eq!(
        w.cal.get("evt1aaaaaa").fields.start,
        "2026-11-26T10:00:00+09:00"
    );
}

#[tokio::test]
async fn scenario_lost_acknowledgement_is_reconciled_by_readback_and_never_duplicates() {
    let _f = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let w = world(None).await;
    w.cal.drop_ack.store(true, SeqCst);
    let request = Uuid::new_v4();
    let (_, p) = prepare(&w, create_body(&w, request)).await;
    let digest = p["preview"]["digest"].as_str().unwrap().to_owned();
    let (_, r) = apply(&w, create_body(&w, request), &digest).await;
    assert_eq!(
        (r["state"].as_str(), r["confirmed_by"].as_str()),
        (Some("confirmed"), Some("readback"))
    );
    let (_, again) = apply(&w, create_body(&w, request), &digest).await;
    assert_eq!(again, r);
    assert_eq!(w.cal.creates.load(SeqCst), 1);
    // Readback itself unavailable: honest `unknown`, never `confirmed`, never re-sent.
    w.cal.no_readback.store(true, SeqCst);
    let second = Uuid::new_v4();
    let (_, p) = prepare(&w, create_body(&w, second)).await;
    let digest = p["preview"]["digest"].as_str().unwrap().to_owned();
    let (_, r) = apply(&w, create_body(&w, second), &digest).await;
    assert_eq!(
        (r["state"].as_str(), r["code"].as_str()),
        (Some("unknown"), Some("readback_failed"))
    );
    assert_eq!(w.cal.creates.load(SeqCst), 2);
    w.cal.no_readback.store(false, SeqCst);
    let (_, r) = apply(&w, create_body(&w, second), &digest).await;
    assert_eq!(
        (r["state"].as_str(), r["confirmed_by"].as_str()),
        (Some("confirmed"), Some("readback"))
    );
    assert_eq!(w.cal.creates.load(SeqCst), 2, "reconcile is read-only");
}

#[tokio::test]
async fn scenario_permission_owner_epoch_and_revocation() {
    let _f = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let w = world(None).await;
    // Google refuses the write: member must re-consent; the refusal is a definite no-write.
    w.cal.deny.store(true, SeqCst);
    let request = Uuid::new_v4();
    let (_, p) = prepare(&w, create_body(&w, request)).await;
    let digest = p["preview"]["digest"].as_str().unwrap().to_owned();
    let (_, r) = apply(&w, create_body(&w, request), &digest).await;
    assert_eq!(
        (r["state"].as_str(), r["reason"].as_str()),
        (Some("needs_connection"), Some("calendar_permission_denied"))
    );
    let (_, again) = apply(&w, create_body(&w, request), &digest).await;
    assert_eq!(again["state"], "needs_connection");
    assert_eq!(w.cal.creates.load(SeqCst), 1);
    // Another session / a stale connection epoch / malformed input.
    assert_eq!(
        send(
            &w.app,
            w.g.owner,
            "other-session",
            PREPARE,
            "POST",
            create_body(&w, Uuid::new_v4())
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let mut wrong = create_body(&w, Uuid::new_v4());
    wrong["epoch"] = json!(w.conn["epoch"].as_i64().unwrap() + 1);
    assert_eq!(prepare(&w, wrong).await.0, StatusCode::CONFLICT);
    let mut no_duration = create_body(&w, Uuid::new_v4());
    no_duration
        .as_object_mut()
        .unwrap()
        .remove("duration_minutes");
    assert_eq!(prepare(&w, no_duration).await.0, StatusCode::BAD_REQUEST);
    let mut invitees = create_body(&w, Uuid::new_v4());
    invitees["attendees"] = json!(["someone@example.test"]);
    assert!(
        prepare(&w, invitees).await.0.is_client_error(),
        "no invitation field exists"
    );
    // Status shows the connection, then revocation requires consent again.
    let (st, status) = send(&w.app, w.g.owner, &w.token, STATUS, "GET", Value::Null).await;
    assert_eq!(
        (st, &status["connection"]["connection_id"]),
        (StatusCode::OK, &w.conn["connection_id"])
    );
    let user = w.g.store.authenticate(w.token.clone()).await.unwrap();
    w.g.store
        .disconnect_member_service(
            &user,
            w.g.owner,
            Service::Calendar,
            w.conn["connection_id"].as_str().unwrap().parse().unwrap(),
            w.conn["epoch"].as_i64().unwrap(),
        )
        .await
        .unwrap();
    let (_, p) = prepare(&w, create_body(&w, Uuid::new_v4())).await;
    assert_eq!(
        (p["state"].as_str(), p["reason"].as_str()),
        (Some("needs_connection"), Some("disconnected"))
    );
    let (_, r) = apply(&w, create_body(&w, request), &digest).await;
    assert_eq!(r["state"], "needs_connection");
}

#[tokio::test]
async fn scenario_update_wording_approval_digest_binds_the_final_content() {
    let _f = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let w = world(None).await;
    let request = Uuid::new_v4();
    let (_, p) = prepare(&w, create_body(&w, request)).await;
    let digest = p["preview"]["digest"].as_str().unwrap().to_owned();
    // The member edits the wording/time after approving: the old approval is void.
    let mut edited = create_body(&w, request);
    edited["time"] = json!("17:00");
    let (st, r) = apply(&w, edited.clone(), &digest).await;
    assert_eq!(
        (st, r["code"].as_str()),
        (StatusCode::CONFLICT, Some("approval_mismatch"))
    );
    assert_eq!(w.cal.creates.load(SeqCst), 0);
    // Re-preview yields a new digest for the new content.
    let (_, p2) = prepare(&w, edited.clone()).await;
    assert_ne!(p2["preview"]["digest"], p["preview"]["digest"]);
    assert_eq!(p2["preview"]["start"], "2026-11-26T17:00:00+09:00");
    let (_, r) = apply(&w, edited, p2["preview"]["digest"].as_str().unwrap()).await;
    assert_eq!(r["state"], "confirmed");
    assert_eq!(w.cal.creates.load(SeqCst), 1);
}

#[tokio::test]
async fn scenario_nothing_reaches_google_unless_a_provider_is_enabled() {
    let _f = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let w = world(None).await;
    let off = router(w.g.clone(), disabled_provider(), || 1_795_000_000);
    let request = Uuid::new_v4();
    let (_, p) = send(
        &off,
        w.g.owner,
        &w.token,
        PREPARE,
        "POST",
        create_body(&w, request),
    )
    .await;
    let digest = p["preview"]["digest"].as_str().unwrap().to_owned();
    let (_, r) = send(
        &off,
        w.g.owner,
        &w.token,
        APPLY,
        "POST",
        json!({"request":create_body(&w, request),"approved_digest":digest}),
    )
    .await;
    assert_eq!(
        (r["state"].as_str(), r["code"].as_str()),
        (Some("failed"), Some("provider_unavailable"))
    );
    let (st, _) = send(
        &off,
        w.g.owner,
        &w.token,
        PREPARE,
        "POST",
        move_body(&w, Uuid::new_v4(), "x", None),
    )
    .await;
    assert_eq!(st, StatusCode::BAD_GATEWAY);
    assert!(
        public_response(
            "/api/v1/personal/google/inbox",
            &Method::POST,
            StatusCode::OK,
            b"{}"
        )
        .is_none()
    );
    assert!(matches!(
        public_response(
            PREPARE,
            &Method::POST,
            StatusCode::OK,
            br#"{"state":"preview","extra":1}"#
        ),
        Some(Err(_))
    ));
}
