use super::*;
use crate::model::protocol::{FinishOutcome, ModelEvent, StreamDecoder, TextMessage, TextRole};
use serde_json::json;

#[test]
fn plan_text_projects_developer_order_and_omits_forbidden_api_fields() {
    let request = TextRequest {
        model: "fixture-model".into(),
        messages: vec![
            TextMessage {
                role: TextRole::System,
                text: "trusted instruction".into(),
                phase: None,
            },
            TextMessage {
                role: TextRole::User,
                text: "synthetic text".into(),
                phase: None,
            },
            TextMessage {
                role: TextRole::System,
                text: "later instruction".into(),
                phase: None,
            },
        ],
        max_output_tokens: 128,
        effort: Some("high".into()),
    };
    let caps = TextCapabilities {
        efforts: vec!["high".into()],
    };
    let body = encode_text_request(&request, &caps).unwrap();
    assert_eq!(
        body,
        json!({"model":"fixture-model","input":[{"role":"developer","content":"trusted instruction"},{"role":"user","content":"synthetic text"},{"role":"developer","content":"later instruction"}],"stream":true,"store":false,"reasoning":{"effort":"high"}})
    );
    let api = encode_request(Protocol::Responses, &request, &caps).unwrap();
    assert_eq!(api["max_output_tokens"], 128);
    assert_eq!(api["input"][0]["role"], "system");
    assert_eq!(
        encode_text_request(&request, &TextCapabilities::default()).err(),
        Some(ProtocolError::UnsupportedCapability)
    );
}

