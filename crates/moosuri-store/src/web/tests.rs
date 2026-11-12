use super::*;
use crate::{
    agent_execution::*,
    tests::{TEST_LOCK, admin, directory, member},
};
use moosuri_core::*;

async fn setup(profile: WebToolProfile) -> (tempfile::TempDir, Store, UserContext, String) {
    let dir = directory();
    let store = Store::open_with_web(dir.path(), profile).unwrap();
    let user = admin(&store).await.user;
    let provider = store
        .create_provider(
            &user,
            ProviderChange {
                name: "Synthetic".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&user, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(&user, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                provider_id: provider.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Synthetic".into())
        .await
        .unwrap()
        .id;
    (dir, store, user, thread)
}
async fn admit(
    store: &Store,
    user: &UserContext,
    thread: &str,
    request: WebToolRequest,
) -> (AgentClaim, StepLease) {
    store
        .submit_turn(
            user,
            thread.into(),
            Uuid::new_v4().to_string(),
            "Public research".into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let context = store.agent_context(&claim.lease).await.unwrap();
    let step = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: br#"{"v":1,"synthetic":true}"#.to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "web-call".into(),
                input: KnowledgeToolInput::External {
                    request: ExternalToolRequest::Web(request),
                },
            }],
        )
        .await
        .unwrap();
    (claim, step)
}
fn page() -> KnowledgeToolResult {
    KnowledgeToolResult::External(
        serde_json::json!({"web_page":{"url":"https://example.com/actual","title":"Observed title","text":"actual synthetic excerpt","retrieved_at_unix_ms":12345,"truncated":true}}),
    )
}

#[tokio::test]
async fn persistent_profile_current_availability_and_unfinished_read_are_fenced() {
    let _g = TEST_LOCK.lock().await;
    let (dir, store, user, thread) = setup(WebToolProfile {
        read: true,
        search: false,
    })
    .await;
    let (claim, step) = admit(
        &store,
        &user,
        &thread,
        WebToolRequest::Read {
            url: "https://example.com".into(),
        },
    )
    .await;
    let profile = claim.tool_profile.as_ref().unwrap();
    assert_eq!(profile.version, 3);
    assert!(profile.web.as_ref().unwrap().read);
    assert!(!profile.web.as_ref().unwrap().search);
    let ExternalToolStart::Dispatch(permit) = store
        .prepare_external_tool(&step, "web-call".into())
        .await
        .unwrap()
    else {
        panic!("missing permit")
    };
    assert!(matches!(
        store
            .prepare_external_tool(&step, "web-call".into())
            .await
            .unwrap(),
        ExternalToolStart::Complete(KnowledgeToolResult::Error(ToolReadError::OutcomeUnknown))
    ));
    assert!(matches!(
        store.complete_external_tool(&permit, page()).await.unwrap(),
        KnowledgeToolResult::Error(ToolReadError::OutcomeUnknown)
    ));
    assert!(
        store
            .conversation(&user, thread.clone(), None)
            .await
            .unwrap()
            .turns[0]
            .run
            .web_evidence
            .is_none()
    );
    drop(permit);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    drop(claim);
    drop(step);
    let (_, step) = admit(
        &store,
        &user,
        &thread,
        WebToolRequest::Read {
            url: "https://example.com".into(),
        },
    )
    .await;
    let result = store
        .prepare_external_tool(&step, "web-call".into())
        .await
        .unwrap();
    assert!(matches!(
        result,
        ExternalToolStart::Complete(KnowledgeToolResult::Error(ToolReadError::Unavailable))
    ));
}

#[tokio::test]
async fn actual_completed_slots_expose_only_scoped_observation_metadata_and_purge_it() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, user, thread) = setup(WebToolProfile {
        read: true,
        search: false,
    })
    .await;
    let other = member(&store, &user, "other-web@example.test").await.user;
    let collection = store
        .create_collection(&user, "Synthetic source".into(), "synthetic".into())
        .await
        .unwrap();
    let source = store
        .ingest_text(
            &user,
            collection.id,
            IngestBatch {
                request_id: Uuid::new_v4().to_string(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: "one".into(),
                    version: "v1".into(),
                    expected_revision: None,
                    title: "Source used with web".into(),
                    text: "Synthetic source fact".into(),
                    speaker: None,
                    observed_at: 1,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap()
        .source_ids
        .remove(0);
    let (claim, step) = admit(
        &store,
        &user,
        &thread,
        WebToolRequest::Read {
            url: "https://example.com".into(),
        },
    )
    .await;
    let ExternalToolStart::Dispatch(permit) = store
        .prepare_external_tool(&step, "web-call".into())
        .await
        .unwrap()
    else {
        panic!("permit")
    };
    // A private dispatch nonce cannot be replaced by result JSON or another user.
    let nonce = store
        .call(|db| {
            let nonce =
                db.conn
                    .query_row("SELECT nonce FROM external_tool_dispatches", [], |r| {
                        r.get::<_, Vec<u8>>(0)
                    })?;
            db.conn
                .execute("UPDATE external_tool_dispatches SET nonce=x'00'", [])?;
            Ok(nonce)
        })
        .await
        .unwrap();
    assert!(matches!(
        store.complete_external_tool(&permit, page()).await,
        Err(StoreError::Unauthorized)
    ));
    store
        .call(move |db| {
            db.conn
                .execute("UPDATE external_tool_dispatches SET nonce=?1", [nonce])?;
            Ok(())
        })
        .await
        .unwrap();
    store.complete_external_tool(&permit, page()).await.unwrap();
    let run = &store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .turns[0]
        .run;
    let refs = run.web_evidence.as_ref().unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].url, "https://example.com/actual");
    assert!(refs[0].truncated);
    assert!(
        !serde_json::to_string(run)
            .unwrap()
            .contains("actual synthetic excerpt")
    );
    assert!(
        store
            .conversation(&other, thread.clone(), None)
            .await
            .is_err()
    );
    let context = store.agent_context(&claim.lease).await.unwrap();
    let source_step = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &source_step,
            PrivateCheckpoint {
                bytes: br#"{"v":1,"synthetic":true}"#.to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "source-call".into(),
                input: KnowledgeToolInput::ReadSource {
                    id: source.clone(),
                    revision: 1,
                    start_byte: 0,
                    max_bytes: 100,
                },
            }],
        )
        .await
        .unwrap();
    store
        .execute_knowledge_tool(&source_step, "source-call".into())
        .await
        .unwrap();
    store.delete_source(&user, source, 1).await.unwrap();
    let current = store
        .conversation(permit.user(), thread, None)
        .await
        .unwrap();
    assert!(current.turns[0].run.web_evidence.is_none());
}

