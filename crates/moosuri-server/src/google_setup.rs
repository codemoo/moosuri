//! Installation-owned Google OAuth configuration and an owner-scoped profile check.
//! Configuration validation performs no provider request and never returns secrets.
use super::*;
use moosuri_connectors::{
    gmail::{GoogleClientFileError, OAuthClient},
    model::CancellationToken,
};
use moosuri_core::{GoogleConnectionCheck, GoogleSetupStatus, GoogleSetupValidation};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    client_json: Zeroizing<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    expected_revision: i64,
    client_json: Zeroizing<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    connection_id: String,
}
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(status).put(save))
        .route("/validate", post(validate_file))
        .route("/oauth/start", post(start))
        .route("/check", post(check))
        .layer(DefaultBodyLimit::max(128 * 1024))
}
pub(super) fn client_file_code(error: GoogleClientFileError) -> &'static str {
    match error {
        GoogleClientFileError::Invalid => "invalid_google_client_file",
        GoogleClientFileError::RedirectMismatch => "google_redirect_uri_mismatch",
    }
}
async fn view(s: &AppState, user: &UserContext) -> Result<GoogleSetupStatus, StoreError> {
    // Store validates the current administrator inside its writer, including for reads.
    let saved = s.store.google_setup(user).await?;
    let redirect_uri = s.config.google_redirect_uri();
    let revision = saved.as_ref().map(|row| row.revision).unwrap_or(0);
    let (mode, client_id, saved_redirect_uri) = match &s.config.google {
        Some(client) => (
            "operator_file",
            Some(client.client_id.clone()),
            Some(client.redirect_uri.clone()),
        ),
        None => match saved {
            Some(row) => ("managed", Some(row.client_id), Some(row.redirect_uri)),
            None => ("unconfigured", None, None),
        },
    };
    let redirect_matches = saved_redirect_uri.as_ref() == Some(&redirect_uri);
    let configured = redirect_matches && s.mail.configured().await;
    Ok(GoogleSetupStatus {
        revision,
        mode: mode.into(),
        configured,
        client_id,
        redirect_uri,
        saved_redirect_uri,
        redirect_matches,
        personal_connections: s.store.mail_connections(user).await?,
    })
}
async fn status(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(view(&s, &user).await?).into_response())
}
async fn validate_file(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Upload>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    s.store.google_setup(&user).await?;
    let redirect = s.config.google_redirect_uri();
    let client = match OAuthClient::from_web_client_json(&input.client_json, &redirect) {
        Ok(client) => client,
        Err(error) => return Ok(failure(StatusCode::BAD_REQUEST, client_file_code(error))),
    };
    Ok(Json(GoogleSetupValidation {
        client_id: client.client_id,
        redirect_uri: redirect,
        local_valid: true,
    })
    .into_response())
}
async fn save(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Save>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    s.store.google_setup(&user).await?;
    if s.config.google.is_some() {
        return Ok(failure(StatusCode::CONFLICT, "google_managed_by_operator"));
    }
    let client = match OAuthClient::from_web_client_json(
        &input.client_json,
        &s.config.google_redirect_uri(),
    ) {
        Ok(client) => client,
        Err(error) => return Ok(failure(StatusCode::BAD_REQUEST, client_file_code(error))),
    };
    s.store
        .save_google_setup(&user, input.expected_revision, client)
        .await?;
    s.runtime.wake();
    Ok(Json(view(&s, &user).await?).into_response())
}
async fn start(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&s, &headers).await?;
    s.store.google_setup(&user).await?;
    mail::start_for(&s, &user, true).await
}
async fn check(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Check>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    s.store.google_setup(&user).await?;
    if input.connection_id.is_empty() || input.connection_id.len() > 128 {
        return Err(StoreError::InvalidInput.into());
    }
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let cancel = CancellationToken::new();
    let checked_app = s
        .mail
        .oauth_client()
        .await?
        .ok_or(StoreError::Unavailable)?
        .pin;
    let binding = s.mail.binding(&user, &input.connection_id, &cancel).await?;
    let profile = match s
        .mail
        .client()
        .profile(&binding.access_token, &cancel)
        .await
    {
        Ok(profile) => profile,
        Err(_) => {
            return Ok(failure(
                StatusCode::BAD_GATEWAY,
                "google_connection_check_failed",
            ));
        }
    };
    // Only the requesting administrator's own connection may be checked. Recheck
    // both role and personal authority after IO before showing even the address.
    s.store.google_setup(&user).await?;
    let latest = s.store.mail_binding(&user, input.connection_id).await?;
    let latest_app = s
        .mail
        .oauth_client()
        .await?
        .ok_or(StoreError::Conflict)?
        .pin;
    if latest.epoch != binding.epoch || latest_app != checked_app {
        return Err(StoreError::Conflict.into());
    }
    if !profile.email_address.eq_ignore_ascii_case(&binding.email) {
        return Ok(failure(StatusCode::CONFLICT, "google_profile_mismatch"));
    }
    let checked_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::Unavailable)?
        .as_secs();
    Ok(Json(GoogleConnectionCheck {
        email: binding.email,
        checked_at: i64::try_from(checked_at).map_err(|_| StoreError::Unavailable)?,
    })
    .into_response())
}