#[test]
fn plan_catalog_is_account_ordered_strict_and_bounded_without_api_shape_fallback() {
    let bytes = serde_json::to_vec(&json!({"models":[
        {"slug":"z-model","display_name":"Z model","visibility":"list"},
        {"slug":"hidden-model","display_name":"Hidden","visibility":"hidden"},
        {"slug":"a-model","display_name":"A model","visibility":"list"}
    ]}))
    .unwrap();
    let catalog = parse_catalog(&bytes).unwrap();
    assert_eq!(
        catalog
            .models
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        vec!["z-model", "a-model"]
    );
    assert_eq!(catalog.models[0].name, "Z model");
    assert!(!catalog.truncated);
    for value in [
        json!({"data":[]}),
        json!({"models":{}}),
        json!({"models":[{"slug":"id","display_name":"name"}]}),
        json!({"models":[{"slug":"bad id","display_name":"name","visibility":"list"}]}),
        json!({"models":[{"slug":"id","display_name":"name\nprivate marker","visibility":"list"}]}),
        json!({"models":[{"slug":"id","display_name":null,"visibility":"list"}]}),
        json!({"models":[{"slug":"id","display_name":"name","visibility":"list"},{"slug":"id","display_name":"other","visibility":"list"}]}),
    ] {
        let error = parse_catalog(&serde_json::to_vec(&value).unwrap())
            .err()
            .unwrap();
        assert!(matches!(error, TransportError::InvalidCatalog));
        assert!(!error.to_string().contains("private marker"));
    }
    let entries = (0..513)
        .map(|i| json!({"slug":format!("m-{i}"),"display_name":"fixture","visibility":"list"}))
        .collect::<Vec<_>>();
    let catalog = parse_catalog(&serde_json::to_vec(&json!({"models":entries})).unwrap()).unwrap();
    assert_eq!(catalog.models.len(), 512);
    assert!(catalog.truncated);
    let tail =
        json!({"models":[{"slug":"x".repeat(257),"display_name":"hidden","visibility":"hidden"}]});
    assert!(parse_catalog(&serde_json::to_vec(&tail).unwrap()).is_err());
    assert!(matches!(
        parse_catalog(&vec![b' '; 1024 * 1024 + 1]),
        Err(TransportError::LimitExceeded)
    ));
    assert!(matches!(
        parse_catalog(&serde_json::to_vec(&json!({"models":vec![json!({});4097]})).unwrap()),
        Err(TransportError::LimitExceeded)
    ));
}
fn frame(v: Value) -> String {
    format!("data: {v}\n\n")
}
#[test]
fn plan_stream_errors_are_sanitized_preserve_partial_text_and_usage_and_never_finish() {
    let prefix=[
        json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"m","type":"message","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","output_index":0,"item_id":"m","content_index":0,"part":{"type":"output_text","text":""}}),
        json!({"type":"response.output_text.delta","output_index":0,"item_id":"m","content_index":0,"delta":"관측한 부분"})
    ].into_iter().map(frame).collect::<String>();
    for (code, expected) in [
        (
            "subscription_sharing_usage_limit_exceeded",
            ProtocolError::ChatGptPlan(PlanError::UsageLimitExceeded),
        ),
        ("unknown-private-marker", ProtocolError::Provider),
    ] {
        let mut d = StreamDecoder::chatgpt_plan("fixture-model");
        let wire = prefix.clone()
            + &frame(
                json!({"type":"response.failed","response":{"id":"r","model":"fixture-model","error":{"code":code,"message":"private marker"},"usage":{"input_tokens":5,"output_tokens":2}}}),
            );
        let mut events = vec![];
        let mut error = None;
        for byte in wire.as_bytes() {
            let b = d.push(&[*byte]);
            events.extend(b.events);
            if b.error.is_some() {
                error = b.error;
                break;
            }
        }
        assert_eq!(error, Some(expected));
        assert!(
            events
                .iter()
                .any(|e| matches!(e,ModelEvent::TextDelta{text,..} if text=="관측한 부분"))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e,ModelEvent::UsageSnapshot(u) if u.output_tokens==Some(2)))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ModelEvent::Finished { .. }))
        );
        assert_eq!(d.finish(), Err(expected));
        assert!(!expected.to_string().contains("private"));
    }
}
#[test]
fn plan_exact_model_truncation_and_incomplete_remain_distinct() {
    for response in [json!({"id":"r","model":"other-model"}), json!({"id":"r"})] {
        let mut d = StreamDecoder::chatgpt_plan("fixture-model");
        assert_eq!(
            d.push(frame(json!({"type":"response.created","response":response})).as_bytes())
                .error,
            Some(ProtocolError::Protocol)
        );
    }
    let mut d = StreamDecoder::chatgpt_plan("fixture-model");
    assert!(
        d.push(
            frame(json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}))
                .as_bytes()
        )
        .error
        .is_none()
    );
    assert_eq!(d.finish(), Err(ProtocolError::Truncated));
    let mut d = StreamDecoder::chatgpt_plan("fixture-model");
    let wire = frame(
        json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),
    ) + &frame(
        json!({"type":"response.incomplete","response":{"id":"r","model":"fixture-model","status":"incomplete","output":[],"incomplete_details":{"reason":"max_output_tokens"}}}),
    );
    let batch = d.push(wire.as_bytes());
    assert_eq!(batch.error, None);
    assert!(batch.events.iter().any(|e| matches!(
        e,
        ModelEvent::Finished {
            outcome: FinishOutcome::Incomplete,
            ..
        }
    )));
    assert!(d.finish().is_ok());
}

#[test]
fn plan_stream_rejects_terminal_model_or_usage_identity_mismatch_and_named_error_is_safe() {
    for response in [
        json!({"id":"r","model":"wrong-model","status":"completed","output":[]}),
        json!({"id":"wrong-response","model":"fixture-model","error":{"code":"subscription_sharing_usage_limit_exceeded"},"usage":{"input_tokens":9}}),
    ] {
        let kind = if response.get("error").is_some() {
            "response.failed"
        } else {
            "response.completed"
        };
        let mut d = StreamDecoder::chatgpt_plan("fixture-model");
        let wire =
            frame(json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}))
                + &frame(json!({"type":kind,"response":response}));
        let batch = d.push(wire.as_bytes());
        assert_eq!(batch.error, Some(ProtocolError::Protocol));
        assert!(!batch.events.iter().any(|e| matches!(
            e,
            ModelEvent::UsageSnapshot(_) | ModelEvent::Finished { .. }
        )));
    }
    let mut d = StreamDecoder::chatgpt_plan("fixture-model");
    let wire = format!(
        "event: error\ndata: {}\n\n",
        json!({"type":"error","code":"subscription_sharing_usage_unavailable","message":"private marker"})
    );
    assert_eq!(
        d.push(wire.as_bytes()).error,
        Some(ProtocolError::ChatGptPlan(PlanError::UsageUnavailable))
    );
}

