//! Provider-reported capacity, distinct from this installation's token usage.
use crate::ModelSource;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuotaScope {
    PersonalSubscription,
    SharedProviderKey,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuotaStatus {
    Known,
    Unsupported,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuotaUnit {
    Usd,
    Requests,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct QuotaMetric {
    pub id: String,
    pub label: String,
    pub unit: QuotaUnit,
    pub used: Option<f64>,
    pub limit: Option<f64>,
    pub remaining: Option<f64>,
    /// Only a timestamp explicitly reported by the provider, in Unix seconds.
    #[ts(type = "number | null")]
    pub reset_at: Option<i64>,
    /// A provider-reported interval is not a known next reset instant.
    pub reset_interval: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ProviderQuota {
    pub source: ModelSource,
    pub provider_id: String,
    #[ts(type = "number")]
    pub provider_revision: i64,
    pub scope: QuotaScope,
    pub status: QuotaStatus,
    #[ts(type = "number | null")]
    pub observed_at: Option<i64>,
    pub metrics: Vec<QuotaMetric>,
    pub evidence_url: String,
    pub message: String,
}
