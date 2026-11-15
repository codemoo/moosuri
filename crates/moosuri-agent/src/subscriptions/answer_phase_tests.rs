//! Real personal Responses streams must distinguish progress from an answer.
use super::*;
use moosuri_core::MessagePhase;

pub(super) fn wire(mode: usize) -> String {
    let messages = match mode {
        1 => vec![(
            Some("commentary"),
            Some("commentary"),
            "Checking the fixture.",
        )],
        2 => vec![
            (
                Some("commentary"),
                Some("commentary"),
                "Checking the fixture.",
            ),
            (
                Some("final_answer"),
                Some("final_answer"),
                "The answer is 42.",
            ),
        ],
        3 => vec![(None, None, "The answer is 42.")],
        4 => vec![(None, Some("commentary"), "Checking the fixture.")],
        5 => vec![(None, Some("final_answer"), "The answer is 42.")],
        _ => unreachable!(),
    };
    let mut frames = vec![
        json!({"type":"response.created","response":{"id":"phase-response","model":"fixture-model"}}),
    ];
    let mut output = vec![];
    for (index, (initial_phase, final_phase, text)) in messages.into_iter().enumerate() {
        let id = format!("phase-message-{index}");
        let mut added = json!({"type":"message","id":id,"role":"assistant","content":[]});
        let mut done = json!({"type":"message","id":id,"role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]});
        if let Some(phase) = initial_phase {
            added["phase"] = json!(phase);
        }
        if let Some(phase) = final_phase {
            done["phase"] = json!(phase);
        }
        frames.extend([
            json!({"type":"response.output_item.added","output_index":index,"item":added}),
            json!({"type":"response.content_part.added","item_id":id,"output_index":index,"content_index":0,"part":{"type":"output_text","text":""}}),
            json!({"type":"response.output_text.delta","item_id":id,"output_index":index,"content_index":0,"delta":text}),
            json!({"type":"response.output_text.done","item_id":id,"output_index":index,"content_index":0,"text":text}),
            json!({"type":"response.content_part.done","item_id":id,"output_index":index,"content_index":0,"part":{"type":"output_text","text":text,"annotations":[]}}),
            json!({"type":"response.output_item.done","output_index":index,"item":done}),
        ]);
        output.push(done);
    }
    frames.push(json!({"type":"response.completed","response":{"id":"phase-response","model":"fixture-model","status":"completed","output":output,"usage":{"input_tokens":8,"output_tokens":2}}}));
    frames
        .iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect()
}

#[tokio::test]
async fn saved_personal_terminal_requires_answer_and_honors_late_phase_metadata() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let (_dir, store, u, f, service, _login, _choice) = foreground_tools_tests::configured().await;
    for mode in 1..=5 {
        f.state.answer_phase.store(mode, Ordering::SeqCst);
        let done =
            foreground_tools_tests::run(&store, &u, &service, "What is the fixture answer?").await;
        assert!(
            !done.assistant.is_empty(),
            "partial observations must survive: {mode}"
        );
        if matches!(mode, 1 | 4) {
            assert_eq!(done.run.state, RunState::Incomplete, "{mode}");
            assert_eq!(done.run.error.as_deref(), Some("protocol_failed"), "{mode}");
            assert_eq!(
                done.assistant[0].phase,
                Some(MessagePhase::Commentary),
                "{mode}"
            );
            assert_eq!(done.assistant[0].text, "Checking the fixture.", "{mode}");
        } else {
            assert_eq!(done.run.state, RunState::Completed, "{mode}");
            assert!(done.run.error.is_none(), "{mode}");
            let answer = done.assistant.last().unwrap();
            assert_eq!(answer.text, "The answer is 42.", "{mode}");
            assert_eq!(
                answer.phase,
                if mode == 3 {
                    None
                } else {
                    Some(MessagePhase::FinalAnswer)
                },
                "{mode}"
            );
        }
    }
}