#[tokio::test]
async fn reopening_without_web_does_not_upgrade_or_reenable_a_pinned_queued_run() {
    let _g = TEST_LOCK.lock().await;
    let (dir, store, user, thread) = setup(WebToolProfile {
        read: true,
        search: false,
    })
    .await;
    store
        .submit_turn(
            &user,
            thread.clone(),
            Uuid::new_v4().to_string(),
            "queued public research".into(),
        )
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(claim.tool_profile.as_ref().unwrap().version, 3);
    let context = store.agent_context(&claim.lease).await.unwrap();
    let step = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: br#"{"v":1,"synthetic":true}"#.to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "web-call".into(),
                input: KnowledgeToolInput::External {
                    request: ExternalToolRequest::Web(WebToolRequest::Read {
                        url: "https://example.com".into(),
                    }),
                },
            }],
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .prepare_external_tool(&step, "web-call".into())
            .await
            .unwrap(),
        ExternalToolStart::Complete(KnowledgeToolResult::Error(ToolReadError::Unavailable))
    ));
}

#[tokio::test]
async fn logout_and_current_config_withdrawal_block_late_results() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, user, thread) = setup(WebToolProfile {
        read: true,
        search: false,
    })
    .await;
    let (_claim, step) = admit(
        &store,
        &user,
        &thread,
        WebToolRequest::Read {
            url: "https://example.com".into(),
        },
    )
    .await;
    let ExternalToolStart::Dispatch(permit) = store
        .prepare_external_tool(&step, "web-call".into())
        .await
        .unwrap()
    else {
        panic!("permit")
    };
    store
        .call(|db| {
            db.web_profile.read = false;
            Ok(())
        })
        .await
        .unwrap();
    assert!(store.check_external_tool(&permit).await.is_err());
    assert!(store.complete_external_tool(&permit, page()).await.is_err());
    let thread = store
        .create_thread(&user, "Second".into())
        .await
        .unwrap()
        .id;
    store
        .call(|db| {
            db.web_profile.read = true;
            Ok(())
        })
        .await
        .unwrap();
    let (_, step) = admit(
        &store,
        &user,
        &thread,
        WebToolRequest::Read {
            url: "https://example.com".into(),
        },
    )
    .await;
    let ExternalToolStart::Dispatch(permit) = store
        .prepare_external_tool(&step, "web-call".into())
        .await
        .unwrap()
    else {
        panic!("permit")
    };
    store.logout(&user).await.unwrap();
    assert!(store.check_external_tool(&permit).await.is_err());
    assert!(store.complete_external_tool(&permit, page()).await.is_err());
}
