//! Explicit, bounded continuation of a server-issued latest-INBOX setup card.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResumeInput {}

pub(super) async fn resume(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((thread, run)): Path<(String, String)>,
    Json(_input): Json<ResumeInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    if state.runtime.is_stopping() {
        return Err(StoreError::Unavailable.into());
    }
    // Validate before helper IO as well as atomically at admission. The caller
    // cannot use an unrelated run as a way to probe a helper or submit text.
    state
        .store
        .check_gmail_resume(&user, thread.clone(), run.clone())
        .await?;
    let fingerprint = state
        .mail
        .gws_inbox_resume_fingerprint(&user)
        .await?
        .ok_or(StoreError::Conflict)?;
    let _delivery = state.delivery.read().await;
    let turn = state
        .store
        .resume_gmail(&user, thread, run, fingerprint)
        .await?;
    state.runtime.wake();
    Ok((StatusCode::CREATED, Json(turn)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caller_cannot_replace_original_task_or_binding() {
        assert!(serde_json::from_str::<ResumeInput>("{}").is_ok());
        for input in [
            r#"{"text":"send mail"}"#,
            r#"{"account_id":"another"}"#,
            r#"{"attachments":[]}"#,
            r#"{"request_id":"new"}"#,
        ] {
            assert!(serde_json::from_str::<ResumeInput>(input).is_err());
        }
    }
}
