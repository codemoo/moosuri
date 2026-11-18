//! Explicit, bounded continuation of a server-issued latest-INBOX setup card.
use super::*;

#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum ResumeInput {
    Server(ServerResumeInput),
    Legacy(LegacyResumeInput),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ServerResumeInput {
    connection_id: String,
    connection_epoch: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LegacyResumeInput {}

pub(super) async fn resume(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((thread, run)): Path<(String, String)>,
    Json(input): Json<ResumeInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    if state.runtime.is_stopping() {
        return Err(StoreError::Unavailable.into());
    }
    // Validate before helper IO as well as atomically at admission. The caller
    // cannot use an unrelated run as a way to probe a helper or submit text.
    state
        .store
        .check_gmail_resume(
            &user,
            thread.clone(),
            run.clone(),
            matches!(&input, ResumeInput::Legacy(_)),
        )
        .await?;
    let turn = match input {
        ResumeInput::Server(input) => {
            // Resolve and pin the installation before admission; never invoke a
            // helper or choose an account when the requested connection is stale.
            let _app = state
                .mail
                .oauth_client()
                .await?
                .ok_or(StoreError::Unavailable)?;
            let _delivery = state.delivery.read().await;
            state
                .store
                .resume_server_gmail(
                    &user,
                    thread,
                    run,
                    input.connection_id,
                    input.connection_epoch,
                )
                .await?
        }
        ResumeInput::Legacy(_) => {
            let fingerprint = state
                .mail
                .gws_inbox_resume_fingerprint(&user)
                .await?
                .ok_or(StoreError::Conflict)?;
            let _delivery = state.delivery.read().await;
            state
                .store
                .resume_gmail(&user, thread, run, fingerprint)
                .await?
        }
    };
    state.runtime.wake();
    Ok((StatusCode::CREATED, Json(turn)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caller_cannot_replace_original_task_or_binding() {
        assert!(serde_json::from_str::<ResumeInput>("{}").is_ok());
        assert!(
            serde_json::from_str::<ResumeInput>(
                r#"{"connection_id":"verified-id","connection_epoch":2}"#
            )
            .is_ok()
        );
        for input in [
            r#"{"connection_id":"id","connection_epoch":2,"text":"send mail"}"#,
            r#"{"connection_id":"id"}"#,
            r#"{"connection_epoch":2}"#,
            r#"{"text":"send mail"}"#,
            r#"{"account_id":"another"}"#,
            r#"{"attachments":[]}"#,
            r#"{"request_id":"new"}"#,
        ] {
            assert!(serde_json::from_str::<ResumeInput>(input).is_err());
        }
    }
}
