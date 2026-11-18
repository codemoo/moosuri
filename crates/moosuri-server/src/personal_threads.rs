//! Conversation lifecycle on the private member socket. No agent or background
//! worker is created here; storage and withdrawal stay in the normal Store.
use super::*;
use axum::extract::Query;
use serde::Serialize;
use std::io::Write;

const MAX_RESPONSE: usize = 32 * 1024 * 1024;

pub(super) fn routes<S: Clone + Send + Sync + 'static>(state: AuthState) -> Router<S> {
    Router::new()
        .route("/api/v1/threads", get(list).post(create))
        .route("/api/v1/threads/{id}", delete(remove))
        .route("/api/v1/threads/{id}/messages", get(history))
        .with_state(state)
}
async fn owner(state: &AuthState, headers: &HeaderMap) -> Result<UserContext, HttpError> {
    let user = state.store.authenticate(cookie_token(headers)?).await?;
    expected_account(headers, &user)?;
    if user.is_chatgpt_helper() {
        return Err(StoreError::Forbidden.into());
    }
    Ok(user)
}
// Bound encoding as well as transport. A valid 20-turn page retains the existing
// DTO and pagination; an oversized result fails without returning a partial page.
struct Limited(Vec<u8>);
impl Write for Limited {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_RESPONSE.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("response_limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encoded(status: StatusCode, value: &impl Serialize) -> HttpResult {
    let mut bytes = Limited(Vec::new());
    serde_json::to_writer(&mut bytes, value).map_err(|_| HttpError(StoreError::Unavailable))?;
    Ok((
        status,
        [(header::CONTENT_TYPE, "application/json")],
        bytes.0,
    )
        .into_response())
}
async fn list(State(state): State<AuthState>, headers: HeaderMap) -> HttpResult {
    let _delivery = state.delivery.read().await;
    let user = owner(&state, &headers).await?;
    encoded(
        StatusCode::OK,
        &ThreadsResponse {
            threads: state.store.list_threads(&user).await?,
        },
    )
}
async fn create(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Json(input): Json<CreateThreadRequest>,
) -> HttpResult {
    let _delivery = state.delivery.read().await;
    let user = owner(&state, &headers).await?;
    let created = state
        .store
        .create_thread_with_request_id(&user, input.title, input.request_id)
        .await?;
    encoded(StatusCode::CREATED, &created)
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    before: Option<i64>,
}
async fn history(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> HttpResult {
    let _delivery = state.delivery.read().await;
    let user = owner(&state, &headers).await?;
    encoded(
        StatusCode::OK,
        &state.store.conversation(&user, id, page.before).await?,
    )
}
async fn remove(
    State(state): State<AuthState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let _delivery = state.delivery.write().await;
    let user = owner(&state, &headers).await?;
    state.store.delete_thread(&user, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    const PASSWORD: &str = "Synthetic-member-password-42";
    async fn request(
        app: &Router,
        method: &str,
        path: &str,
        token: &str,
        id: &str,
        body: serde_json::Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "personal.internal")
                    .header("origin", "https://personal.internal")
                    .header("content-type", "application/json")
                    .header("cookie", format!("moosuri_session={token}"))
                    .header("x-moosuri-account", id)
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn value(response: Response) -> serde_json::Value {
        serde_json::from_slice(&to_bytes(response.into_body(), MAX_RESPONSE).await.unwrap())
            .unwrap()
    }
    #[tokio::test]
    async fn private_lifecycle_preserves_idempotency_pagination_and_owner_withdrawal() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path()).unwrap();
        store
            .bootstrap("A".into(), "a@example.test".into(), PASSWORD.into(), false)
            .await
            .unwrap();
        let a = store
            .login("a@example.test".into(), PASSWORD.into())
            .await
            .unwrap();
        let invite = store
            .create_user(&a.user, "B".into(), "b@example.test".into())
            .await
            .unwrap();
        let temporary = store
            .login("b@example.test".into(), invite.temporary_password)
            .await
            .unwrap();
        let b = store
            .change_password(&temporary.user, PASSWORD.into())
            .await
            .unwrap();
        let app = personal_auth_router(store.clone());
        let key = uuid::Uuid::new_v4().to_string();
        let body = serde_json::json!({"title":"A private history","request_id":key});
        let created = request(
            &app,
            "POST",
            "/api/v1/threads",
            &a.token,
            &a.user.account().id,
            body.clone(),
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let created = value(created).await;
        let replay = value(
            request(
                &app,
                "POST",
                "/api/v1/threads",
                &a.token,
                &a.user.account().id,
                body,
            )
            .await,
        )
        .await;
        assert_eq!(created["id"], replay["id"]);
        let id = created["id"].as_str().unwrap();
        // Seed settled synthetic history through normal Store admission, without
        // starting a model worker, to exercise the v1 20-turn cursor contract.
        use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
        let provider = store
            .create_provider(
                &a.user,
                moosuri_store::ProviderChange {
                    name: "Synthetic".into(),
                    config: ProviderConfig {
                        kind: ProviderKind::Compatible,
                        base_url: Some("http://127.0.0.1:11434/v1".into()),
                        allowed_ips: vec!["127.0.0.1".into()],
                        router_providers: vec![],
                        zero_data_retention: false,
                    },
                    enabled: true,
                    credential: moosuri_store::CredentialChange::Clear,
                },
            )
            .await
            .unwrap();
        store
            .record_model_probe(&a.user, provider.id.clone(), 1, "synthetic".into(), None)
            .await
            .unwrap();
        store
            .set_default_model(
                &a.user,
                Some(ModelSelection {
                    source: Default::default(),
                    provider_id: provider.id,
                    provider_revision: 1,
                    model: "synthetic".into(),
                    effort: None,
                }),
            )
            .await
            .unwrap();
        for index in 0..25 {
            let turn = store
                .submit_turn(
                    &a.user,
                    id.into(),
                    uuid::Uuid::new_v4().to_string(),
                    format!("Synthetic private history {index}"),
                )
                .await
                .unwrap();
            store
                .cancel_run(&a.user, turn.run.id, turn.run.attempt)
                .await
                .unwrap();
        }
        let history = format!("/api/v1/threads/{id}/messages");
        let page = value(
            request(
                &app,
                "GET",
                &history,
                &a.token,
                &a.user.account().id,
                serde_json::json!({}),
            )
            .await,
        )
        .await;
        assert_eq!(page["thread_id"], id);
        assert_eq!(page["turns"].as_array().unwrap().len(), 20);
        let before = page["next_before"].as_i64().unwrap();
        let earlier = value(
            request(
                &app,
                "GET",
                &format!("{history}?before={before}"),
                &a.token,
                &a.user.account().id,
                serde_json::json!({}),
            )
            .await,
        )
        .await;
        assert_eq!(earlier["turns"].as_array().unwrap().len(), 5);
        assert!(earlier["next_before"].is_null());
        for method in ["GET", "DELETE"] {
            let path = if method == "GET" {
                history.clone()
            } else {
                format!("/api/v1/threads/{id}")
            };
            assert_eq!(
                request(
                    &app,
                    method,
                    &path,
                    &b.token,
                    &b.user.account().id,
                    serde_json::json!({})
                )
                .await
                .status(),
                StatusCode::NOT_FOUND
            );
        }
        assert_eq!(
            request(
                &app,
                "GET",
                &history,
                &a.token,
                &b.user.account().id,
                serde_json::json!({})
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &app,
                "GET",
                &format!("{history}?before=0"),
                &a.token,
                &a.user.account().id,
                serde_json::json!({})
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        let other = value(
            request(
                &app,
                "GET",
                "/api/v1/threads",
                &b.token,
                &b.user.account().id,
                serde_json::json!({}),
            )
            .await,
        )
        .await;
        assert_eq!(other["threads"], serde_json::json!([]));
        assert_eq!(
            request(
                &app,
                "DELETE",
                &format!("/api/v1/threads/{id}"),
                &a.token,
                &a.user.account().id,
                serde_json::json!({})
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            request(
                &app,
                "GET",
                &history,
                &a.token,
                &a.user.account().id,
                serde_json::json!({})
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(
                &app,
                "POST",
                "/api/v1/threads",
                &a.token,
                &a.user.account().id,
                serde_json::json!({"title":"A private history","request_id":key})
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(
                &app,
                "POST",
                &history,
                &a.token,
                &a.user.account().id,
                serde_json::json!({"text":"must not execute"})
            )
            .await
            .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }
}
