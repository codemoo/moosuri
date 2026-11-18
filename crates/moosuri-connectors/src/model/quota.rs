//! Allowlisted official OpenRouter key metadata. Never returns key labels/IDs,
//! guardrail IDs, arbitrary provider text, or inferred model-specific capacity.
use super::TransportError;
use moosuri_core::{QuotaMetric, QuotaUnit};
use serde::Deserialize;

#[derive(Deserialize)]
struct Response {
    data: Key,
}
#[derive(Deserialize)]
struct Key {
    limit: Option<f64>,
    limit_remaining: Option<f64>,
    limit_reset: Option<String>,
    usage: Option<f64>,
    usage_daily: Option<f64>,
    usage_weekly: Option<f64>,
    usage_monthly: Option<f64>,
    #[serde(default)]
    budgets: Vec<Budget>,
    free_model_daily_requests: Option<Requests>,
}
#[derive(Deserialize)]
struct Budget {
    scope: String,
    limit_usd: f64,
    spend_usd: f64,
    remaining_usd: f64,
    reset_interval: Option<String>,
    resets_at: Option<String>,
}
#[derive(Deserialize)]
struct Requests {
    used: f64,
    limit: f64,
    remaining: f64,
}
fn invalid() -> TransportError {
    TransportError::InvalidCatalog
}
fn positive(v: Option<f64>) -> Result<Option<f64>, TransportError> {
    if v.is_some_and(|v| !v.is_finite() || v < 0.) {
        return Err(invalid());
    }
    Ok(v)
}
fn finite(v: Option<f64>) -> Result<Option<f64>, TransportError> {
    if v.is_some_and(|v| !v.is_finite()) {
        return Err(invalid());
    }
    Ok(v)
}
fn interval(v: Option<String>) -> Option<String> {
    v.filter(|v| matches!(v.as_str(), "daily" | "weekly" | "monthly"))
}
pub(super) fn parse_openrouter(bytes: &[u8]) -> Result<Vec<QuotaMetric>, TransportError> {
    let key = serde_json::from_slice::<Response>(bytes)
        .map_err(|_| invalid())?
        .data;
    if key.budgets.len() > 32 {
        return Err(invalid());
    }
    let mut metrics = vec![QuotaMetric {
        id: "key_credit_limit".into(),
        label: "API key credit cap (all models)".into(),
        unit: QuotaUnit::Usd,
        used: None,
        limit: positive(key.limit)?,
        remaining: finite(key.limit_remaining)?,
        reset_at: None,
        reset_interval: interval(key.limit_reset),
    }];
    for (id, label, value) in [
        ("usage_all_time", "API key spend (all time)", key.usage),
        (
            "usage_daily",
            "API key spend (current UTC day)",
            key.usage_daily,
        ),
        (
            "usage_weekly",
            "API key spend (current UTC week)",
            key.usage_weekly,
        ),
        (
            "usage_monthly",
            "API key spend (current UTC month)",
            key.usage_monthly,
        ),
    ] {
        if let Some(value) = positive(value)? {
            metrics.push(QuotaMetric {
                id: id.into(),
                label: label.into(),
                unit: QuotaUnit::Usd,
                used: Some(value),
                limit: None,
                remaining: None,
                reset_at: None,
                reset_interval: None,
            });
        }
    }
    for (i, budget) in key.budgets.into_iter().enumerate() {
        let label = match budget.scope.as_str() {
            "workspace_default_member" => "Workspace default member budget",
            "workspace_default_key" => "Workspace default key budget",
            "member" => "Member budget",
            "api_key" => "API key budget",
            "workspace" => "Workspace budget",
            "key_limit" => "API key spending limit",
            _ => continue,
        };
        let reset_at = budget
            .resets_at
            .map(|v| {
                chrono::DateTime::parse_from_rfc3339(&v)
                    .map(|v| v.timestamp())
                    .map_err(|_| invalid())
            })
            .transpose()?;
        metrics.push(QuotaMetric {
            id: format!("budget_{i}"),
            label: label.into(),
            unit: QuotaUnit::Usd,
            used: positive(Some(budget.spend_usd))?,
            limit: positive(Some(budget.limit_usd))?,
            remaining: finite(Some(budget.remaining_usd))?,
            reset_at,
            reset_interval: interval(budget.reset_interval),
        });
    }
    if let Some(requests) = key.free_model_daily_requests {
        for v in [requests.used, requests.limit, requests.remaining] {
            if v.fract() != 0. {
                return Err(invalid());
            }
            positive(Some(v))?;
        }
        metrics.push(QuotaMetric {
            id: "free_model_daily_requests".into(),
            label: "Free-model requests (current UTC day, all free models)".into(),
            unit: QuotaUnit::Requests,
            used: Some(requests.used),
            limit: Some(requests.limit),
            remaining: Some(requests.remaining),
            reset_at: None,
            reset_interval: Some("daily".into()),
        });
    }
    // An empty object cannot establish current capacity.
    if metrics
        .iter()
        .all(|v| v.used.is_none() && v.limit.is_none() && v.remaining.is_none())
    {
        return Err(invalid());
    }
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_values_preserve_scope_and_only_explicit_reset() {
        let metrics = parse_openrouter(br#"{"data":{"label":"SECRET_KEY_NAME","limit":5,"limit_remaining":2,"limit_reset":"daily","usage":20,"budgets":[{"scope":"workspace","guardrail_id":"SECRET_ID","limit_usd":10,"spend_usd":11,"remaining_usd":-1,"reset_interval":"monthly","resets_at":"2026-12-01T00:00:00Z"}],"free_model_daily_requests":{"used":2,"limit":50,"remaining":48}}}"#).unwrap();
        assert_eq!(metrics[0].remaining, Some(2.));
        assert_eq!(metrics[0].used, None); // all-time usage is not current-window spend
        assert_eq!(metrics[0].reset_at, None); // interval alone is not next reset
        assert_eq!(metrics[2].remaining, Some(-1.));
        assert_eq!(metrics[2].reset_at, Some(1796083200));
        assert_eq!(metrics[3].reset_at, None);
        let json = serde_json::to_string(&metrics).unwrap();
        assert!(!json.contains("SECRET"));
    }
    #[test]
    fn missing_or_invalid_metadata_is_unknown_never_zero() {
        for body in [
            r#"{"data":{}}"#,
            r#"{"data":{"usage":-2}}"#,
            r#"{"data":{"limit_remaining":"2"}}"#,
            r#"{"data":{"free_model_daily_requests":{"used":0.5,"limit":50,"remaining":49.5}}}"#,
            r#"{"data":{"budgets":[{"scope":"api_key","limit_usd":5,"spend_usd":1,"remaining_usd":4,"resets_at":"tomorrow"}]}}"#,
        ] {
            assert!(parse_openrouter(body.as_bytes()).is_err(), "{body}");
        }
        let metrics =
            parse_openrouter(br#"{"data":{"usage":3,"limit":null,"limit_remaining":null}}"#)
                .unwrap();
        assert_eq!(metrics[0].limit, None);
        assert_eq!(metrics[0].remaining, None);
    }
}