#[test]
fn plan_failed_usage_requires_started_explicit_matching_id_and_model() {
    let created = json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}});
    let valid = json!({"id":"r","model":"fixture-model","error":{"code":"subscription_sharing_usage_limit_exceeded"},"usage":{"input_tokens":99,"output_tokens":2}});
    let mut responses = vec![valid.clone()];
    for key in ["id", "model"] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(key);
        responses.push(missing);
        let mut wrong = valid.clone();
        wrong[key] = json!("foreign");
        responses.push(wrong);
        let mut null = valid.clone();
        null[key] = Value::Null;
        responses.push(null);
    }
    for started in [false, true] {
        for response in &responses {
            let mut d = StreamDecoder::chatgpt_plan("fixture-model");
            if started {
                assert!(d.push(frame(created.clone()).as_bytes()).error.is_none());
            }
            let batch =
                d.push(frame(json!({"type":"response.failed","response":response})).as_bytes());
            let attributed = started && response == &valid;
            assert_eq!(
                batch.error,
                Some(if attributed {
                    ProtocolError::ChatGptPlan(PlanError::UsageLimitExceeded)
                } else {
                    ProtocolError::Protocol
                })
            );
            assert_eq!(
                batch
                    .events
                    .iter()
                    .filter(|event| matches!(event, ModelEvent::UsageSnapshot(_)))
                    .count(),
                usize::from(attributed)
            );
            assert!(
                !batch
                    .events
                    .iter()
                    .any(|event| matches!(event, ModelEvent::Finished { .. }))
            );
            assert!(d.finish().is_err());
        }
    }
    // No usage: a standalone safe error can still explain admission failure.
    let mut d = StreamDecoder::chatgpt_plan("fixture-model");
    let batch=d.push(frame(json!({"type":"response.failed","response":{"error":{"code":"subscription_sharing_usage_limit_exceeded"}}})).as_bytes());
    assert_eq!(
        batch.error,
        Some(ProtocolError::ChatGptPlan(PlanError::UsageLimitExceeded))
    );
    assert!(batch.events.is_empty());
}

#[test]
fn plan_full_progress_snapshots_cannot_hide_foreign_or_missing_identity() {
    for kind in ["response.in_progress", "response.queued"] {
        for response in [
            json!({"id":"foreign","model":"fixture-model"}),
            json!({"id":"r","model":"foreign"}),
            json!({"model":"fixture-model"}),
            json!({"id":"r"}),
        ] {
            let mut d = StreamDecoder::chatgpt_plan("fixture-model");
            let wire = frame(
                json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),
            ) + &frame(json!({"type":kind,"response":response}))
                + &frame(
                    json!({"type":"response.completed","response":{"id":"r","model":"fixture-model","status":"completed","output":[]}}),
                );
            let batch = d.push(wire.as_bytes());
            assert_eq!(batch.error, Some(ProtocolError::Protocol));
            assert!(!batch.events.iter().any(|event| matches!(
                event,
                ModelEvent::Finished { .. } | ModelEvent::UsageSnapshot(_)
            )));
            assert_eq!(d.finish(), Err(ProtocolError::Protocol));
        }
        let mut d = StreamDecoder::chatgpt_plan("fixture-model");
        let wire = frame(
            json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),
        ) + &frame(json!({"type":kind,"response":{"id":"r","model":"fixture-model"}}))
            + &frame(
                json!({"type":"response.completed","response":{"id":"r","model":"fixture-model","status":"completed","output":[]}}),
            );
        assert!(d.push(wire.as_bytes()).error.is_none());
        assert!(d.finish().is_ok());
    }
    // The opt-in restriction must not change the existing API progress behavior.
    let mut api = StreamDecoder::new(Protocol::Responses);
    let wire = frame(
        json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),
    ) + &frame(
        json!({"type":"response.in_progress","response":{"id":"foreign","model":"fixture-model"}}),
    ) + &frame(
        json!({"type":"response.completed","response":{"id":"r","model":"fixture-model","status":"completed","output":[]}}),
    );
    assert!(api.push(wire.as_bytes()).error.is_none());
    assert!(api.finish().is_ok());
}
