//! Two common synthetic outcomes, never a private-source export or model score.
mod capsule;
pub(crate) mod fixture;
mod native;
pub(crate) mod oracle;
mod rust_subject;
use crate::{EvalResult, baseline, digest};
use serde::Serialize;
use std::path::Path;

pub(crate) const MEMORY: &str = "My synthetic archive preference is violet.";
pub(crate) const MEMORY_QUERY: &str = "What is my synthetic archive preference?";
pub(crate) const CALCULATION: &str =
    "Add 18.25 USD and 6.50 USD from this message. Give the total in USD.";
const PROFILE: &str = "paired-script-v1;two-worlds;wall-clock;no-external-connectors;6-model/12-tool-calls;scripted-not-inference";
#[derive(Serialize)]
pub struct Row {
    case_id: &'static str,
    status: &'static str,
    error: Option<&'static str>,
    legacy_error: Option<&'static str>,
    rust_error: Option<&'static str>,
    legacy_attempted: bool,
    legacy_executed: bool,
    rust_attempted: bool,
    rust_executed: bool,
    legacy_observation_returned: bool,
    rust_observation_returned: bool,
    checks: Vec<oracle::Check>,
    legacy_state_sha256: Option<String>,
    rust_state_sha256: Option<String>,
}
impl Row {
    fn finish(
        &mut self,
        memory: bool,
        old: EvalResult<oracle::Observation>,
        current: EvalResult<oracle::Observation>,
        old_admitted: bool,
        rust_admitted: bool,
    ) {
        self.legacy_executed = old_admitted;
        self.rust_executed = rust_admitted;
        self.legacy_observation_returned = old.is_ok();
        self.rust_observation_returned = current.is_ok();
        self.legacy_error = old.as_ref().err().copied();
        self.rust_error = current.as_ref().err().copied();
        self.status = if old_admitted || rust_admitted {
            "fail"
        } else {
            "not_run"
        };
        match old.and_then(|old| current.map(|new| (old, new))) {
            Ok((old, new)) => {
                self.legacy_state_sha256 = Some(digest(&serde_json::to_vec(&old).unwrap()));
                self.rust_state_sha256 = Some(digest(&serde_json::to_vec(&new).unwrap()));
                self.checks = oracle::grade(memory, &old, &new);
                if old_admitted && rust_admitted && self.checks.iter().all(|c| c.passed) {
                    self.status = "pass";
                }
            }
            Err(e) => self.error = Some(e),
        }
    }
}
#[derive(Serialize)]
pub struct Report {
    format_version: u32,
    evidence_class: &'static str,
    source_sha256: &'static str,
    legacy_source_sha256: Option<String>,
    profile_sha256: String,
    profile: serde_json::Value,
    fixture_sha256: String,
    grader_sha256: String,
    planned: usize,
    executed: usize,
    pass: usize,
    fail: usize,
    not_run: usize,
    rows: Vec<Row>,
    isolation_verified: bool,
    post_run_capsule_pin: bool,
    validation_checks: Vec<oracle::Check>,
    real_model: &'static str,
    non_inferiority: &'static str,
    pub success: bool,
}
/// Preparation is separate from sandboxed execution. Only pinned public package
/// downloads are allowed here; this never imports the old subject.
pub async fn prepare(source: &Path, capsule: &Path, uv: &Path) -> EvalResult<()> {
    baseline::copy_capsule(source, capsule)?;
    capsule::record_copy(capsule)?;
    native::prepare(capsule, uv).await?;
    if !baseline::preflight(capsule).success {
        return Err("capsule_post_install_pin");
    }
    capsule::record_prepared(capsule)
}
#[derive(Serialize)]
pub struct CapsuleCheck {
    pub success: bool,
    error: Option<&'static str>,
    source_files: usize,
    subject_execution: &'static str,
}
pub fn check(capsule: &Path) -> CapsuleCheck {
    match capsule::validate(capsule) {
        Ok(pin) => CapsuleCheck {
            success: true,
            error: None,
            source_files: pin.verified_files,
            subject_execution: "not_run",
        },
        Err(e) => CapsuleCheck {
            success: false,
            error: Some(e),
            source_files: 0,
            subject_execution: "not_run",
        },
    }
}
pub async fn run(capsule: &Path) -> Report {
    let validated = capsule::validate(capsule);
    let mut report = Report {
        format_version: 1,
        evidence_class: "two_world_deterministic_paired_execution",
        source_sha256: env!("MOOSURI_EVAL_SOURCE_SHA256"),
        legacy_source_sha256: validated
            .as_ref()
            .ok()
            .and_then(|p| p.baseline_source_sha256.clone()),
        profile_sha256: digest(PROFILE.as_bytes()),
        profile: serde_json::json!({"kind":"deterministic_protocol_adapters","legacy_transport":"responses","legacy_model_label":"gpt-6.1-terra","legacy_effort":"none","rust_transport":"compatible_chat","rust_model_label":"moosuri-script-v1","rust_effort":null,"clock":"wall_time;timeless_worlds","source_files":149,"external_connectors":false,"real_model_inference":false,"os_isolation":"macos_seatbelt;private_pg16_unix_socket;fixed_loopback_only"}),
        fixture_sha256: digest(
            concat!(include_str!("paired/fixture.rs"), include_str!("paired.rs")).as_bytes(),
        ),
        grader_sha256: digest(include_str!("paired/oracle.rs").as_bytes()),
        planned: 2,
        executed: 0,
        pass: 0,
        fail: 0,
        not_run: 2,
        rows: ["memory-read-graceful-reopen", "current-input-arithmetic"]
            .map(|case_id| Row {
                case_id,
                status: "not_run",
                error: None,
                legacy_error: None,
                rust_error: None,
                legacy_attempted: false,
                rust_attempted: false,
                legacy_executed: false,
                rust_executed: false,
                legacy_observation_returned: false,
                rust_observation_returned: false,
                checks: vec![],
                legacy_state_sha256: None,
                rust_state_sha256: None,
            })
            .into(),
        isolation_verified: false,
        post_run_capsule_pin: false,
        validation_checks: oracle::mutations(),
        real_model: "not_run",
        non_inferiority: "not_run",
        success: false,
    };
    if let Err(error) = validated {
        for row in &mut report.rows {
            row.error = Some(error);
        }
        return report;
    }
    report.isolation_verified = true;
    for (index, row) in report.rows.iter_mut().enumerate() {
        let mut legacy = match native::Legacy::start(capsule).await {
            Ok(subject) => subject,
            Err(e) => {
                row.error = Some(e);
                report.isolation_verified = false;
                continue;
            }
        };
        row.legacy_attempted = true;
        let old = if index == 0 {
            legacy.memory().await
        } else {
            legacy.calculation().await
        };
        let admitted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        row.rust_attempted = true;
        let current = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            rust_subject::execute(index == 0, admitted.clone()),
        )
        .await
        .unwrap_or(Err("paired_rust_world_deadline"));
        row.finish(
            index == 0,
            old,
            current,
            legacy.has_executed(),
            admitted.load(std::sync::atomic::Ordering::SeqCst),
        );
        legacy.shutdown().await;
    }
    report.pass = report.rows.iter().filter(|r| r.status == "pass").count();
    report.fail = report.rows.iter().filter(|r| r.status == "fail").count();
    report.not_run = report.rows.iter().filter(|r| r.status == "not_run").count();
    report.executed = report.pass + report.fail;
    report.post_run_capsule_pin = capsule::validate(capsule).is_ok();
    report.success = report.pass == 2
        && report.post_run_capsule_pin
        && report.validation_checks.iter().all(|c| c.passed);
    report
}
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn paired_admitted_failure_is_executed_not_not_run() {
        use super::*;
        let admitted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = rust_subject::execute(false, admitted.clone()).await;
        assert!(observed.is_ok());
        assert!(admitted.load(std::sync::atomic::Ordering::SeqCst));
        // An independent observer can fail after a real admitted production run.
        // Its failure must not erase that execution or enter the not-run count.
        let failed = observed.and(Err("synthetic_observer_failure"));
        let mut row = Row {
            case_id: "failure-control",
            status: "not_run",
            error: None,
            legacy_error: None,
            rust_error: None,
            legacy_attempted: false,
            legacy_executed: false,
            rust_attempted: true,
            rust_executed: false,
            legacy_observation_returned: false,
            rust_observation_returned: false,
            checks: vec![],
            legacy_state_sha256: None,
            rust_state_sha256: None,
        };
        row.finish(
            false,
            Err("legacy_not_started"),
            failed,
            false,
            admitted.load(std::sync::atomic::Ordering::SeqCst),
        );
        assert_eq!(row.status, "fail");
        assert!(row.rust_executed);
        assert!(!row.rust_observation_returned);
        assert_eq!(row.rust_error, Some("synthetic_observer_failure"));
        let counts = (
            usize::from(row.status != "not_run"),
            usize::from(row.status == "fail"),
            usize::from(row.status == "not_run"),
        );
        assert_eq!(counts, (1, 1, 0));
    }
}
