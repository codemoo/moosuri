//! Signed OAuth extraction exercises collection authority through real refresh CAS.
use super::*;
use moosuri_core::*;
pub(super) async fn response(
    s: &Provider,
    headers: &axum::http::HeaderMap,
    body: &Value,
) -> axum::response::Response {
    s.models.fetch_add(1, Ordering::SeqCst);
    s.requests.lock().unwrap().push(body.clone());
    assert!(
        headers["authorization"]
            .to_str()
            .unwrap()
            .starts_with("Bearer synthetic-access-")
    );
    assert_eq!(body["store"], false);
    assert!(body.get("max_output_tokens").is_none());
    assert_eq!(body["tools"][0]["tools"].as_array().unwrap().len(), 1);
    assert_eq!(body["input"].as_array().unwrap().len(), 2);
    assert_eq!(body["input"][0]["role"], "developer");
    assert!(
        body["input"][0]["content"]
            .as_str()
            .unwrap()
            .contains("UNTRUSTED SOURCE DATA")
    );
    if s.hold_tool.load(Ordering::SeqCst) == 1 {
        s.tool_entered.notify_one();
        s.tool_release.notified().await;
    }
    let args = r#"{"selected":[0]}"#;
    let item = json!({"type":"function_call","id":"fc","call_id":"source-selection","namespace":"moosuri","name":"source_memory_selection","arguments":args,"status":"completed"});
    let frames = [
        json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc","call_id":"source-selection","namespace":"moosuri","name":"source_memory_selection","arguments":"","status":"in_progress"}}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc","delta":args}),
        json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc","arguments":args}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"r","model":"fixture-model","status":"completed","output":[item],"usage":{"input_tokens":8,"output_tokens":2}}}),
    ];
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        frames
            .iter()
            .map(|v| format!("data: {v}\n\n"))
            .collect::<String>(),
    )
        .into_response()
}
async fn enqueue(s: &Store, u: &UserContext) -> (String, String) {
    let c = s
        .create_collection(
            u,
            "Synthetic extraction".into(),
            uuid::Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    s.set_memory_extraction(
        u,
        c.id.clone(),
        MemoryExtractionChange {
            expected_revision: 0,
            enabled: true,
            attribution_mode: "specified".into(),
            reported_speaker: "Reported author".into(),
            daily_calls: 10,
            daily_input_bytes: 24000,
            daily_output_tokens: 4000,
        },
    )
    .await
    .unwrap();
    let source = s
        .ingest_text(
            u,
            c.id.clone(),
            IngestBatch {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: "synthetic".into(),
                    version: "1".into(),
                    expected_revision: None,
                    title: "Synthetic".into(),
                    text: "I do not eat pork.".into(),
                    speaker: None,
                    observed_at: 100,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap()
        .source_ids
        .remove(0);
    (c.id, source)
}
#[tokio::test]
async fn automatic_memory_chatgpt_refresh_survives_logout_but_revoked_collection_cannot_commit_or_replay()
 {
    let _g = crate::tests::TEST_LOCK.lock().await;
    for mode in 0..4 {
        let f = fixture().await;
        f.state.expires.store(1, Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .bootstrap(
                "Synthetic".into(),
                "owner@example.test".into(),
                PASSWORD.into(),
                false,
            )
            .await
            .unwrap();
        let user = store
            .login("owner@example.test".into(), PASSWORD.into())
            .await
            .unwrap()
            .user;
        let s = service(store.clone(), &f);
        let launch = login(&s, &user).await;
        let binding = store
            .subscription_ref(
                &user,
                launch.binding_id,
                launch.revision,
                "fixture-model".into(),
                None,
            )
            .await
            .unwrap();
        store
            .record_subscription_probe(&user, binding.clone())
            .await
            .unwrap();
        let choice = store
            .record_subscription_tool_probe(&user, binding.clone())
            .await
            .unwrap();
        store
            .set_model_preference(
                &user,
                Some(ModelSelection {
                    source: choice.source,
                    provider_id: choice.provider_id,
                    provider_revision: choice.provider_revision,
                    model: choice.model,
                    effort: choice.effort,
                }),
            )
            .await
            .unwrap();
        let (collection, source) = enqueue(&store, &user).await;
        let claim = store.claim_memory_extraction().await.unwrap().unwrap();
        let mut wrong = binding.clone();
        wrong.effort = Some("high".into());
        assert!(
            store
                .subscription_access(
                    SubscriptionAuthority::Extraction(claim.lease.clone()),
                    wrong
                )
                .await
                .is_err()
        );
        store.logout(&user).await.unwrap();
        let observer = store
            .login("owner@example.test".into(), PASSWORD.into())
            .await
            .unwrap()
            .user;
        if mode == 1 {
            f.state.pause_discovery.store(1, Ordering::SeqCst);
        }
        if mode == 2 {
            f.state.hold_tool.store(1, Ordering::SeqCst);
        }
        if mode == 3 {
            f.state.pause_token.store(1, Ordering::SeqCst);
        }
        let operation = crate::automatic_memory::execute(&store, &s.signals, &s, claim);
        tokio::pin!(operation);
        let revoke = async {
            if mode == 1 {
                f.state.discovery_entered.notified().await;
                store
                    .delete_source(&observer, source.clone(), 1)
                    .await
                    .unwrap();
                f.state.discovery_release.notify_one();
            } else if mode == 2 {
                f.state.tool_entered.notified().await;
                store
                    .delete_source(&observer, source.clone(), 1)
                    .await
                    .unwrap();
                f.state.tool_release.notify_one();
            } else if mode == 3 {
                f.state.token_entered.notified().await;
                store
                    .delete_source(&observer, source.clone(), 1)
                    .await
                    .unwrap();
                // Keep the provider reply held until the worker has dropped the
                // dispatched refresh; this exercises cancellation, not rejection
                // of a completed replacement after source withdrawal.
            }
        };
        tokio::time::timeout(Duration::from_secs(8), async {
            tokio::join!(operation, revoke);
        })
        .await
        .unwrap();
        let memories = store.memories(&observer, None).await.unwrap().items;
        if mode == 0 {
            assert_eq!(memories[0].draft.text, "I do not eat pork.");
            assert_eq!(memories.len(), 1);
        } else {
            assert!(memories.is_empty());
        }
        assert_eq!(
            f.state.models.load(Ordering::SeqCst),
            usize::from(mode == 0 || mode == 2)
        );
        assert_eq!(
            f.state.refreshes.load(Ordering::SeqCst),
            usize::from(mode != 1)
        );
        let statuses = store.subscriptions(&observer).await.unwrap();
        assert_eq!(
            statuses[0].status,
            if mode == 3 { "needs_reauth" } else { "active" }
        );
        if mode == 3 {
            f.state.token_release.notify_one();
            assert!(
                tokio::time::timeout(
                    Duration::from_secs(1),
                    s.client(
                        SubscriptionAuthority::User(observer.clone()),
                        binding.clone()
                    )
                )
                .await
                .unwrap()
                .is_err()
            );
            assert_eq!(
                f.state.refreshes.load(Ordering::SeqCst),
                1,
                "a dropped rotating exchange must never replay"
            );
        }
        assert!(store.claim_memory_extraction().await.unwrap().is_none());
        assert_ne!(
            store
                .memory_extraction_status(&observer, collection)
                .await
                .unwrap()
                .state,
            "queued"
        );
    }
}
