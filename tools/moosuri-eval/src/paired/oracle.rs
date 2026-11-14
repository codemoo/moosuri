//! Hidden from both scripted providers and subjects. Compare observed state.
use serde::Serialize;
#[derive(Default, Serialize)]
pub(crate) struct Observation {
    pub terminal: bool,
    pub answers: Vec<String>,
    pub memory_text: String,
    pub memory_revision: i64,
    pub read_revisions: Vec<i64>,
    pub memory_count: i64,
    pub model_calls: usize,
}
#[derive(Serialize)]
pub struct Check {
    pub code: &'static str,
    pub passed: bool,
}
fn check(code: &'static str, passed: bool) -> Check {
    Check { code, passed }
}
pub(super) fn grade(memory: bool, a: &Observation, b: &Observation) -> Vec<Check> {
    let mut checks = vec![
        check("both_terminal", a.terminal && b.terminal),
        check(
            "bounded_actual_model_calls",
            [a, b]
                .iter()
                .all(|o| o.model_calls > 0 && o.model_calls <= 12),
        ),
    ];
    if memory {
        checks.extend([
            check(
                "same_initial_and_reopened_memory_state",
                [a, b].iter().all(|o| {
                    o.memory_text == "My synthetic archive preference is violet."
                        && o.memory_revision == 1
                        && o.memory_count == 1
                }),
            ),
            check(
                "both_reads_have_current_persisted_evidence",
                [a, b].iter().all(|o| o.read_revisions == [1, 1]),
            ),
            check(
                "both_answers_use_observed_memory",
                [a, b].iter().all(|o| {
                    o.answers.len() == 2
                        && o.answers
                            .iter()
                            .all(|s| s.contains("violet") && !s.contains("amber"))
                }),
            ),
        ]);
    } else {
        // Independent scaled-integer cents, never the fixture's calculation helper.
        let cents = 1825_i64 + 650_i64;
        let expected = format!("{}.{:02} USD", cents / 100, cents % 100);
        checks.extend([
            check(
                "independent_exact_input_total",
                [a, b]
                    .iter()
                    .all(|o| o.answers.len() == 1 && o.answers[0].trim() == expected),
            ),
            check(
                "arithmetic_has_no_memory_effect",
                a.memory_count == 0 && b.memory_count == 0,
            ),
        ]);
    }
    checks
}
pub(super) fn mutations() -> Vec<Check> {
    let correct = Observation {
        terminal: true,
        answers: vec!["24.75 USD".into()],
        model_calls: 1,
        ..Default::default()
    };
    let wrong = Observation {
        terminal: true,
        answers: vec!["25.75 USD".into()],
        model_calls: 1,
        ..Default::default()
    };
    let memory = Observation {
        terminal: true,
        answers: vec!["violet".into(); 2],
        memory_text: "My synthetic archive preference is violet.".into(),
        memory_revision: 1,
        read_revisions: vec![1, 1],
        memory_count: 1,
        model_calls: 2,
    };
    let missing = Observation {
        read_revisions: vec![],
        ..Observation {
            terminal: true,
            answers: vec!["violet".into(); 2],
            memory_text: memory.memory_text.clone(),
            memory_revision: 1,
            memory_count: 1,
            model_calls: 2,
            ..Default::default()
        }
    };
    vec![
        check(
            "positive_arithmetic_control",
            grade(false, &correct, &correct).iter().all(|c| c.passed),
        ),
        check(
            "wrong_arithmetic_rejected",
            grade(false, &correct, &wrong).iter().any(|c| !c.passed),
        ),
        check(
            "positive_memory_control",
            grade(true, &memory, &memory).iter().all(|c| c.passed),
        ),
        check(
            "success_prose_without_read_evidence_rejected",
            grade(true, &memory, &missing).iter().any(|c| !c.passed),
        ),
    ]
}
#[cfg(test)]
mod tests {
    #[test]
    fn paired_oracle_rejects_false_success() {
        assert!(super::mutations().iter().all(|c| c.passed));
    }
}
