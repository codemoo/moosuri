//! Member-private Gmail research -> local reply draft -> standard final approval.
//!
//! Mounted only on the member daemon next to the existing personal_google
//! routes; every route authenticates the original member session. The cited
//! source is a completed member readonly snapshot (read through the existing
//! `/inbox` route with a read intent). Nothing here reads Gmail, and nothing
//! here sends: the standard dispatcher sends an approved MailAction, and the
//! standard journaled `MailService` adapter is the only thing that writes a
//! Gmail draft. A readonly member connection yields an honest local-only result.
#![allow(dead_code)] // removable once main mounts `routes` and `public_response`.
use super::{Failure, response};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, Method, StatusCode},
    response::Response,
    routing::post,
};
use moosuri_core::{GmailDraftRef, GmailDraftSave};
use moosuri_store::{
    MemberMailSaveAdmission, MemberMailWork, MemberMailWorkApproval, MemberMailWorkInput,
    MemberMailWorkPatch, MemberMailWorkPrepared, Store, StoreError, UserContext,
    personal_volume::VolumeOwner,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

pub(crate) const BASE: &str = "/api/v1/personal/google/mail-work";
pub(crate) const ACTIONS: [&str; 7] = [
    "list",
    "prepare",
    "revise",
    "preview",
    "save-draft",
    "approve",
    "discard",
];

/// The only external writer. Production wraps the standard MailService, which
/// owns token refresh, the draft journal, readback and receipts.
#[async_trait::async_trait]
pub(crate) trait MailWriter: Send + Sync {
    /// False when the installation has no Google application configured for
    /// standard Gmail writes; checked before any attempt is recorded.
    async fn ready(&self) -> bool;
    async fn save_draft(
        &self,
        user: &UserContext,
        action: String,
        request: GmailDraftSave,
    ) -> Result<GmailDraftRef, StoreError>;
}
pub(crate) struct StandardMailWriter(pub moosuri_agent::mail::MailService);
#[async_trait::async_trait]
impl MailWriter for StandardMailWriter {
    async fn ready(&self) -> bool {
        self.0.configured().await
    }
    async fn save_draft(
        &self,
        user: &UserContext,
        action: String,
        request: GmailDraftSave,
    ) -> Result<GmailDraftRef, StoreError> {
        self.0
            .save_gmail_draft(
                user,
                action,
                request,
                &moosuri_connectors::model::CancellationToken::new(),
            )
            .await
    }
}

#[derive(Clone)]
struct Work {
    store: Store,
    owner: VolumeOwner,
    writer: Option<Arc<dyn MailWriter>>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    serial: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListInput {
    thread_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviseInput {
    id: Uuid,
    revision: i64,
    #[serde(default)]
    to: Option<Vec<String>>,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    body: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewInput {
    id: Uuid,
    revision: i64,
    connection_id: String,
    connection_epoch: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActInput {
    id: Uuid,
    revision: i64,
    action_revision: i64,
    action_digest: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiscardInput {
    id: Uuid,
    revision: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Items {
    items: Vec<MemberMailWork>,
}

/// Mount beside `personal_google::routes`. `wake` nudges the standard mail
/// dispatcher after an approval (the global route's `runtime.wake()`).
pub(crate) fn routes(
    store: Store,
    owner: VolumeOwner,
    writer: Option<Arc<dyn MailWriter>>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
) -> Router {
    let state = Work {
        store,
        owner,
        writer,
        wake,
        serial: Default::default(),
    };
    Router::new()
        .route(&format!("{BASE}/list"), post(list))
        .route(&format!("{BASE}/prepare"), post(prepare))
        .route(&format!("{BASE}/revise"), post(revise))
        .route(&format!("{BASE}/preview"), post(preview))
        .route(&format!("{BASE}/save-draft"), post(save_draft))
        .route(&format!("{BASE}/approve"), post(approve))
        .route(&format!("{BASE}/discard"), post(discard))
        .with_state(state)
        .layer(DefaultBodyLimit::max(32 * 1024))
}

/// Gateway allowlist hook. `None` means the path is not a mail-work path; the
/// caller must then continue with its normal allowlist. Every success body is
/// parsed and re-encoded through the typed DTOs; unknown fields/states fail.
pub(crate) fn public_response(
    path: &str,
    method: &Method,
    status: StatusCode,
    bytes: &[u8],
) -> Option<Result<Vec<u8>, StatusCode>> {
    let action = path.strip_prefix(BASE)?.strip_prefix('/')?;
    Some(checked(action, method, status, bytes))
}
fn checked(
    action: &str,
    method: &Method,
    status: StatusCode,
    bytes: &[u8],
) -> Result<Vec<u8>, StatusCode> {
    let invalid = StatusCode::BAD_GATEWAY;
    if method != Method::POST
        || status != StatusCode::OK
        || bytes.len() > 1024 * 1024
        || !ACTIONS.contains(&action)
    {
        return Err(invalid);
    }
    let decode = |v: &[u8]| -> Result<serde_json::Value, StatusCode> {
        serde_json::from_slice(v).map_err(|_| invalid)
    };
    fn encode<T: Serialize>(v: &T) -> Result<Vec<u8>, StatusCode> {
        serde_json::to_vec(v).map_err(|_| StatusCode::BAD_GATEWAY)
    }
    let value = decode(bytes)?;
    match action {
        "list" => {
            let v: Items = serde_json::from_value(value).map_err(|_| invalid)?;
            if v.items.len() > 20 || !v.items.iter().all(MemberMailWork::valid) {
                return Err(invalid);
            }
            encode(&v)
        }
        "prepare" => {
            let v: MemberMailWorkPrepared = serde_json::from_value(value).map_err(|_| invalid)?;
            let ok = match &v {
                MemberMailWorkPrepared::Draft { work } => work.valid(),
                MemberMailWorkPrepared::NeedsDetails { missing } => {
                    !missing.is_empty()
                        && missing.len() <= 2
                        && missing.iter().all(|m| m == "to" || m == "body")
                }
                MemberMailWorkPrepared::SourceUnavailable { read_state } => {
                    !read_state.is_empty()
                        && read_state.len() <= 32
                        && read_state
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b == b'_')
                }
            };
            if !ok {
                return Err(invalid);
            }
            encode(&v)
        }
        _ => {
            let v: MemberMailWork = serde_json::from_value(value).map_err(|_| invalid)?;
            if !v.valid() {
                return Err(invalid);
            }
            encode(&v)
        }
    }
}
impl Work {
    async fn user(&self, headers: &HeaderMap) -> Result<UserContext, Failure> {
        let user = moosuri_server::personal_request_user(&self.store, headers).await?;
        if user.account().id != self.owner.account.to_string() {
            return Err(StoreError::Forbidden.into());
        }
        Ok(user)
    }
    fn serial(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, Failure> {
        self.serial
            .try_lock()
            .map_err(|_| Failure(StatusCode::CONFLICT, "conflict"))
    }
}
fn reply<T: Serialize>(action: &str, value: &T) -> Result<Response, Failure> {
    let unavailable = || Failure(StatusCode::SERVICE_UNAVAILABLE, "unavailable");
    let bytes = serde_json::to_vec(value).map_err(|_| unavailable())?;
    let bytes =
        checked(action, &Method::POST, StatusCode::OK, &bytes).map_err(|_| unavailable())?;
    Ok(response(StatusCode::OK, bytes))
}
/// A missing scope is a typed, actionable refusal, not a generic 403.
fn scoped(code: &'static str) -> impl Fn(StoreError) -> Failure {
    move |e| match e {
        StoreError::Forbidden => Failure(StatusCode::FORBIDDEN, code),
        e => e.into(),
    }
}

async fn list(
    State(w): State<Work>,
    headers: HeaderMap,
    Json(input): Json<ListInput>,
) -> Result<Response, Failure> {
    let user = w.user(&headers).await?;
    let items = w
        .store
        .member_mail_work_list(&user, w.owner, input.thread_id)
        .await?;
    reply("list", &Items { items })
}
async fn prepare(
    State(w): State<Work>,
    headers: HeaderMap,
    Json(input): Json<MemberMailWorkInput>,
) -> Result<Response, Failure> {
    let user = w.user(&headers).await?;
    let prepared = w
        .store
        .prepare_member_mail_work(&user, w.owner, input)
        .await?;
    w.user(&headers).await?;
    reply("prepare", &prepared)
}
async fn revise(
    State(w): State<Work>,
    headers: HeaderMap,
    Json(input): Json<ReviseInput>,
) -> Result<Response, Failure> {
    let user = w.user(&headers).await?;
    let work = w
        .store
        .revise_member_mail_work(
            &user,
            w.owner,
            input.id,
            input.revision,
            MemberMailWorkPatch {
                to: input.to,
                subject: input.subject,
                body: input.body,
            },
        )
        .await?;
    reply("revise", &work)
}
async fn preview(
    State(w): State<Work>,
    headers: HeaderMap,
    Json(input): Json<PreviewInput>,
) -> Result<Response, Failure> {
    let user = w.user(&headers).await?;
    if input.connection_id.is_empty() || input.connection_epoch <= 0 {
        return Err(StoreError::InvalidInput.into());
    }
    let work = w
        .store
        .preview_member_mail_work(
            &user,
            w.owner,
            input.id,
            input.revision,
            input.connection_id,
            input.connection_epoch,
        )
        .await
        .map_err(scoped("write_scope_missing"))?;
    reply("preview", &work)
}
async fn save_draft(
    State(w): State<Work>,
    headers: HeaderMap,
    Json(input): Json<ActInput>,
) -> Result<Response, Failure> {
    let user = w.user(&headers).await?;
    let _serial = w.serial()?;
    let unavailable = || Failure(StatusCode::SERVICE_UNAVAILABLE, "mail_writer_unavailable");
    let writer = w.writer.clone().ok_or_else(unavailable)?;
    if !writer.ready().await {
        return Err(unavailable());
    }
    let admission = w
        .store
        .begin_member_mail_provider_save(
            &user,
            w.owner,
            input.id,
            MemberMailWorkApproval {
                revision: input.revision,
                action_revision: input.action_revision,
                action_digest: input.action_digest,
            },
        )
        .await
        .map_err(scoped("write_scope_missing"))?;
    let work = match admission {
        MemberMailSaveAdmission::Recorded(work) | MemberMailSaveAdmission::Gone(work) => *work,
        MemberMailSaveAdmission::Fresh(lease) => {
            let result = writer
                .save_draft(
                    lease.user(),
                    lease.action_id().to_owned(),
                    lease.request().clone(),
                )
                .await;
            // Only refusals that provably precede any provider write are
            // definite. Everything else (including a standard readback
            // conflict) stays unknown and is resolved by the standard journal.
            let (receipt, definite) = match result {
                Ok(receipt) => (Some(receipt), false),
                Err(
                    StoreError::Forbidden
                    | StoreError::Unauthorized
                    | StoreError::NotFound
                    | StoreError::InvalidInput,
                ) => (None, true),
                Err(_) => (None, false),
            };
            let work = w
                .store
                .finish_member_mail_provider_save(lease, receipt, definite)
                .await?;
            // Disclosure only to a still-valid original session.
            w.user(&headers).await?;
            work
        }
    };
    reply("save-draft", &work)
}
async fn approve(
    State(w): State<Work>,
    headers: HeaderMap,
    Json(input): Json<ActInput>,
) -> Result<Response, Failure> {
    let user = w.user(&headers).await?;
    let _serial = w.serial()?;
    if let Some(writer) = &w.writer {
        if !writer.ready().await {
            return Err(Failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "mail_writer_unavailable",
            ));
        }
    } else {
        return Err(Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "mail_writer_unavailable",
        ));
    }
    let work = w
        .store
        .approve_member_mail_work(
            &user,
            w.owner,
            input.id,
            MemberMailWorkApproval {
                revision: input.revision,
                action_revision: input.action_revision,
                action_digest: input.action_digest,
            },
        )
        .await
        .map_err(scoped("send_scope_missing"))?;
    if let Some(wake) = &w.wake {
        wake();
    }
    reply("approve", &work)
}
async fn discard(
    State(w): State<Work>,
    headers: HeaderMap,
    Json(input): Json<DiscardInput>,
) -> Result<Response, Failure> {
    let user = w.user(&headers).await?;
    let work = w
        .store
        .discard_member_mail_work(&user, w.owner, input.id, input.revision)
        .await?;
    reply("discard", &work)
}

#[cfg(test)]
mod tests {
    use super::super::{
        BASE as GOOGLE,
        tests::{Fake, transport_for},
    };
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use moosuri_connectors::gmail::{SecretString, TokenSet};
    use serde_json::json;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tower::ServiceExt;

    struct FakeWriter {
        calls: AtomicUsize,
        ready: bool,
        fail: bool,
    }
    #[async_trait::async_trait]
    impl MailWriter for FakeWriter {
        async fn ready(&self) -> bool {
            self.ready
        }
        async fn save_draft(
            &self,
            _user: &UserContext,
            _action: String,
            _request: GmailDraftSave,
        ) -> Result<GmailDraftRef, StoreError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(StoreError::Unavailable);
            }
            Ok(GmailDraftRef {
                id: "draft-1".into(),
                message_id: "message-1".into(),
            })
        }
    }
    fn writer(ready: bool, fail: bool) -> Arc<FakeWriter> {
        Arc::new(FakeWriter {
            calls: AtomicUsize::new(0),
            ready,
            fail,
        })
    }
    struct Env {
        _temp: tempfile::TempDir,
        store: Store,
        owner: VolumeOwner,
        token: String,
        app: Router,
        thread: String,
        read: Uuid,
    }
    async fn request(
        app: &Router,
        owner: VolumeOwner,
        token: &str,
        path: &str,
        value: serde_json::Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("host", "personal.internal")
                    .header("origin", "https://personal.internal")
                    .header("x-moosuri-account", owner.account.to_string())
                    .header("cookie", format!("moosuri_session={token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(value.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn value(reply: Response) -> serde_json::Value {
        serde_json::from_slice(&to_bytes(reply.into_body(), 1 << 20).await.unwrap()).unwrap()
    }
    async fn env(writer: Option<Arc<dyn MailWriter>>) -> Env {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("member");
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = VolumeOwner {
            account: Uuid::new_v4(),
            tenant: Uuid::new_v4(),
            volume: Uuid::new_v4(),
            generation: 1,
        };
        owner
            .initialize(
                &root,
                "Synthetic member".into(),
                "a@example.test".into(),
                "Synthetic-member-password-42".into(),
            )
            .await
            .unwrap();
        let store = owner.open_verified(&root).await.unwrap();
        let login = store
            .login(
                "a@example.test".into(),
                "Synthetic-member-password-42".into(),
            )
            .await
            .unwrap();
        let token = store
            .change_password(&login.user, "Changed-member-password-42".into())
            .await
            .unwrap()
            .token;
        let user = store.authenticate(token.clone()).await.unwrap();
        let thread = store
            .create_thread(&user, "Original".into())
            .await
            .unwrap()
            .id;
        // The research read goes through the existing typed member Google routes.
        let google = super::super::routes_with_test_transport(
            store.clone(),
            owner,
            transport_for(
                Arc::new(Fake::default()),
                Uuid::new_v4(),
                "a@example.test".into(),
                "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
            ),
        );
        let read = Uuid::new_v4();
        let intent = json!({"thread_id":thread,"request_id":read});
        let started = value(
            request(
                &google,
                owner,
                &token,
                &format!("{GOOGLE}/start"),
                json!({"read":intent}),
            )
            .await,
        )
        .await;
        let url = url::Url::parse(started["authorization_url"].as_str().unwrap()).unwrap();
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let captured = value(
            request(
                &google,
                owner,
                &token,
                "/internal/google/capture",
                json!({"state":state,"code":"synthetic-code"}),
            )
            .await,
        )
        .await;
        let done = value(
            request(
                &google,
                owner,
                &token,
                &format!("{GOOGLE}/complete"),
                json!({"handle":captured["handle"]}),
            )
            .await,
        )
        .await;
        let read_reply = request(
            &google,
            owner,
            &token,
            &format!("{GOOGLE}/inbox"),
            json!({"connection_id":done["connection_id"],"epoch":done["epoch"],"read":intent}),
        )
        .await;
        assert_eq!(read_reply.status(), StatusCode::OK);
        assert_eq!(value(read_reply).await["state"], "completed");
        Env {
            _temp: temp,
            app: routes(store.clone(), owner, writer, None),
            store,
            owner,
            token,
            thread,
            read,
        }
    }
    async fn call(
        e: &Env,
        action: &str,
        body: serde_json::Value,
    ) -> (StatusCode, serde_json::Value) {
        let reply = request(&e.app, e.owner, &e.token, &format!("{BASE}/{action}"), body).await;
        (reply.status(), value(reply).await)
    }
    async fn prepared(e: &Env, id: Uuid, body: &str) -> serde_json::Value {
        let (status, v) = call(
            e,
            "prepare",
            json!({"id":id,"thread_id":e.thread,"read_request_id":e.read,"message_id":"one","body":body}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        v
    }
    /// Existing standard compose consent path, same mailbox as the readonly grant.
    async fn compose_connection(e: &Env) -> moosuri_core::MailConnection {
        let user = e.store.authenticate(e.token.clone()).await.unwrap();
        let pin = moosuri_store::GoogleAppPin {
            revision: None,
            client_id: "synthetic-app".into(),
            redirect_uri: "http://127.0.0.1/callback".into(),
        };
        let start = e
            .store
            .start_mail_compose_oauth_with_app(&user, pin.clone())
            .await
            .unwrap();
        let handle = e
            .store
            .capture_mail_oauth(
                start.state,
                SecretString::new("synthetic-code".into()).unwrap(),
            )
            .await
            .unwrap();
        let exchange = e
            .store
            .claim_mail_oauth_with_app(&user, handle, pin)
            .await
            .unwrap();
        let scopes = moosuri_connectors::gmail::COMPOSE_SCOPES
            .iter()
            .map(|s| s.to_string())
            .collect();
        e.store
            .complete_mail_oauth(
                exchange,
                TokenSet {
                    access_token: SecretString::new("SYNTHETIC-MAIL-ACCESS".into()).unwrap(),
                    refresh_token: Some(
                        SecretString::new("SYNTHETIC-MAIL-REFRESH".into()).unwrap(),
                    ),
                    expires_in: 3600,
                    scopes: Some(scopes),
                },
                "a@example.test".into(),
            )
            .await
            .unwrap()
    }
    async fn previewed(e: &Env) -> serde_json::Value {
        let connection = compose_connection(e).await;
        let work = prepared(e, Uuid::new_v4(), "Thank you for the update.").await;
        let (status, v) = call(
            e,
            "preview",
            json!({"id":work["work"]["id"],"revision":work["work"]["revision"],"connection_id":connection.id,"connection_epoch":connection.epoch}),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(v["delivery"]["state"], "final_preview");
        v
    }
    fn act(v: &serde_json::Value) -> serde_json::Value {
        json!({"id":v["id"],"revision":v["revision"],"action_revision":v["delivery"]["preview"]["action_revision"],"action_digest":v["delivery"]["preview"]["action_digest"]})
    }

    #[tokio::test]
    async fn only_the_original_member_prepares_a_local_draft_from_the_cited_read_and_readonly_never_writes()
     {
        let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
        let w = writer(true, false);
        let e = env(Some(w.clone())).await;
        let path = format!("{BASE}/prepare");
        let body = json!({"id":Uuid::new_v4(),"thread_id":e.thread,"read_request_id":e.read,"message_id":"one","body":"Hello"});
        assert_eq!(
            request(&e.app, e.owner, "other-session", &path, body.clone())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let foreign = VolumeOwner {
            account: Uuid::new_v4(),
            ..e.owner
        };
        assert!(
            !request(&e.app, foreign, &e.token, &path, body)
                .await
                .status()
                .is_success()
        );
        let missing = prepared(&e, Uuid::new_v4(), "").await;
        assert_eq!(missing["outcome"], "needs_details");
        assert_eq!(missing["missing"], json!(["body"]));
        let id = Uuid::new_v4();
        let first = prepared(&e, id, "Hello, I would like to apply.").await;
        let work = &first["work"];
        assert_eq!(first["outcome"], "draft");
        assert_eq!(work["state"], "local_draft");
        assert_eq!(work["source"]["mailbox"], "a@example.test");
        assert_eq!(work["source"]["message_id"], "one");
        assert_eq!(work["draft"]["to"], json!(["sender@example.test"]));
        assert_eq!(work["delivery"]["state"], "needs_connection");
        assert_eq!(work["delivery"]["reason"], "readonly_only");
        assert_eq!(
            prepared(&e, id, "Hello, I would like to apply.").await,
            first
        );
        // Readonly: there is nothing to save or approve, and the writer is never reached.
        let stale =
            json!({"id":id,"revision":1,"action_revision":1,"action_digest":"0".repeat(64)});
        assert_eq!(
            call(&e, "save-draft", stale.clone()).await.0,
            StatusCode::CONFLICT
        );
        assert_eq!(call(&e, "approve", stale).await.0, StatusCode::CONFLICT);
        assert_eq!(w.calls.load(Ordering::SeqCst), 0);
        assert!(
            e.store
                .mail_actions(&e.store.authenticate(e.token.clone()).await.unwrap())
                .await
                .unwrap()
                .is_empty()
        );
        let bytes = serde_json::to_vec(&first).unwrap();
        assert!(
            public_response(&path, &Method::POST, StatusCode::OK, &bytes)
                .unwrap()
                .is_ok()
        );
        let mut forged = first.clone();
        forged["work"]["delivery"]["state"] = json!("delivered");
        assert!(
            public_response(
                &path,
                &Method::POST,
                StatusCode::OK,
                &serde_json::to_vec(&forged).unwrap()
            )
            .unwrap()
            .is_err()
        );
        assert!(
            public_response(
                "/api/v1/personal/google/inbox",
                &Method::POST,
                StatusCode::OK,
                &bytes
            )
            .is_none()
        );
    }

    #[tokio::test]
    async fn standard_writer_runs_once_its_receipt_is_recorded_and_replays_without_a_second_write()
    {
        let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
        let w = writer(true, false);
        let e = env(Some(w.clone())).await;
        let shown = previewed(&e).await;
        assert_eq!(shown["delivery"]["preview"]["threaded"], false);
        let (status, saved) = call(&e, "save-draft", act(&shown)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(saved["delivery"]["state"], "receipt");
        assert_eq!(saved["delivery"]["kind"], "gmail_draft");
        assert_eq!(saved["delivery"]["provider_draft_id"], "draft-1");
        assert_eq!(w.calls.load(Ordering::SeqCst), 1);
        let (_, again) = call(&e, "save-draft", act(&saved)).await;
        assert_eq!(again["delivery"]["provider_draft_id"], "draft-1");
        assert_eq!(w.calls.load(Ordering::SeqCst), 1);
        // The send is only a standard approval; no provider message id is invented.
        let (status, approved) = call(&e, "approve", act(&saved)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(approved["delivery"]["state"], "approved");
        assert!(approved["delivery"].get("provider_message_id").is_none());
    }

    #[tokio::test]
    async fn unready_or_uncertain_writers_are_reported_honestly_and_a_disconnect_withdraws_the_draft()
     {
        let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
        let down = writer(false, false);
        let e = env(Some(down.clone())).await;
        let shown = previewed(&e).await;
        let (status, v) = call(&e, "save-draft", act(&shown)).await;
        assert_eq!(
            (status, &v["code"]),
            (
                StatusCode::SERVICE_UNAVAILABLE,
                &json!("mail_writer_unavailable")
            )
        );
        assert_eq!(down.calls.load(Ordering::SeqCst), 0);
        let (status, listed) = call(&e, "list", json!({"thread_id":e.thread})).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(listed["items"][0]["delivery"]["state"], "final_preview");

        let lossy = writer(true, true);
        let e = env(Some(lossy.clone())).await;
        let shown = previewed(&e).await;
        let (status, unknown) = call(&e, "save-draft", act(&shown)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(unknown["delivery"]["state"], "unknown");
        assert_eq!(unknown["delivery"]["operation"], "gmail_draft");
        assert!(unknown["delivery"].get("provider_draft_id").is_none());
        assert_eq!(lossy.calls.load(Ordering::SeqCst), 1);

        // Disconnecting the readonly source destroys the draft and the unsent approval.
        let user = e.store.authenticate(e.token.clone()).await.unwrap();
        let status = e
            .store
            .member_google_status_local(&user, e.owner)
            .await
            .unwrap();
        e.store
            .disconnect_member_google(&user, e.owner, status.connection_id.unwrap(), status.epoch)
            .await
            .unwrap();
        let (_, gone) = call(&e, "list", json!({"thread_id":e.thread})).await;
        assert_eq!(gone["items"][0]["state"], "withdrawn");
        assert!(gone["items"][0]["draft"].is_null() && gone["items"][0]["source"].is_null());
    }
}
