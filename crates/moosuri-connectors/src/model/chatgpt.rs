//! Explicit ChatGPT plan dialect. Authentication and personal ownership are caller obligations.
//! No OAuth login, cookie import, shared provider registration, or capability inference.
use super::protocol::{Protocol, ProtocolError, TextCapabilities, TextRequest, encode_request};
use super::transport::{Catalog, CatalogModel, TransportError};
use serde_json::Value;

/// Ordinary API behavior stays the default. Plan routing must be selected explicitly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResponsesDialect {
    #[default]
    Api,
    ChatGptPlan,
}

pub const TOOL_NAMESPACE: &str = "moosuri";

/// Exact known provider codes only; arbitrary provider diagnostics never enter errors.
#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("subscription_sharing_usage_limit_exceeded")]
    UsageLimitExceeded,
    #[error("subscription_sharing_usage_unavailable")]
    UsageUnavailable,
    #[error("subscription_sharing_user_not_eligible")]
    UserNotEligible,
    #[error("subscription_sharing_unsupported_capability")]
    UnsupportedCapability,
    #[error("subscription_sharing_route_not_supported")]
    RouteNotSupported,
    #[error("subscription_sharing_invalid_user")]
    InvalidUser,
    #[error("chatpass_v2_scope_not_authorized")]
    ScopeNotAuthorized,
    #[error("chatpass_v2_invalid_authorization_context")]
    InvalidAuthorizationContext,
    #[error("subscription_sharing_user_unavailable")]
    UserUnavailable,
}
impl PlanError {
    pub(super) fn from_value(value: &Value) -> Option<Self> {
        let code = value
            .pointer("/response/error/code")
            .or_else(|| value.pointer("/error/code"))
            .or_else(|| value.get("code"))?
            .as_str()?;
        Some(match code {
            "subscription_sharing_usage_limit_exceeded" => Self::UsageLimitExceeded,
            "subscription_sharing_usage_unavailable" => Self::UsageUnavailable,
            "subscription_sharing_user_not_eligible" => Self::UserNotEligible,
            "subscription_sharing_unsupported_capability" => Self::UnsupportedCapability,
            "subscription_sharing_route_not_supported" => Self::RouteNotSupported,
            "subscription_sharing_invalid_user" => Self::InvalidUser,
            "chatpass_v2_scope_not_authorized" => Self::ScopeNotAuthorized,
            "chatpass_v2_invalid_authorization_context" => Self::InvalidAuthorizationContext,
            "subscription_sharing_user_unavailable" => Self::UserUnavailable,
            _ => return None,
        })
    }
}

/// Local request budgets remain validation bounds, not a provider hard output-token cap.
/// SIWC rejects max_output_tokens and explicit system input items.
pub fn encode_text_request(
    request: &TextRequest,
    caps: &TextCapabilities,
) -> Result<Value, ProtocolError> {
    let mut body = encode_request(Protocol::Responses, request, caps)?;
    body.as_object_mut()
        .ok_or(ProtocolError::Protocol)?
        .remove("max_output_tokens");
    for item in body["input"]
        .as_array_mut()
        .ok_or(ProtocolError::Protocol)?
    {
        if item["role"] == "system" {
            item["role"] = Value::String("developer".into());
        }
    }
    if serde_json::to_vec(&body)
        .map_err(|_| ProtocolError::Protocol)?
        .len()
        > 512 * 1024
    {
        return Err(ProtocolError::LimitExceeded);
    }
    Ok(body)
}

pub(super) fn parse_catalog(bytes: &[u8]) -> Result<Catalog, TransportError> {
    if bytes.len() > 1024 * 1024 {
        return Err(TransportError::LimitExceeded);
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| TransportError::InvalidCatalog)?;
    let entries = value
        .get("models")
        .and_then(Value::as_array)
        .ok_or(TransportError::InvalidCatalog)?;
    // Bound inspected entries too. A hidden oversized tail is not trusted metadata.
    if entries.len() > 4096 {
        return Err(TransportError::LimitExceeded);
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut models = Vec::new();
    let mut visible = 0usize;
    for entry in entries {
        let visibility = entry
            .get("visibility")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() <= 64 && !v.chars().any(char::is_control))
            .ok_or(TransportError::InvalidCatalog)?;
        let id = entry
            .get("slug")
            .and_then(Value::as_str)
            .filter(|v| {
                !v.is_empty()
                    && v.len() <= 256
                    && !v.chars().any(|c| c.is_control() || c.is_whitespace())
            })
            .ok_or(TransportError::InvalidCatalog)?;
        let name = entry
            .get("display_name")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty() && v.len() <= 512 && !v.chars().any(char::is_control))
            .ok_or(TransportError::InvalidCatalog)?;
        if !seen.insert(id) {
            return Err(TransportError::InvalidCatalog);
        }
        if visibility == "list" {
            visible += 1;
            if models.len() < 512 {
                models.push(CatalogModel {
                    id: id.into(),
                    name: name.into(),
                });
            }
        }
    }
    Ok(Catalog {
        models,
        truncated: visible > 512,
    })
}

#[cfg(test)]
mod tests;
