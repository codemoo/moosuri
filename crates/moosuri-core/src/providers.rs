//! Public model configuration. Credentials never appear in these response DTOs.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    OpenAi,
    Anthropic,
    OpenRouter,
    Compatible,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub base_url: Option<String>,
    pub allowed_ips: Vec<String>,
    pub router_providers: Vec<String>,
    pub zero_data_retention: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ModelSelection {
    pub provider_id: String,
    #[ts(type = "number")]
    pub provider_revision: i64,
    pub model: String,
    pub effort: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ModelChoice {
    pub provider_id: String,
    #[ts(type = "number")]
    pub provider_revision: i64,
    pub provider_name: String,
    pub kind: ProviderKind,
    pub model: String,
    pub effort: Option<String>,
    #[ts(type = "number")]
    pub verified_at: i64,
    /// Successful bounded tool round trip for this exact binding, separate from text.
    #[serde(default)]
    pub function_tools: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub config: ProviderConfig,
    pub enabled: bool,
    pub has_credential: bool,
    pub choices: Vec<ModelChoice>,
    pub is_default: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ProvidersResponse {
    pub providers: Vec<ProviderSummary>,
    pub default_selection: Option<ModelSelection>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ModelSettings {
    pub selection: Option<ModelSelection>,
    pub effective: Option<ModelChoice>,
    pub choices: Vec<ModelChoice>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ModelCatalogEntry {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ModelCatalog {
    pub models: Vec<ModelCatalogEntry>,
    pub truncated: bool,
}
