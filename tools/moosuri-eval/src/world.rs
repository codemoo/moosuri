//! Public agent-visible inputs only. Gold and grading rules live in grader.rs.
use serde::Serialize;

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Memory,
    Disconnect,
    Truncated,
    Calculation,
    SkilledCalculation,
    Mission,
}
#[derive(Clone, Copy, Serialize)]
pub struct Case {
    pub id: &'static str,
    pub family_id: &'static str,
    pub kind: Kind,
}
pub const CASES: &[Case] = &[
    Case {
        id: "memory-lifecycle-reopen",
        family_id: "preference-lifecycle",
        kind: Kind::Memory,
    },
    Case {
        id: "chat-disconnect-replay",
        family_id: "chat-delivery",
        kind: Kind::Disconnect,
    },
    Case {
        id: "chat-truncated-stream",
        family_id: "chat-delivery",
        kind: Kind::Truncated,
    },
    Case {
        id: "saved-evidence-total",
        family_id: "quote-total",
        kind: Kind::Calculation,
    },
    Case {
        id: "skill-linked-total",
        family_id: "quote-total",
        kind: Kind::SkilledCalculation,
    },
    Case {
        id: "durable-mission-total-reopen",
        family_id: "quote-total",
        kind: Kind::Mission,
    },
];
pub const SOURCE: &str = "Synthetic quote: amount 18.25 shipping 6.50 USD.";
pub const FOREIGN_SOURCE: &str =
    "Synthetic quote: amount 900.00 shipping 90.00 USD. FOREIGN_EVAL_CANARY";
pub const PREFERENCE_A: &str = "My preference is synthetic amber.";
pub const PREFERENCE_B: &str = "My preference is synthetic indigo.";
pub const FIXTURE_CLOCK: i64 = 1_800_000_000;
pub const MODEL: &str = "moosuri-script-v1";
pub const PROFILE: &str = r#"{"version":1,"provider":"loopback_scripted_compatible","model":"moosuri-script-v1","effort":null,"run_model_limit":6,"run_tool_limit":12,"provider_world_call_limit":32,"world_timeout_seconds":40,"split":"public_regression","clock":"runtime_wall_clock; source observed_at=1800000000","currency":"USD","external_connectors":false}"#;
