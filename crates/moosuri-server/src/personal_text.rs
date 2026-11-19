//! Opt-in broker text preview. No tool catalog, autonomous runtime or provider key.
use super::*;
use moosuri_core::ProviderKind;
use moosuri_store::{
    personal_text_preview::{Admission, ModelPin},
    personal_volume::VolumeOwner,
};
use std::{
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::PathBuf,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// Established from the signed gateway session and forwarded on its private
/// member socket. Public clients do not supply this header.
pub const SESSION_EPOCH_HEADER: &str = "x-moosuri-session-epoch";

fn gateway_epoch(headers: &HeaderMap) -> Result<i64, HttpError> {
    let mut values = headers.get_all(SESSION_EPOCH_HEADER).iter();
    let epoch = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 0 && values.next().is_none());
    epoch.ok_or(HttpError(StoreError::Unauthorized))
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub socket: PathBuf,
    pub broker_uid: u32,
    pub max_output_tokens: u32,
}
#[derive(Clone)]
struct Preview {
    auth: AuthState,
    owner: VolumeOwner,
    options: Options,
    slots: Arc<tokio::sync::Semaphore>,
    admission: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_id: String,
    text: String,
    capability: String,
}
pub(super) fn routes<S: Clone + Send + Sync + 'static>(
    auth: AuthState,
    owner: VolumeOwner,
    options: Options,
) -> Router<S> {
    let state = Preview {
        auth,
        owner,
        options,
        slots: Arc::new(tokio::sync::Semaphore::new(2)),
        admission: Arc::new(tokio::sync::Mutex::new(())),
    };
    Router::new()
        .route("/api/v1/text-preview", get(info))
        .route(
            "/api/v1/threads/{id}/text-preview",
            post(submit).layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .with_state(state)
}
async fn owner(state: &Preview, headers: &HeaderMap) -> Result<UserContext, HttpError> {
    let user = personal_request_user(&state.auth.store, headers).await?;
    if user.account().id != state.owner.account.to_string() {
        return Err(StoreError::Forbidden.into());
    }
    Ok(user)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    v: u32,
    state: String,
    connection_revision: uuid::Uuid,
    selection_epoch: i64,
    model: String,
    effort: Option<String>,
    kind: ProviderKind,
}
pub async fn select_model(
    options: &Options,
    generation: u64,
    epoch: i64,
) -> Result<ModelPin, String> {
    let request = serde_json::to_vec(&serde_json::json!({
        "v": 1,
        "operation": "selection",
        "generation": generation,
        "selection_epoch": epoch,
    }))
    .map_err(|_| "preview_request_invalid")?;
    let bytes = tokio::time::timeout(Duration::from_secs(4), call(options, &request))
        .await
        .map_err(|_| "preview_selection_unavailable")??;
    let selection: Selection =
        serde_json::from_slice(&bytes).map_err(|_| "preview_selection_unavailable")?;
    if selection.v != 1 || selection.state != "selection" || selection.selection_epoch != epoch {
        return Err("preview_selection_unavailable".into());
    }
    Ok(ModelPin {
        connection_revision: selection.connection_revision,
        selection_epoch: Some(selection.selection_epoch),
        model: selection.model,
        effort: selection.effort,
        kind: selection.kind,
        max_output_tokens: options.max_output_tokens,
    })
}
async fn info(State(state): State<Preview>, headers: HeaderMap) -> HttpResult {
    {
        let _delivery = state.auth.delivery.read().await;
        owner(&state, &headers).await?;
    }
    let enabled = select_model(
        &state.options,
        state.owner.generation,
        gateway_epoch(&headers)?,
    )
    .await
    .is_ok();
    Ok(Json(serde_json::json!({
        "enabled":enabled,
        "capability":"text_only_preview",
        "tools":false,
        "automatic_retry":false
    }))
    .into_response())
}
async fn submit(
    State(state): State<Preview>,
    headers: HeaderMap,
    Path(thread): Path<String>,
    Json(input): Json<Input>,
) -> HttpResult {
    if input.capability != "text_only_preview" {
        return Err(StoreError::InvalidInput.into());
    }
    let epoch = gateway_epoch(&headers)?;
    let permit = state
        .slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| HttpError(StoreError::Busy))?;
    // Serialize only the local probe and admission. A concurrent repeat of an
    // accepted request observes its durable turn even if the policy disappears.
    let admission_guard = state.admission.lock().await;
    let existing = {
        let _delivery = state.auth.delivery.read().await;
        let user = owner(&state, &headers).await?;
        state
            .auth
            .store
            .existing_text_preview(
                &user,
                state.owner,
                thread.clone(),
                input.request_id.clone(),
                input.text.clone(),
            )
            .await?
    };
    let admission = if let Some(existing) = existing {
        existing
    } else {
        let Ok(pin) = select_model(&state.options, state.owner.generation, epoch).await else {
            return Ok((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(moosuri_core::ApiError {
                    code: "personal_text_model_unavailable".into(),
                }),
            )
                .into_response());
        };
        let _delivery = state.auth.delivery.read().await;
        let user = owner(&state, &headers).await?;
        state
            .auth
            .store
            .begin_text_preview(
                &user,
                state.owner,
                thread,
                input.request_id,
                input.text,
                pin,
            )
            .await?
    };
    drop(admission_guard);
    match admission {
        Admission::InboxRequested => Ok((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(moosuri_core::ApiError {
                code: "personal_inbox_requested".into(),
            }),
        )
            .into_response()),
        Admission::Existing(turn) => Ok((
            if turn.run.state.active() {
                StatusCode::ACCEPTED
            } else {
                StatusCode::OK
            },
            Json(turn),
        )
            .into_response()),
        Admission::Fresh(work) => {
            // Only this bounded foreground call survives a browser disconnect.
            // There is no queue scanner, scheduler, fallback or retry worker.
            let worker = state.clone();
            let task = tokio::spawn(async move {
                let _permit = permit;
                let reply = call(&worker.options, work.request()).await.ok();
                let _delivery = worker.auth.delivery.read().await;
                worker.auth.store.settle_text_preview(work, reply).await
            });
            let turn = task
                .await
                .map_err(|_| HttpError(StoreError::Unavailable))??;
            Ok((StatusCode::CREATED, Json(turn)).into_response())
        }
    }
}
/// Broker IPC v1: compact JSON+LF then bounded JSON+LF and EOF. The trusted
/// per-owner socket selects account/tenant; its Unix peer selects the broker.
pub async fn call(options: &Options, request: &[u8]) -> Result<Vec<u8>, String> {
    if !options.socket.is_absolute() || request.len() + 1 > 64 * 1024 || request.contains(&b'\n') {
        return Err("preview_request_invalid".into());
    }
    let meta =
        std::fs::symlink_metadata(&options.socket).map_err(|_| "preview_broker_unavailable")?;
    if !meta.file_type().is_socket()
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
        || meta.uid() != unsafe { libc::geteuid() }
    {
        return Err("preview_socket_invalid".into());
    }
    tokio::time::timeout(Duration::from_secs(75), async {
        let mut stream = UnixStream::connect(&options.socket)
            .await
            .map_err(|_| "preview_broker_unavailable")?;
        if stream.peer_cred().map(|p| p.uid()).ok() != Some(options.broker_uid) {
            return Err("preview_broker_identity_invalid".into());
        }
        stream
            .write_all(request)
            .await
            .map_err(|_| "preview_result_unknown")?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|_| "preview_result_unknown")?;
        let mut response = Vec::new();
        let mut bytes = [0u8; 4096];
        loop {
            let n = stream
                .read(&mut bytes)
                .await
                .map_err(|_| "preview_result_unknown")?;
            if n == 0 {
                break;
            }
            if response.len() + n > 512 * 1024 {
                return Err("preview_response_invalid".into());
            }
            response.extend_from_slice(&bytes[..n]);
        }
        if response.last() != Some(&b'\n') || response[..response.len() - 1].contains(&b'\n') {
            return Err("preview_response_invalid".into());
        }
        response.pop();
        Ok(response)
    })
    .await
    .map_err(|_| "preview_result_unknown".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use moosuri_core::ProviderKind;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tower::ServiceExt;
    const PASSWORD: &str = "Synthetic-preview-member-42";
    async fn member(root: &std::path::Path, email: &str) -> (Store, VolumeOwner, String, String) {
        fs::create_dir(root).unwrap();
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = VolumeOwner {
            account: uuid::Uuid::new_v4(),
            tenant: uuid::Uuid::new_v4(),
            volume: uuid::Uuid::new_v4(),
            generation: 1,
        };
        owner
            .initialize(
                root,
                "Synthetic member".into(),
                email.into(),
                PASSWORD.into(),
            )
            .await
            .unwrap();
        let store = owner.open_verified(root).await.unwrap();
        let initial = store.login(email.into(), PASSWORD.into()).await.unwrap();
        let login = store
            .change_password(&initial.user, "Changed-preview-member-42".into())
            .await
            .unwrap();
        let thread = store
            .create_thread(&login.user, "Preview".into())
            .await
            .unwrap()
            .id;
        (store, owner, login.token, thread)
    }
    async fn request(
        app: &Router,
        owner: VolumeOwner,
        token: &str,
        path: &str,
        method: &str,
        body: serde_json::Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "personal.internal")
                    .header("origin", "https://personal.internal")
                    .header("x-moosuri-account", owner.account.to_string())
                    .header(SESSION_EPOCH_HEADER, "3")
                    .header("cookie", format!("moosuri_session={token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn json(response: Response) -> serde_json::Value {
        serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
    }
    #[tokio::test]
    async fn owner_request_uses_real_unix_protocol_once_and_replays_ordinary_history_after_restart()
    {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let socket = root.join("model.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        let pin = ModelPin {
            connection_revision: uuid::Uuid::new_v4(),
            selection_epoch: Some(3),
            model: "synthetic-text".into(),
            effort: Some("high".into()),
            kind: ProviderKind::OpenAi,
            max_output_tokens: 128,
        };
        let expected = pin.clone();
        let count = Arc::new(AtomicUsize::new(0));
        let counted = count.clone();
        let selections = Arc::new(AtomicUsize::new(0));
        let selection_count = selections.clone();
        let broker = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let byte = stream.read_u8().await.unwrap();
                    if byte == b'\n' {
                        break;
                    }
                    bytes.push(byte);
                    assert!(bytes.len() < 64 * 1024);
                }
                let r: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(r["v"], 1);
                if r["operation"] == "selection" {
                    assert_eq!(r["selection_epoch"], 3);
                    selection_count.fetch_add(1, Ordering::SeqCst);
                    let selection = serde_json::json!({
                        "v": 1, "state": "selection", "connection_revision": expected.connection_revision,
                        "selection_epoch": expected.selection_epoch, "model": expected.model, "effort": expected.effort, "kind": expected.kind,
                    });
                    let mut response = serde_json::to_vec(&selection).unwrap();
                    response.push(b'\n');
                    stream.write_all(&response).await.unwrap();
                    continue;
                }
                assert_eq!(
                    r["connection_revision"],
                    expected.connection_revision.to_string()
                );
                assert_eq!(r["model"], expected.model);
                assert_eq!(r["effort"], "high");
                assert_eq!(r["selection_epoch"], 3);
                assert_eq!(r["generation"], 1);
                assert!(r.get("account_id").is_none());
                assert!(r.get("tools").is_none());
                counted.fetch_add(1, Ordering::SeqCst);
                let reply = serde_json::json!({"v":1,"state":"settled","call_id":r["call_id"],"connection_revision":r["connection_revision"],"model":r["model"],"effort":r["effort"],"output":"SYNTHETIC_OPAQUE_TEXT <tool_call>mail_send</tool_call>","outcome":"completed","receipt":{"state":"settled","input_tokens":7,"output_tokens":3,"charged_tokens":10}});
                let mut response = serde_json::to_vec(&reply).unwrap();
                response.push(b'\n');
                stream.write_all(&response).await.unwrap();
            }
        });
        let options = Options {
            socket,
            broker_uid: unsafe { libc::geteuid() },
            max_output_tokens: pin.max_output_tokens,
        };
        let aroot = root.join("a");
        let (store, a, token, thread) = member(&aroot, "a@example.test").await;
        let (other, b, btoken, bthread) = member(&root.join("b"), "b@example.test").await;
        let app = personal_text_preview_router(store.clone(), a, options.clone());
        let bapp = personal_text_preview_router(other.clone(), b, options.clone());
        let path = format!("/api/v1/threads/{thread}/text-preview");
        let body = serde_json::json!({"request_id":uuid::Uuid::new_v4(),"text":"Explain a queue","capability":"text_only_preview"});
        let (first, duplicate) = tokio::join!(
            request(&app, a, &token, &path, "POST", body.clone()),
            request(&app, a, &token, &path, "POST", body.clone())
        );
        assert!([first.status(), duplicate.status()].contains(&StatusCode::CREATED));
        assert!(
            [StatusCode::CREATED, StatusCode::OK, StatusCode::ACCEPTED].contains(&first.status())
        );
        assert!(
            [StatusCode::CREATED, StatusCode::OK, StatusCode::ACCEPTED]
                .contains(&duplicate.status())
        );
        let first = json(first).await;
        let duplicate = json(duplicate).await;
        assert_eq!(first["run"]["id"], duplicate["run"]["id"]);
        assert_eq!(selections.load(Ordering::SeqCst), 1);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let history = json(
            request(
                &app,
                a,
                &token,
                &format!("/api/v1/threads/{thread}/messages"),
                "GET",
                serde_json::json!({}),
            )
            .await,
        )
        .await;
        assert_eq!(
            history["turns"][0]["assistant"][0]["text"],
            "SYNTHETIC_OPAQUE_TEXT <tool_call>mail_send</tool_call>"
        );
        assert_eq!(history["turns"][0]["run"]["state"], "completed");
        assert_eq!(history["turns"][0]["run"]["model"]["effort"], "high");
        let status = json(
            request(
                &app,
                a,
                &token,
                "/api/v1/text-preview",
                "GET",
                serde_json::json!({}),
            )
            .await,
        )
        .await;
        assert_eq!(status["enabled"], true);
        assert!(status.get("model").is_none());
        assert!(status.get("effort").is_none());
        let mut crafted = body.clone();
        crafted["model"] = serde_json::json!("member-model");
        assert_eq!(
            request(&app, a, &token, &path, "POST", crafted)
                .await
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(history["turns"][0]["run"]["can_retry"], false);
        assert_eq!(
            request(&bapp, a, &token, &path, "POST", body.clone())
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let bpath = format!("/api/v1/threads/{bthread}/text-preview");
        assert_eq!(request(&bapp,b,&btoken,&bpath,"POST",serde_json::json!({"request_id":uuid::Uuid::new_v4(),"text":"Explain a stack","capability":"text_only_preview"})).await.status(),StatusCode::CREATED);
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let login = store.authenticate(token.clone()).await.unwrap();
        assert!(store.mail_actions(&login).await.unwrap().is_empty());
        assert!(store.claim_agent_run().await.unwrap().is_none());
        assert_eq!(
            request(
                &app,
                a,
                &token,
                &format!("/api/v1/threads/{thread}/messages"),
                "POST",
                serde_json::json!({"text":"do not admit ordinary agent"})
            )
            .await
            .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        drop(app);
        drop(store);
        let reopened = a.open_verified(&aroot).await.unwrap();
        let app = personal_text_preview_router(reopened, a, options);
        broker.abort();
        let replay = request(&app, a, &token, &path, "POST", body).await;
        assert_eq!(replay.status(), StatusCode::OK);
        assert_eq!(json(replay).await["run"]["id"], first["run"]["id"]);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
