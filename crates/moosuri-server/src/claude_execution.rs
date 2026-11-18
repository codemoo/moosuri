//! Owner-only official CLI settings. Auth tokens never cross this boundary.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Create {
    label: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    revision: i64,
    model: Option<String>,
}
pub(super) async fn list(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(serde_json::json!({"runtime_configured":s.config.claude.is_some(),"bindings":s.store.claude_bindings(&u).await?})).into_response())
}
pub(super) async fn create(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<Create>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let p = s.runtime.subscription_service().claude_profile().await?;
    Ok(Json(
        s.store
            .create_claude_binding(&u, i.label, p.digest().into(), p.root_id())
            .await?,
    )
    .into_response())
}
pub(super) async fn native(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<Input>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(
        s.runtime
            .subscription_service()
            .claude_native_ref(&u, id, i.revision)
            .await?,
    )
    .into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ModelsInput {
    revision: i64,
    #[serde(default)]
    force: bool,
}
pub(super) async fn models(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<ModelsInput>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    Ok(Json(
        s.runtime
            .subscription_service()
            .claude_models_refresh(&u, id, i.revision, i.force)
            .await?,
    )
    .into_response())
}
pub(super) async fn probe(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<Input>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    Ok(Json(
        s.runtime
            .subscription_service()
            .claude_probe(&u, id, i.revision, i.model.ok_or(StoreError::InvalidInput)?)
            .await?,
    )
    .into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolProbeInput {
    revision: i64,
    model: String,
    effort: Option<String>,
}

pub(super) async fn tool_probe(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<ToolProbeInput>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let result = s
        .runtime
        .subscription_service()
        .claude_tool_probe(&u, id, i.revision, i.model, i.effort)
        .await;
    match result {
        Ok(choice) => Ok(Json(choice).into_response()),
        Err(StoreError::Unavailable) => Ok(failure(
            StatusCode::BAD_GATEWAY,
            "model_tool_probe_incomplete",
        )),
        Err(error) => Err(error.into()),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UsageInput {
    revision: i64,
}
pub(super) async fn usage(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<UsageInput>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    Ok(Json(
        s.runtime
            .subscription_service()
            .claude_usage(&u, id, i.revision)
            .await?,
    )
    .into_response())
}
pub(super) async fn disconnect(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<Input>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    s.runtime
        .subscription_service()
        .claude_disconnect(&u, id, i.revision)
        .await?;
    Ok(
        Json(serde_json::json!({"local_disconnected":true,"remote_revocation_confirmed":false}))
            .into_response(),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Code {
    code: zeroize::Zeroizing<String>,
}
pub(super) async fn login_start(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<Input>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(
        s.runtime
            .subscription_service()
            .claude_login_start(&u, id, i.revision)
            .await?,
    )
    .into_response())
}
pub(super) async fn login_status(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, attempt)): Path<(String, String)>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(
        s.runtime
            .subscription_service()
            .claude_login_status(&u, &id, attempt)
            .await?,
    )
    .into_response())
}
pub(super) async fn login_code(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, attempt)): Path<(String, String)>,
    Json(i): Json<Code>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let code = moosuri_connectors::claude_code::LoginCode::new(i.code)
        .map_err(|_| StoreError::InvalidInput)?;
    Ok(Json(
        s.runtime
            .subscription_service()
            .claude_login_code(&u, &id, attempt, code)
            .await?,
    )
    .into_response())
}
pub(super) async fn login_cancel(
    State(s): State<AppState>,
    h: HeaderMap,
    Path((id, attempt)): Path<(String, String)>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(
        s.runtime
            .subscription_service()
            .claude_login_cancel(&u, &id, attempt)
            .await?,
    )
    .into_response())
}
