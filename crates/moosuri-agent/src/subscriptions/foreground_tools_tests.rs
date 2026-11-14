//! Personal plan calls exercise the real Store checkpoint and typed tool loop.
use super::*;
use moosuri_core::{ModelChoice, ModelSelection};

fn final_wire(text: &str) -> String {
    let item = json!({"type":"message","id":"m","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]});
    [json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"m","role":"assistant","content":[]}}),json!({"type":"response.content_part.added","item_id":"m","output_index":0,"content_index":0,"part":{"type":"output_text","text":""}}),json!({"type":"response.output_text.delta","item_id":"m","output_index":0,"content_index":0,"delta":text}),json!({"type":"response.output_text.done","item_id":"m","output_index":0,"content_index":0,"text":text}),json!({"type":"response.content_part.done","item_id":"m","output_index":0,"content_index":0,"part":{"type":"output_text","text":text,"annotations":[]}}),json!({"type":"response.output_item.done","output_index":0,"item":item}),json!({"type":"response.completed","response":{"id":"r","model":"fixture-model","status":"completed","output":[item],"usage":{"input_tokens":8,"output_tokens":2}}})].iter().map(|e|format!("data: {e}\n\n")).collect()
}
fn call_wire(name: &str, args: Value, namespace: &str) -> String {
    let args = args.to_string();
    let item = json!({"type":"function_call","id":"fc","call_id":"call-foreground","namespace":namespace,"name":name,"arguments":args,"status":"completed"});
    [json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc","call_id":"call-foreground","namespace":namespace,"name":name,"arguments":"","status":"in_progress"}}),json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc","delta":args}),json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc","arguments":args}),json!({"type":"response.output_item.done","output_index":0,"item":item}),json!({"type":"response.completed","response":{"id":"r","model":"fixture-model","status":"completed","output":[item],"usage":{"input_tokens":8,"output_tokens":2}}})].iter().map(|e|format!("data: {e}\n\n")).collect()
}
pub(super) async fn tool_response(
    s: &Provider,
    headers: &axum::http::HeaderMap,
    body: &Value,
) -> axum::response::Response {
    if s.tool_mode.load(Ordering::SeqCst) >= 10 {
        return writes_tests::response(s, headers, body).await;
    }
    // P4 can route explicit non-artifact cues through isolated memory planning.
    if body["tools"][0]["tools"][0]["name"] == "memory_command_decision" {
        s.models.fetch_add(1, Ordering::SeqCst);
        return ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],call_wire("memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null}),"moosuri")).into_response();
    }
    s.models.fetch_add(1, Ordering::SeqCst);
    s.requests.lock().unwrap().push(body.clone());
    s.authorizations
        .lock()
        .unwrap()
        .push(headers["authorization"].to_str().unwrap().to_owned());
    assert_eq!(body["model"], "fixture-model");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert!(body.get("max_output_tokens").is_none());
    assert!(body.get("previous_response_id").is_none());
    assert_eq!(body["tools"][0]["name"], "moosuri");
    assert_eq!(body["tools"][0]["type"], "namespace");
    let defs = body["tools"][0]["tools"].as_array().unwrap();
    assert!(defs.iter().all(|d| !matches!(
        d["name"].as_str(),
        Some(
            "memory_command_decision"
                | "mission_intent"
                | "artifact_intent"
                | "artifact_write"
                | "mail_send"
                | "mail_approve"
        )
    )));
    let input = body["input"].as_array().unwrap();
    assert_eq!(input[0]["role"], "developer");
    assert!(input.iter().all(|v| v["role"] != "system"));
    let previous = input.iter().find(|v| v["type"] == "function_call_output");
    let wire = if let Some(previous) = previous {
        assert_eq!(previous["call_id"], "call-foreground");
        assert!(
            input
                .iter()
                .any(|v| v["type"] == "function_call" && v["namespace"] == "moosuri")
        );
        let value: Value = serde_json::from_str(previous["output"].as_str().unwrap()).unwrap();
        if s.tool_mode.load(Ordering::SeqCst) == 2 {
            assert_eq!(value["kind"], "external");
            final_wire("Draft prepared; awaiting approval.")
        } else {
            assert_eq!(value["kind"], "decimal");
            assert_eq!(value["value"]["exact"], "0.3");
            final_wire("0.3")
        }
    } else {
        if s.hold_tool.load(Ordering::SeqCst) > 0 {
            s.tool_entered.notify_one();
            s.tool_release.notified().await;
        }
        let mode = s.tool_mode.load(Ordering::SeqCst);
        if mode == 2 {
            let d = defs
                .iter()
                .find(|d| d["name"] == "mail_prepare_draft")
                .unwrap();
            let connection = d["parameters"]["properties"]["connection_id"]["enum"][0].clone();
            call_wire(
                "mail_prepare_draft",
                json!({"connection_id":connection,"from":"sender@example.test","to":["to@example.test"],"cc":[],"bcc":["hidden@example.test"],"subject":"Synthetic draft","text":"Entire synthetic body"}),
                "moosuri",
            )
        } else {
            assert!(defs.iter().any(|d| d["name"] == "artifact_read"));
            let wire = call_wire(
                "decimal_calculate",
                json!({"operation":"sum","operands":["0.1","0.2"]}),
                if mode == 3 { "foreign" } else { "moosuri" },
            );
            if mode == 4 {
                wire + "data: {\"type\":\"error\",\"error\":{\"code\":\"unknown\"}}\n\n"
            } else {
                wire
            }
        }
    };
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        wire,
    )
        .into_response()
}
async fn configured() -> (
    tempfile::TempDir,
    Store,
    UserContext,
    Fixture,
    SubscriptionService,
    LocalLoginLaunch,
    ModelChoice,
) {
    let f = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Owner".into(),
            "owner@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let u = store
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let s = service(store.clone(), &f);
    let l = login(&s, &u).await;
    let choice = proof(&s, &u, l.binding_id.clone(), l.revision).await;
    store
        .set_model_preference(
            &u,
            Some(ModelSelection {
                source: choice.source,
                provider_id: choice.provider_id.clone(),
                provider_revision: choice.provider_revision,
                model: choice.model.clone(),
                effort: choice.effort.clone(),
            }),
        )
        .await
        .unwrap();
    (dir, store, u, f, s, l, choice)
}
async fn enable(store: &Store, u: &UserContext, l: &LocalLoginLaunch) -> ModelChoice {
    let (b, challenge) = store
        .subscription_tool_probe_binding(
            u,
            l.binding_id.clone(),
            l.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    assert!(challenge.starts_with("proof-"));
    store
        .record_subscription_tool_probe(u, b.subscription.unwrap())
        .await
        .unwrap()
}
async fn run(
    store: &Store,
    u: &UserContext,
    s: &SubscriptionService,
    text: &str,
) -> moosuri_core::ChatTurn {
    let t = store
        .create_thread(u, "Foreground tools".into())
        .await
        .unwrap();
    store
        .submit_turn(
            u,
            t.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            text.into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    crate::runner::execute(
        store.clone(),
        claim,
        s.signals.clone(),
        crate::mail::MailService::new(store.clone(), None, s.signals.clone()),
        crate::web::Service::new(None),
        s.clone(),
    )
    .await;
    store
        .conversation(u, t.id, None)
        .await
        .unwrap()
        .turns
        .remove(0)
}
#[tokio::test]
async fn personal_saved_plan_tool_result_checkpoint_and_frozen_text_capability() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, store, u, f, s, l, _) = configured().await;
    let t = store.create_thread(&u, "Frozen text".into()).await.unwrap();
    store
        .submit_turn(&u, t.id, uuid::Uuid::new_v4().to_string(), "Hello".into())
        .await
        .unwrap();
    assert!(enable(&store, &u, &l).await.function_tools);
    // Re-proving text preserves the exact tool capability, but the admitted run stays text-only.
    assert!(
        proof(&s, &u, l.binding_id.clone(), l.revision)
            .await
            .function_tools
    );
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    assert!(!claim.model.function_tools);
    crate::runner::execute(
        store.clone(),
        claim,
        s.signals.clone(),
        crate::mail::MailService::new(store.clone(), None, s.signals.clone()),
        crate::web::Service::new(None),
        s.clone(),
    )
    .await;
    f.state.tool_mode.store(1, Ordering::SeqCst);
    let done = run(
        &store,
        &u,
        &s,
        "Compute 0.1 plus 0.2; do not save anything.",
    )
    .await;
    assert_eq!(done.run.state, RunState::Completed);
    assert!(done.run.model.function_tools);
    assert_eq!(
        done.assistant
            .iter()
            .map(|v| v.text.as_str())
            .collect::<String>(),
        "0.3"
    );
    assert_eq!(f.state.requests.lock().unwrap().len(), 2);
    assert!(
        store
            .subscription_tool_probe_binding(
                &u,
                l.binding_id.clone(),
                l.revision,
                "fixture-model".into(),
                Some("high".into())
            )
            .await
            .is_err()
    );
    s.disconnect(&u, l.binding_id, l.revision).await.unwrap();
    assert!(store.model_settings(&u).await.unwrap().effective.is_none());
}
#[tokio::test]
async fn personal_tool_partial_namespace_and_trailing_error_never_execute() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    for mode in [3, 4] {
        let (_d, store, u, f, s, l, _) = configured().await;
        enable(&store, &u, &l).await;
        f.state.tool_mode.store(mode, Ordering::SeqCst);
        let done = run(&store, &u, &s, "Compute a decimal total").await;
        assert_ne!(done.run.state, RunState::Completed);
        assert_eq!(f.state.requests.lock().unwrap().len(), 1);
    }
}
#[tokio::test]
async fn personal_tool_round_refreshes_before_result_and_cancel_fences_late_call() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    for cancel in [false, true] {
        let (_d, store, u, f, s, l, _) = configured().await;
        enable(&store, &u, &l).await;
        f.state.tool_mode.store(1, Ordering::SeqCst);
        f.state.hold_tool.store(1, Ordering::SeqCst);
        let t = store.create_thread(&u, "Held tool".into()).await.unwrap();
        let turn = store
            .submit_turn(
                &u,
                t.id.clone(),
                uuid::Uuid::new_v4().to_string(),
                "Compute 0.1 plus 0.2".into(),
            )
            .await
            .unwrap();
        let claim = store.claim_agent_run().await.unwrap().unwrap();
        let task = {
            let store = store.clone();
            let s = s.clone();
            tokio::spawn(async move {
                crate::runner::execute(
                    store.clone(),
                    claim,
                    s.signals.clone(),
                    crate::mail::MailService::new(store.clone(), None, s.signals.clone()),
                    crate::web::Service::new(None),
                    s,
                )
                .await
            })
        };
        tokio::time::timeout(Duration::from_secs(5), f.state.tool_entered.notified())
            .await
            .unwrap();
        if cancel {
            store.cancel_run(&u, turn.run.id, 1).await.unwrap();
        } else {
            store
                .expire_subscription_fixture(&u, l.binding_id, l.revision)
                .await
                .unwrap();
        }
        f.state.tool_release.notify_one();
        task.await.unwrap();
        let done = store
            .conversation(&u, t.id, None)
            .await
            .unwrap()
            .turns
            .remove(0);
        if cancel {
            assert_eq!(done.run.state, RunState::Cancelled);
            assert_eq!(f.state.requests.lock().unwrap().len(), 1);
        } else {
            assert_eq!(done.run.state, RunState::Completed);
            assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
            let auth = f.state.authorizations.lock().unwrap();
            assert_ne!(auth[0], auth[1]);
        }
    }
}
#[tokio::test]
async fn personal_foreground_mail_creates_unapproved_local_draft_without_send() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, store, u, f, s, l, _) = configured().await;
    enable(&store, &u, &l).await;
    use moosuri_connectors::gmail::{SecretString, TokenSet};
    let start = store.start_mail_oauth(&u).await.unwrap();
    let h = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let e = store.claim_mail_oauth(&u, h).await.unwrap();
    store
        .complete_mail_oauth(
            e,
            TokenSet {
                access_token: SecretString::new("synthetic-gmail-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-gmail-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: None,
            },
            "sender@example.test".into(),
        )
        .await
        .unwrap();
    f.state.tool_mode.store(2, Ordering::SeqCst);
    let done = run(&store, &u, &s, "담당자에게 이 내용을 메일로 보내줘.").await;
    assert_eq!(done.run.state, RunState::Completed);
    let drafts = store.mail_actions(&u).await.unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].state, "pending_approval");
    assert_eq!(
        drafts[0].content.as_ref().unwrap().bcc,
        vec!["hidden@example.test"]
    );
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    assert_eq!(f.state.requests.lock().unwrap().len(), 2);
}

#[path = "writes_tests.rs"]
mod writes_tests;
