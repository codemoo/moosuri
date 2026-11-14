//! Numeric server observations. No content, model labels or identifiers.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct RunTimings {
    pub attempt: i32,
    #[ts(type = "number | null")]
    pub queue_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub context_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub first_dispatch_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub model_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub first_final_body_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub terminal_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub tool_ms: Option<u64>,
    pub tool_calls: u32,
    pub measured_tool_calls: u32,
    pub model_calls: u32,
    pub measured_model_calls: u32,
    pub clock_discontinuity: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct LatencySamples {
    pub samples: u32,
    pub missing: u32,
    #[ts(type = "number | null")]
    pub p50_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub p95_ms: Option<u64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct RunTimingAggregate {
    pub runs: u32,
    pub attempts: u32,
    pub unmeasured_runs: u32,
    pub not_dispatched: u32,
    pub truncated: bool,
    pub outcomes: std::collections::BTreeMap<String, u32>,
    pub queue: LatencySamples,
    pub context: LatencySamples,
    pub first_dispatch: LatencySamples,
    pub model: LatencySamples,
    pub tool: LatencySamples,
    pub first_final_body: LatencySamples,
    pub terminal: LatencySamples,
}
