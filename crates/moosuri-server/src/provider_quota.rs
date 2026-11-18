//! Official, explicitly scoped capacity metadata. Shared key information is
//! administrator-only; personal SIWC never calls undocumented quota endpoints.
use super::*;
use axum::extract::Query;
use moosuri_connectors::model::{CancellationToken, TransportLimits};
use moosuri_core::{ModelSource, ProviderKind, ProviderQuota, QuotaScope, QuotaStatus};

const SIWC: &str =
    "https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference";
const OPENROUTER: &str = "https://openrouter.ai/docs/api_reference/limits";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct QuotaQuery {
    revision: i64,
}
fn report(
    id: String,
    revision: i64,
    personal: bool,
    evidence: &str,
    message: &str,
) -> ProviderQuota {
    ProviderQuota {
        source: if personal {
            ModelSource::PersonalChatGpt
        } else {
            ModelSource::Shared
        },
        provider_id: id,
        provider_revision: revision,
        scope: if personal {
            QuotaScope::PersonalSubscription
        } else {
            QuotaScope::SharedProviderKey
        },
        status: QuotaStatus::Unsupported,
        observed_at: None,
        metrics: vec![],
        evidence_url: evidence.into(),
        message: message.into(),
    }
}
pub(super) async fn personal(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<QuotaQuery>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    // List itself applies full authentication and owner scope, including helper exclusion.
    let binding = s
        .store
        .subscriptions(&u)
        .await?
        .into_iter()
        .find(|v| v.id == id)
        .ok_or(StoreError::NotFound)?;
    if binding.revision != q.revision {
        return Err(StoreError::Conflict.into());
    }
    Ok(Json(report(id, q.revision, true, SIWC,
        "ChatGPT plan remaining allowance and next reset are unknown: the official SIWC interface does not document a quota read endpoint. Check ChatGPT for your plan limits.")).into_response())
}
async fn shared_metadata(
    s: &AppState,
    u: &UserContext,
    id: &str,
    revision: i64,
) -> Result<moosuri_core::ProviderSummary, StoreError> {
    // This method requires live administrator authority before returning any
    // shared provider/key metadata, including unsupported/unavailable states.
    let binding = s
        .store
        .list_providers(u)
        .await?
        .providers
        .into_iter()
        .find(|v| v.id == id)
        .ok_or(StoreError::NotFound)?;
    if binding.revision != revision {
        return Err(StoreError::Conflict);
    }
    Ok(binding)
}
pub(super) async fn shared(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<QuotaQuery>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let provider = shared_metadata(&s, &u, &id, q.revision).await?;
    let (evidence, message) = match provider.config.kind {
        ProviderKind::OpenRouter => (
            OPENROUTER,
            "OpenRouter shared API key capacity, across all users and models. Key credit caps are not the account balance or a model allowance.",
        ),
        ProviderKind::OpenAi => (
            "https://developers.openai.com/api/docs/guides/rate-limits",
            "Shared API key remaining allowance and next reset are unknown. Inference rate-limit headers are request-scoped and are not a subscription or account balance.",
        ),
        ProviderKind::Anthropic => (
            "https://platform.claude.com/docs/en/api/rate-limits",
            "Shared API key remaining allowance and next reset are unknown. Inference rate-limit headers are request-scoped and are not a subscription or account balance.",
        ),
        ProviderKind::Compatible => (
            "https://platform.openai.com/docs/api-reference",
            "This compatible connection has no standard quota endpoint. Remaining allowance and next reset are unknown; check its provider documentation.",
        ),
    };
    let mut result = report(id.clone(), q.revision, false, evidence, message);
    if provider.config.kind != ProviderKind::OpenRouter {
        return Ok(Json(result).into_response());
    }
    result.status = QuotaStatus::Unavailable;
    if !provider.enabled || !provider.has_credential {
        result.message = "OpenRouter key metadata is unavailable while the connection is disabled or has no credential.".into();
        return Ok(Json(result).into_response());
    }
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let binding = s
        .store
        .probe_binding(&u, id.clone(), q.revision, "catalog".into(), None)
        .await?;
    let client = providers::bound_client(&binding)
        .map_err(|_| StoreError::Unavailable)?
        .with_limits(TransportLimits {
            connect: Duration::from_secs(5),
            idle: Duration::from_secs(10),
            total: Duration::from_secs(15),
            response_bytes: 128 * 1024,
        })
        .map_err(|_| StoreError::Unavailable)?;
    let cancellation = CancellationToken::new();
    let operation = client.openrouter_quota(&cancellation);
    tokio::pin!(operation);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let response = loop {
        tokio::select! { biased;
            _ = tick.tick() => {
                let p = shared_metadata(&s, &u, &id, q.revision).await?;
                if !p.enabled { return Err(StoreError::Conflict.into()); }
            },
            r = &mut operation => break r,
        }
    };
    let p = shared_metadata(&s, &u, &id, q.revision).await?;
    if !p.enabled {
        return Err(StoreError::Conflict.into());
    }
    match response {
        Ok(metrics) => {
            result.metrics = metrics;
            result.status = QuotaStatus::Known;
            result.observed_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|v| i64::try_from(v.as_secs()).ok());
        }
        Err(_) => {
            result.message = "OpenRouter key metadata could not be verified. Remaining allowance and next reset are unknown. Try refreshing later.".into();
        }
    }
    Ok(Json(result).into_response())
}
