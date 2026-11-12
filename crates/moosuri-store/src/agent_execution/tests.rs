use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_core::*;

async fn configured(store: &Store) -> UserContext {
    let user = admin(store).await.user;
    let p = store
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
        .record_model_probe(&user, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                provider_id: p.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    user
}
fn capsule(text: &str) -> PrivateCheckpoint {
    PrivateCheckpoint {
        bytes: encoded(&serde_json::json!({"v":1,"private":text}))
            .unwrap()
            .into_bytes(),
    }
}
fn patch(text: &str) -> Vec<OutputPatch> {
    vec![OutputPatch::Append {
        item: "answer".into(),
        text: text.into(),
        phase: None,
        refusal: false,
    }]
}
fn query(text: &str) -> KnowledgeQuery {
    KnowledgeQuery {
        kind: KnowledgeKind::Source,
        text: text.into(),
        collection_id: None,
        subject: None,
        speaker: None,
        valid_at: None,
        cursor: None,
    }
}
fn input(external: &str, text: &str) -> SourceInput {
    SourceInput {
        external_id: external.into(),
        version: "v1".into(),
        expected_revision: None,
        title: "Synthetic source".into(),
        text: text.into(),
        speaker: None,
        observed_at: 100,
        valid_from: None,
        valid_until: None,
    }
}
fn batch(cursor: i64, entries: Vec<SourceInput>) -> IngestBatch {
    IngestBatch {
        request_id: Uuid::new_v4().to_string(),
        expected_cursor: cursor,
        next_cursor: cursor + 1,
        complete_snapshot: false,
        entries,
    }
}
fn draft(text: &str) -> MemoryDraft {
    MemoryDraft {
        text: text.into(),
        subject: "Synthetic assertion".into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    }
}
async fn source(store: &Store, user: &UserContext, text: &str) -> (KnowledgeCollection, String) {
    let group = store
        .create_collection(user, "Synthetic".into(), Uuid::new_v4().to_string())
        .await
        .unwrap();
    let r = store
        .ingest_text(user, group.id.clone(), batch(0, vec![input("one", text)]))
        .await
        .unwrap();
    (group, r.source_ids[0].clone())
}
async fn claimed(store: &Store, user: &UserContext, thread: &str) -> AgentClaim {
    store
        .submit_turn(
            user,
            thread.into(),
            Uuid::new_v4().to_string(),
            "Keep this user text".into(),
        )
        .await
        .unwrap();
    store.claim_agent_run().await.unwrap().unwrap()
}
fn read(id: &str, start: u32, max: u32) -> PendingKnowledgeCall {
    PendingKnowledgeCall {
        call_id: "read-1".into(),
        input: KnowledgeToolInput::ReadSource {
            id: id.into(),
            revision: 1,
            start_byte: start,
            max_bytes: max,
        },
    }
}
async fn tool_step(store: &Store, claim: &AgentClaim, call: PendingKnowledgeCall) -> StepLease {
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(&step, capsule("model continuation"), vec![call])
        .await
        .unwrap();
    step
}
async fn finish(store: &Store, claim: &AgentClaim, text: &str) -> StepLease {
    let context = store.agent_context(&claim.lease).await.unwrap();
    let step = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store.append_agent_output(&step, patch(text)).await.unwrap();
    store
        .commit_agent_step(&step, capsule(text), vec![])
        .await
        .unwrap();
    store
        .finish_agent_run(&step, RunState::Completed, None)
        .await
        .unwrap();
    step
}

#[tokio::test]
async fn slots_exact_utf8_spans_receipts_and_private_state_are_atomic_and_bounded() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (_, id) = source(&store, &user, "앞🌱반복 인용\n반복 인용 끝").await;
    let thread = store.create_thread(&user, "Private".into()).await.unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        0
    );
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    assert!(matches!(
        store.begin_agent_step(&claim.lease, 0).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store.execute_knowledge_tool(&step, "read-1".into()).await,
        Err(StoreError::Conflict)
    ));
    let call = read(&id, 3, 6); // 4-byte emoji plus a 3-byte character: clamp to emoji only.
    let context = store
        .commit_agent_step(&step, capsule("private completion"), vec![call.clone()])
        .await
        .unwrap();
    assert!(matches!(
        context.current_step_status,
        Some(AgentStepStatus::Completed)
    ));
    assert_eq!(context.calls.len(), 1);
    store
        .commit_agent_step(&step, capsule("private completion"), vec![call])
        .await
        .unwrap();
    assert!(matches!(
        store
            .commit_agent_step(&step, capsule("changed"), vec![])
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .begin_agent_step(&claim.lease, context.checkpoint_seq)
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "not-persisted".into())
            .await,
        Err(StoreError::NotFound)
    ));
    let result = store
        .execute_knowledge_tool(&step, "read-1".into())
        .await
        .unwrap();
    match &result {
        KnowledgeToolResult::Source(v) => {
            assert_eq!(v.text, "🌱");
            assert_eq!((v.start_byte, v.end_byte), (3, 7));
            assert_eq!(v.next_start_byte, Some(7));
        }
        _ => panic!("expected excerpt"),
    }
    let replay = store
        .execute_knowledge_tool(&step, "read-1".into())
        .await
        .unwrap();
    assert_eq!(encoded(&result).unwrap(), encoded(&replay).unwrap());
    assert!(encoded(&result).unwrap().len() <= TOOL_RESULT_LIMIT);
    assert!(matches!(
        store.append_output(&claim.lease, patch("bypass")).await,
        Err(StoreError::Conflict)
    ));
    let run = claim.lease.run.clone();
    store
        .call(move |db| {
            assert_eq!(
                db.conn.query_row(
                    "SELECT tool_calls FROM agent_execution WHERE run_id=?1",
                    [&run],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM agent_dependencies WHERE run_id=?1",
                    [&run],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            Ok(())
        })
        .await
        .unwrap();
    let last = finish(&store, &claim, "Visible answer").await;
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        2
    );
    store
        .update_agent_step_usage(
            &step,
            RunUsage {
                input_tokens: Some(7),
                output_tokens: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    store
        .update_agent_step_usage(
            &last,
            RunUsage {
                input_tokens: Some(8),
                output_tokens: Some(3),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let page = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap();
    assert_eq!(page.turns[0].run.usage.input_tokens, Some(15));
    assert_eq!(page.turns[0].assistant[0].text, "Visible answer");
    let events = store.chat_events(&user, thread.id, 0).await.unwrap();
    assert!(!encoded(&events).unwrap().contains("private completion"));
}

#[tokio::test]
async fn tool_read_validation_search_five_hit_limit_and_memory_wire_limit() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (group, id) = source(&store, &user, &format!("🌱{}", "searchword ".repeat(4000))).await;
    store
        .ingest_text(
            &user,
            group.id.clone(),
            batch(
                1,
                (0..7)
                    .map(|i| input(&format!("extra{i}"), "searchword other text"))
                    .collect(),
            ),
        )
        .await
        .unwrap();
    let mut large = draft("x");
    for i in 0..3 {
        large.evidence.push(EvidenceRef {
            source_id: id.clone(),
            source_revision: 1,
            start_byte: 4 + i * 4096,
            end_byte: 4 + (i + 1) * 4096,
            quote: "searchword "
                .repeat(4000)
                .get(i as usize * 4096..(i as usize + 1) * 4096)
                .unwrap()
                .into(),
        });
    }
    while encoded(&large).unwrap().len() <= 16 * 1024 {
        large.text.push('x');
    }
    large.text.pop();
    assert!(large.text.len() < 4096);
    let m = store
        .create_memory(&user, Uuid::new_v4().to_string(), large)
        .await
        .unwrap();
    let thread = store.create_thread(&user, "Bounds".into()).await.unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    assert!(matches!(
        store
            .commit_agent_step(
                &step,
                PrivateCheckpoint {
                    bytes: vec![b'x'; 262145]
                },
                vec![]
            )
            .await,
        Err(StoreError::InvalidInput)
    ));
    assert!(matches!(
        store
            .commit_agent_step(
                &step,
                PrivateCheckpoint {
                    bytes: b"invalid json".to_vec()
                },
                vec![]
            )
            .await,
        Err(StoreError::InvalidInput)
    ));
    let calls = vec![
        PendingKnowledgeCall {
            call_id: "search".into(),
            input: KnowledgeToolInput::Search {
                query: query("searchword"),
            },
        },
        read(&id, 1, 4),
        PendingKnowledgeCall {
            call_id: "memory".into(),
            input: KnowledgeToolInput::ReadMemory {
                id: m.id.clone(),
                revision: 1,
            },
        },
    ];
    store
        .commit_agent_step(&step, capsule("v1"), calls)
        .await
        .unwrap();
    let result = store
        .execute_knowledge_tool(&step, "search".into())
        .await
        .unwrap();
    match result {
        KnowledgeToolResult::Search(v) => {
            assert_eq!(v.hits.len(), 5);
            assert!(!v.truncated);
            assert!(v.next_cursor.is_some());
        }
        _ => panic!("expected search"),
    }
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "read-1".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Error(ToolReadError::InvalidInput)
    ));
    let result = store
        .execute_knowledge_tool(&step, "memory".into())
        .await
        .unwrap();
    assert!(encoded(&result).unwrap().len() <= TOOL_RESULT_LIMIT);
    assert!(matches!(
        result,
        KnowledgeToolResult::Error(ToolReadError::TooLargeNarrowRead)
    ));
    // Exact source paging clamps at document length, with no invented offsets.
    let context = store.agent_context(&claim.lease).await.unwrap();
    let step = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    let text = store.source(&user, id.clone(), Some(1)).await.unwrap().text;
    store
        .commit_agent_step(
            &step,
            capsule("page"),
            vec![read(&id, text.len() as u32 - 2, 4096)],
        )
        .await
        .unwrap();
    match store
        .execute_knowledge_tool(&step, "read-1".into())
        .await
        .unwrap()
    {
        KnowledgeToolResult::Source(v) => {
            assert_eq!(v.end_byte, text.len() as u32);
            assert_eq!(v.next_start_byte, None);
            assert!(!v.has_more)
        }
        _ => panic!("expected last slice"),
    }
}

#[tokio::test]
async fn source_delete_purges_inherited_context_events_and_checkpoints_but_not_user_or_accounting()
{
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (_, id) = source(&store, &user, "Synthetic private canary").await;
    let (_, unused) = source(&store, &user, "Unrelated source").await;
    let thread = store
        .create_thread(&user, "Inherited".into())
        .await
        .unwrap();
    let a = claimed(&store, &user, &thread.id).await;
    let read = tool_step(&store, &a, read(&id, 0, 4096)).await;
    store
        .execute_knowledge_tool(&read, "read-1".into())
        .await
        .unwrap();
    let first = finish(&store, &a, "Derived canary answer").await;
    let b = claimed(&store, &user, &thread.id).await;
    assert!(b.messages.iter().any(|m| m.text == "Derived canary answer"));
    let second = finish(&store, &b, "Indirect canary answer").await;
    let unrelated_thread = store
        .create_thread(&user, "Unrelated".into())
        .await
        .unwrap();
    let unrelated = claimed(&store, &user, &unrelated_thread.id).await;
    finish(&store, &unrelated, "Independent output").await;
    let cursor = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap()
        .cursor;
    store.delete_source(&user, unused, 1).await.unwrap();
    assert_eq!(
        store
            .conversation(&user, thread.id.clone(), None)
            .await
            .unwrap()
            .turns[0]
            .assistant[0]
            .text,
        "Derived canary answer"
    );
    store.delete_source(&user, id, 1).await.unwrap();
    let page = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap();
    assert!(
        page.turns
            .iter()
            .all(|t| t.assistant.is_empty() && t.user_text == "Keep this user text")
    );
    assert!(page.turns.iter().all(|t| !t.run.can_retry));
    let events = store.chat_events(&user, thread.id, cursor).await.unwrap();
    assert!(events.reset);
    assert!(events.events.is_empty());
    assert_eq!(
        store
            .conversation(&user, unrelated_thread.id.clone(), None)
            .await
            .unwrap()
            .turns[0]
            .assistant[0]
            .text,
        "Independent output"
    );
    store.call(|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM agent_steps WHERE checkpoint IS NOT NULL AND run_id IN(SELECT run_id FROM agent_execution WHERE invalidated=1)",[],|r|r.get::<_,i64>(0))?,0);assert_eq!(db.conn.query_row("SELECT count(*) FROM agent_tool_slots WHERE run_id IN(SELECT run_id FROM agent_execution WHERE invalidated=1)",[],|r|r.get::<_,i64>(0))?,0);Ok(())}).await.unwrap();
    store
        .update_agent_step_usage(
            &first,
            RunUsage {
                input_tokens: Some(4),
                output_tokens: Some(1),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    store
        .update_agent_step_usage(
            &second,
            RunUsage {
                input_tokens: Some(6),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .usage_summary(&user)
            .await
            .unwrap()
            .reported_input_tokens,
        10
    );
}

#[tokio::test]
async fn tool_transaction_failure_never_delivers_an_untracked_result() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (_, id) = source(&store, &user, "private result").await;
    let thread = store.create_thread(&user, "Atomic".into()).await.unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = tool_step(&store, &claim, read(&id, 0, 4096)).await;
    store.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_result BEFORE UPDATE OF result ON agent_tool_slots BEGIN SELECT RAISE(ABORT,'synthetic fault'); END;")?;Ok(())}).await.unwrap();
    assert!(
        store
            .execute_knowledge_tool(&step, "read-1".into())
            .await
            .is_err()
    );
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM agent_dependencies", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT tool_calls FROM agent_execution", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM agent_tool_slots WHERE result IS NOT NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            db.conn.execute_batch("DROP TRIGGER fail_result")?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "read-1".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Source(_)
    ));
}

#[tokio::test]
async fn authority_provider_and_tenant_guards_precede_tool_reads() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let other = member(&store, &user, "other@example.test").await.user;
    let (_, foreign) = source(&store, &other, "Other tenant canary").await;
    let thread = store
        .create_thread(&user, "Isolation".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = tool_step(&store, &claim, read(&foreign, 0, 4096)).await;
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "read-1".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Error(ToolReadError::NotFound)
    ));
    let mut forged = step.clone();
    forged.run.tenant = other.tenant.0.clone();
    assert!(matches!(
        store.execute_knowledge_tool(&forged, "read-1".into()).await,
        Err(StoreError::Unauthorized)
    ));
    store.logout(&user).await.unwrap();
    assert!(matches!(
        store.execute_knowledge_tool(&step, "read-1".into()).await,
        Err(StoreError::Unauthorized)
    ));
    store
        .update_agent_step_usage(
            &step,
            RunUsage {
                input_tokens: Some(3),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM agent_dependencies", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
    let dir2 = directory();
    let store = Store::open(dir2.path()).unwrap();
    let user = configured(&store).await;
    let (_, id) = source(&store, &user, "Provider protected").await;
    let thread = store.create_thread(&user, "Provider".into()).await.unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = tool_step(&store, &claim, read(&id, 0, 4096)).await;
    let p = store
        .list_providers(&user)
        .await
        .unwrap()
        .providers
        .remove(0);
    store
        .update_provider(
            &user,
            p.id,
            p.revision,
            ProviderChange {
                name: p.name,
                config: p.config,
                enabled: false,
                credential: CredentialChange::Keep,
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .execute_knowledge_tool(&step, "read-1".into())
            .await
            .is_err()
    );
    assert!(
        store
            .append_agent_output(&step, patch("must never publish"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn unknown_dispatch_is_never_replayed_after_restart_but_explicit_retry_keeps_budget() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let thread = store.create_thread(&user, "Unknown".into()).await.unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    store
        .append_agent_output(&step, patch("partial"))
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.claim_agent_run().await.unwrap().is_none());
    assert!(
        store
            .commit_agent_step(&step, capsule("late"), vec![])
            .await
            .is_err()
    );
    let p = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap();
    assert_eq!(p.turns[0].run.state, RunState::OutcomeUnknown);
    store
        .retry_run(&user, step.run_id().into(), 1)
        .await
        .unwrap();
    let next = store.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(
        next.context.current_step_status,
        Some(AgentStepStatus::Unknown)
    );
    let retry = store
        .begin_agent_step(&next.lease, next.context.checkpoint_seq)
        .await
        .unwrap();
    assert_eq!(retry.step(), 2);
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        2
    );
    store.delete_thread(&user, thread.id).await.unwrap();
    store
        .update_agent_step_usage(
            &step,
            RunUsage {
                input_tokens: Some(4),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    store
        .update_agent_step_usage(
            &retry,
            RunUsage {
                input_tokens: Some(6),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .usage_summary(&user)
            .await
            .unwrap()
            .reported_input_tokens,
        10
    );
    let mut forged = retry;
    forged.nonce[0] ^= 1;
    assert!(matches!(
        store
            .update_agent_step_usage(&forged, RunUsage::default())
            .await,
        Err(StoreError::Unauthorized)
    ));
}

#[tokio::test]
async fn six_dispatches_and_deadline_survive_attempt_retries_and_exhaustion_is_truthful() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let thread = store.create_thread(&user, "Budget".into()).await.unwrap();
    let mut claim = claimed(&store, &user, &thread.id).await;
    let mut deadline = None;
    for i in 1..=6 {
        let step = store
            .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
            .await
            .unwrap();
        assert_eq!(step.step(), i);
        store
            .commit_agent_step(&step, capsule("completed bounded step"), vec![])
            .await
            .unwrap();
        store
            .finish_agent_run(&step, RunState::Failed, Some(RunFailure::ProtocolFailed))
            .await
            .unwrap();
        let fixed = store
            .call(|db| {
                Ok(db
                    .conn
                    .query_row("SELECT deadline_ms FROM agent_execution", [], |r| {
                        r.get::<_, i64>(0)
                    })?)
            })
            .await
            .unwrap();
        if let Some(old) = deadline {
            assert_eq!(fixed, old)
        } else {
            deadline = Some(fixed)
        }
        if i < 6 {
            store
                .retry_run(&user, step.run_id().into(), step.attempt())
                .await
                .unwrap();
            claim = store.claim_agent_run().await.unwrap().unwrap();
        }
    }
    let page = store.conversation(&user, thread.id, None).await.unwrap();
    assert!(!page.turns[0].run.can_retry);
    assert!(matches!(
        store.retry_run(&user, claim.lease.run.clone(), 6).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        6
    );
}

#[tokio::test]
async fn invalid_argument_slots_consume_twelve_tools_without_accepting_arbitrary_results() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let thread = store
        .create_thread(&user, "Tool budget".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    let calls = (0..12)
        .map(|i| PendingKnowledgeCall {
            call_id: format!("invalid{i}"),
            input: KnowledgeToolInput::InvalidArguments,
        })
        .collect();
    store
        .commit_agent_step(&step, capsule("bad arguments are sanitized"), calls)
        .await
        .unwrap();
    for i in 0..12 {
        assert!(matches!(
            store
                .execute_knowledge_tool(&step, format!("invalid{i}"))
                .await
                .unwrap(),
            KnowledgeToolResult::Error(ToolReadError::InvalidInput)
        ));
    }
    store
        .execute_knowledge_tool(&step, "invalid0".into())
        .await
        .unwrap();
    let c = store.agent_context(&claim.lease).await.unwrap();
    let final_step = store
        .begin_agent_step(&claim.lease, c.checkpoint_seq)
        .await
        .unwrap();
    assert!(matches!(
        store
            .commit_agent_step(
                &final_step,
                capsule("overflow"),
                vec![PendingKnowledgeCall {
                    call_id: "extra".into(),
                    input: KnowledgeToolInput::InvalidArguments
                }]
            )
            .await,
        Err(StoreError::BudgetExhausted)
    ));
    store
        .commit_agent_step(&final_step, capsule("valid final"), vec![])
        .await
        .unwrap();
    store
        .finish_agent_run(&final_step, RunState::Failed, None)
        .await
        .unwrap();
    let page = store.conversation(&user, thread.id, None).await.unwrap();
    assert!(!page.turns[0].run.can_retry);
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT tool_calls FROM agent_execution", [], |r| r
                        .get::<_, i64>(0))?,
                12
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn deadline_and_backward_clock_block_dispatch_and_output() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let thread = store.create_thread(&user, "Deadline".into()).await.unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    store
        .call(|db| {
            db.conn
                .execute("UPDATE agent_execution SET deadline_ms=?1", [millis()? - 1])?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store.append_agent_output(&step, patch("too late")).await,
        Err(StoreError::BudgetExhausted)
    ));
    assert!(
        !store
            .conversation(&user, thread.id, None)
            .await
            .unwrap()
            .turns[0]
            .run
            .can_retry
    );
    let thread = store.create_thread(&user, "Clock".into()).await.unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    let run = step.run_id().to_owned();
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE agent_execution SET last_clock_ms=?2 WHERE run_id=?1",
                params![run, millis()? + 1000],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        store
            .commit_agent_step(&step, capsule("clock rollback"), vec![])
            .await
            .is_err()
    );
}

#[tokio::test]
async fn destructive_journal_fault_replays_derived_purge_before_readiness() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (_, id) = source(&store, &user, "Journal private canary").await;
    let thread = store.create_thread(&user, "Journal".into()).await.unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = tool_step(&store, &claim, read(&id, 0, 4096)).await;
    store
        .execute_knowledge_tool(&step, "read-1".into())
        .await
        .unwrap();
    finish(&store, &claim, "Derived journal canary").await;
    let cursor = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap()
        .cursor;
    let tenant = user.tenant.0.clone();
    let object = id.clone();
    store
        .call(move |db| {
            db.knowledge_journal.append(&tenant, &object, 2)?;
            Ok(())
        })
        .await
        .unwrap();
    // Model the durable-journal/SQLite commit crash gap with a genuine file reopen.
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.source(&user, id, None).await,
        Err(StoreError::NotFound)
    ));
    let page = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap();
    assert!(page.turns[0].assistant.is_empty());
    assert_eq!(page.turns[0].user_text, "Keep this user text");
    assert!(
        store
            .chat_events(&user, thread.id, cursor)
            .await
            .unwrap()
            .reset
    );
    store
        .call(|db| {
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM agent_steps WHERE checkpoint IS NOT NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM agent_tool_slots", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
    let journal = std::fs::read(dir.path().join("knowledge-invalidations.journal")).unwrap();
    assert!(!journal.windows(b"canary".len()).any(|v| v == b"canary"));
}

#[tokio::test]
async fn correction_source_update_forget_and_collection_delete_revoke_actual_dependencies() {
    let _lock = TEST_LOCK.lock().await;
    for action in 0..4 {
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = configured(&store).await;
        let (group, id) = source(&store, &user, "original fact quote").await;
        let mut memory = draft("Derived memory fact");
        memory.evidence.push(EvidenceRef {
            source_id: id.clone(),
            source_revision: 1,
            start_byte: 0,
            end_byte: 8,
            quote: "original".into(),
        });
        let memory = store
            .create_memory(&user, Uuid::new_v4().to_string(), memory)
            .await
            .unwrap();
        let thread = store.create_thread(&user, "Mutation".into()).await.unwrap();
        let claim = claimed(&store, &user, &thread.id).await;
        let call = PendingKnowledgeCall {
            call_id: "memory".into(),
            input: KnowledgeToolInput::ReadMemory {
                id: memory.id.clone(),
                revision: 1,
            },
        };
        let step = tool_step(&store, &claim, call).await;
        store
            .execute_knowledge_tool(&step, "memory".into())
            .await
            .unwrap();
        let output = finish(&store, &claim, "Used derived memory").await;
        let page = store
            .conversation(&user, thread.id.clone(), None)
            .await
            .unwrap();
        assert_eq!(page.turns[0].run.evidence.len(), 2);
        assert!(
            page.turns[0]
                .run
                .evidence
                .iter()
                .any(|v| v.id == memory.id && v.revision == 1 && v.title == "Synthetic assertion")
        );
        assert!(
            page.turns[0]
                .run
                .evidence
                .iter()
                .any(|v| v.id == id && v.revision == 1 && v.title == "Synthetic source")
        );
        match action {
            0 => {
                store
                    .correct_memory(
                        &user,
                        memory.id,
                        1,
                        Uuid::new_v4().to_string(),
                        draft("Corrected independent fact"),
                    )
                    .await
                    .unwrap();
            }
            1 => {
                let mut update = input("one", "Updated fact quote");
                update.version = "v2".into();
                update.expected_revision = Some(1);
                store
                    .ingest_text(&user, group.id, batch(1, vec![update]))
                    .await
                    .unwrap();
            }
            2 => {
                store.forget_memory(&user, memory.id, 1).await.unwrap();
            }
            _ => {
                store
                    .disconnect_collection(&user, group.id, 2, DisconnectMode::Delete)
                    .await
                    .unwrap();
            }
        }
        let page = store.conversation(&user, thread.id, None).await.unwrap();
        assert!(page.turns[0].assistant.is_empty());
        assert!(page.turns[0].run.evidence.is_empty());
        assert!(
            store
                .append_agent_output(&output, patch("stale result"))
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn deletion_between_read_dispatch_and_output_blocks_stale_publication() {
    let _lock = TEST_LOCK.lock().await;
    for after_dispatch in [false, true] {
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = configured(&store).await;
        let (_, id) = source(&store, &user, "read before delete").await;
        let thread = store.create_thread(&user, "Race".into()).await.unwrap();
        let claim = claimed(&store, &user, &thread.id).await;
        let read = tool_step(&store, &claim, read(&id, 0, 4096)).await;
        store
            .execute_knowledge_tool(&read, "read-1".into())
            .await
            .unwrap();
        let context = store.agent_context(&claim.lease).await.unwrap();
        let dispatched = if after_dispatch {
            Some(
                store
                    .begin_agent_step(&claim.lease, context.checkpoint_seq)
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        store.delete_source(&user, id, 1).await.unwrap();
        if let Some(step) = dispatched {
            assert!(
                store
                    .append_agent_output(&step, patch("late answer"))
                    .await
                    .is_err()
            );
            assert!(
                store
                    .commit_agent_step(&step, capsule("private late answer"), vec![])
                    .await
                    .is_err()
            );
        } else {
            assert!(matches!(
                store
                    .begin_agent_step(&claim.lease, context.checkpoint_seq)
                    .await,
                Err(StoreError::Conflict)
            ));
        }
        assert!(store.agent_context(&claim.lease).await.is_err());
        assert!(
            store
                .conversation(&user, thread.id, None)
                .await
                .unwrap()
                .turns[0]
                .assistant
                .is_empty()
        );
        store
            .update_agent_step_usage(
                &read,
                RunUsage {
                    input_tokens: Some(5),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn failed_context_is_not_inherited_and_legacy_success_context_is_fenced() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (_, id) = source(&store, &user, "failed input").await;
    let thread = store.create_thread(&user, "Omitted".into()).await.unwrap();
    let failed = claimed(&store, &user, &thread.id).await;
    let step = tool_step(&store, &failed, read(&id, 0, 4096)).await;
    store
        .execute_knowledge_tool(&step, "read-1".into())
        .await
        .unwrap();
    store
        .append_agent_output(&step, patch("Failed knowledge partial"))
        .await
        .unwrap();
    store
        .finish_agent_run(&step, RunState::Failed, None)
        .await
        .unwrap();
    let next = claimed(&store, &user, &thread.id).await;
    assert!(
        !next
            .messages
            .iter()
            .any(|m| m.text == "Failed knowledge partial")
    );
    finish(&store, &next, "Independent fresh answer").await;
    store.delete_source(&user, id, 1).await.unwrap();
    let page = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap();
    assert!(page.turns[0].assistant.is_empty());
    assert_eq!(page.turns[1].assistant[0].text, "Independent fresh answer");
    // Successful generated knowledge carried through the legacy context builder
    // must keep provenance, even though its public text-only API is unchanged.
    let (_, id) = source(&store, &user, "legacy context source").await;
    let t = store
        .create_thread(&user, "Legacy inherit".into())
        .await
        .unwrap();
    let a = claimed(&store, &user, &t.id).await;
    let step = tool_step(&store, &a, read(&id, 0, 4096)).await;
    store
        .execute_knowledge_tool(&step, "read-1".into())
        .await
        .unwrap();
    finish(&store, &a, "Successful knowledge").await;
    store
        .submit_turn(
            &user,
            t.id.clone(),
            Uuid::new_v4().to_string(),
            "Legacy followup".into(),
        )
        .await
        .unwrap();
    let legacy = store.claim_run().await.unwrap().unwrap();
    assert!(
        legacy
            .messages
            .iter()
            .any(|m| m.text == "Successful knowledge")
    );
    store
        .append_output(&legacy.lease, patch("Legacy derived answer"))
        .await
        .unwrap();
    store
        .finish_run(&legacy.lease, RunState::Completed, None)
        .await
        .unwrap();
    store.delete_source(&user, id, 1).await.unwrap();
    let page = store.conversation(&user, t.id, None).await.unwrap();
    assert!(page.turns.iter().all(|t| t.assistant.is_empty()));
}

#[tokio::test]
async fn supervisor_can_settle_before_dispatch_without_fabricating_a_receipt() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let thread = store
        .create_thread(&user, "Preflight".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    assert!(matches!(
        store
            .finish_agent_execution(&claim.lease, RunState::Completed, None)
            .await,
        Err(StoreError::InvalidInput)
    ));
    store
        .finish_agent_execution(
            &claim.lease,
            RunState::Failed,
            Some(RunFailure::ContinuationUnsupported),
        )
        .await
        .unwrap();
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        0
    );
    assert!(
        store
            .conversation(&user, thread.id, None)
            .await
            .unwrap()
            .turns[0]
            .run
            .can_retry
    );
}

#[tokio::test]
async fn explicit_retry_discards_capsules_pending_slots_and_partial_results_without_resetting_budget()
 {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (_, id) = source(&store, &user, "Prior attempt source").await;
    let thread = store
        .create_thread(&user, "Fresh attempt".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    store
        .commit_agent_step(
            &step,
            capsule("private capsule from failed attempt"),
            vec![
                read(&id, 0, 4096),
                PendingKnowledgeCall {
                    call_id: "pending".into(),
                    input: KnowledgeToolInput::InvalidArguments,
                },
            ],
        )
        .await
        .unwrap();
    store
        .execute_knowledge_tool(&step, "read-1".into())
        .await
        .unwrap();
    store
        .finish_agent_execution(&claim.lease, RunState::Incomplete, None)
        .await
        .unwrap();
    store
        .retry_run(&user, step.run_id().into(), 1)
        .await
        .unwrap();
    let next = store.claim_agent_run().await.unwrap().unwrap();
    assert!(next.context.checkpoint.is_none());
    assert!(next.context.calls.is_empty());
    assert!(
        next.messages
            .iter()
            .all(|m| !m.text.contains("private capsule"))
    );
    let next_step = store
        .begin_agent_step(&next.lease, next.context.checkpoint_seq)
        .await
        .unwrap();
    assert_eq!(next_step.step(), 2);
    assert!(
        store
            .execute_knowledge_tool(&step, "pending".into())
            .await
            .is_err()
    );
    let run = step.run_id().to_owned();
    store
        .call(move |db| {
            assert_eq!(
                db.conn.query_row(
                    "SELECT tool_calls FROM agent_execution WHERE run_id=?1",
                    [&run],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM agent_dependencies WHERE run_id=?1",
                    [&run],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn correlation_ids_match_connector_and_semantic_search_failures_are_persisted() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let thread = store
        .create_thread(&user, "Semantic error".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    let long = "a".repeat(256);
    assert!(matches!(
        store
            .commit_agent_step(
                &step,
                capsule("bad id"),
                vec![PendingKnowledgeCall {
                    call_id: "non graphic id".into(),
                    input: KnowledgeToolInput::InvalidArguments
                }]
            )
            .await,
        Err(StoreError::InvalidInput)
    ));
    let mut cursor = query("search");
    cursor.cursor = Some("unsupported tool cursor".into());
    store
        .commit_agent_step(
            &step,
            capsule("semantic errors"),
            vec![
                PendingKnowledgeCall {
                    call_id: long.clone(),
                    input: KnowledgeToolInput::Search { query: query("") },
                },
                PendingKnowledgeCall {
                    call_id: "cursor".into(),
                    input: KnowledgeToolInput::Search { query: cursor },
                },
            ],
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, long.clone())
            .await
            .unwrap(),
        KnowledgeToolResult::Error(ToolReadError::InvalidInput)
    ));
    assert!(matches!(
        store.execute_knowledge_tool(&step, long).await.unwrap(),
        KnowledgeToolResult::Error(ToolReadError::InvalidInput)
    ));
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "cursor".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Error(ToolReadError::InvalidInput)
    ));
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT tool_calls FROM agent_execution", [], |r| r
                        .get::<_, i64>(0))?,
                2
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn visible_evidence_is_bounded_to_actual_delivered_refs_and_partial_usage_is_unknown() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let group = store
        .create_collection(&user, "Many sources".into(), Uuid::new_v4().to_string())
        .await
        .unwrap();
    store
        .ingest_text(
            &user,
            group.id.clone(),
            batch(
                0,
                (0..15)
                    .map(|i| input(&format!("s{i}"), &format!("topic{} private text", i / 5)))
                    .collect(),
            ),
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Provenance".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    store
        .commit_agent_step(
            &step,
            capsule("searches"),
            (0..3)
                .map(|i| PendingKnowledgeCall {
                    call_id: format!("search{i}"),
                    input: KnowledgeToolInput::Search {
                        query: query(&format!("topic{i}")),
                    },
                })
                .collect(),
        )
        .await
        .unwrap();
    for i in 0..3 {
        store
            .execute_knowledge_tool(&step, format!("search{i}"))
            .await
            .unwrap();
    }
    assert!(
        store
            .conversation(&user, thread.id.clone(), None)
            .await
            .unwrap()
            .turns[0]
            .run
            .evidence
            .is_empty()
    );
    finish(&store, &claim, "Answer with consulted sources").await;
    store
        .update_agent_step_usage(
            &step,
            RunUsage {
                input_tokens: Some(4),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let page = store.conversation(&user, thread.id, None).await.unwrap();
    assert_eq!(page.turns[0].run.evidence.len(), 12);
    assert!(
        page.turns[0]
            .run
            .evidence
            .iter()
            .all(|e| e.kind == KnowledgeKind::Source
                && e.revision == 1
                && e.title == "Synthetic source")
    );
    assert!(page.turns[0].run.usage.input_tokens.is_none());
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM agent_dependencies", [], |r| r
                        .get::<_, i64>(0))?,
                15
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn generation_change_fences_inflight_output_without_affecting_plain_text_lease() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (_, id) = source(&store, &user, "Consulted source").await;
    let thread = store
        .create_thread(&user, "Generation".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let read = tool_step(&store, &claim, read(&id, 0, 4096)).await;
    store
        .execute_knowledge_tool(&read, "read-1".into())
        .await
        .unwrap();
    let context = store.agent_context(&claim.lease).await.unwrap();
    let step = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            draft("Unrelated addition changes generation"),
        )
        .await
        .unwrap();
    assert!(
        store
            .append_agent_output(&step, patch("stale in-flight generation"))
            .await
            .is_err()
    );
    let plain_thread = store
        .create_thread(&user, "No knowledge".into())
        .await
        .unwrap();
    let plain = claimed(&store, &user, &plain_thread.id).await;
    let plain_step = store.begin_agent_step(&plain.lease, 0).await.unwrap();
    store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            draft("Another unrelated addition"),
        )
        .await
        .unwrap();
    store
        .append_agent_output(&plain_step, patch("Plain response remains valid"))
        .await
        .unwrap();
    store
        .commit_agent_step(&plain_step, capsule("plain"), vec![])
        .await
        .unwrap();
    store
        .finish_agent_run(&plain_step, RunState::Completed, None)
        .await
        .unwrap();
    assert_eq!(
        store
            .conversation(&user, plain_thread.id, None)
            .await
            .unwrap()
            .turns[0]
            .assistant[0]
            .text,
        "Plain response remains valid"
    );
}

#[tokio::test]
async fn browse_cursor_query_mistake_persists_typed_error_and_can_be_repaired() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    for index in 0..6 {
        store
            .create_memory(
                &user,
                Uuid::new_v4().to_string(),
                draft(&format!("Synthetic memory {index}")),
            )
            .await
            .unwrap();
    }
    let thread = store
        .create_thread(&user, "Cursor repair".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let mut browse = query("");
    browse.kind = KnowledgeKind::Memory;
    let first = tool_step(
        &store,
        &claim,
        PendingKnowledgeCall {
            call_id: "browse".into(),
            input: KnowledgeToolInput::Search {
                query: browse.clone(),
            },
        },
    )
    .await;
    let page = match store
        .execute_knowledge_tool(&first, "browse".into())
        .await
        .unwrap()
    {
        KnowledgeToolResult::Search(page) => page,
        _ => panic!("expected browse page"),
    };
    assert_eq!(page.hits.len(), 5);
    browse.cursor = page.next_cursor;
    assert!(browse.cursor.is_some());
    let context = store.agent_context(&claim.lease).await.unwrap();
    let mistaken = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    let mut wrong_subject = browse.clone();
    wrong_subject.subject = Some("Different subject".into());
    store
        .commit_agent_step(
            &mistaken,
            capsule("mistaken cursor"),
            vec![PendingKnowledgeCall {
                call_id: "wrong-subject".into(),
                input: KnowledgeToolInput::Search {
                    query: wrong_subject,
                },
            }],
        )
        .await
        .unwrap();
    for _ in 0..2 {
        assert!(matches!(
            store
                .execute_knowledge_tool(&mistaken, "wrong-subject".into())
                .await
                .unwrap(),
            KnowledgeToolResult::Error(ToolReadError::InvalidInput)
        ));
    }
    let persisted = store.agent_context(&claim.lease).await.unwrap();
    assert!(matches!(
        persisted.calls.last().unwrap().result,
        Some(KnowledgeToolResult::Error(ToolReadError::InvalidInput))
    ));
    let repaired = store
        .begin_agent_step(&claim.lease, persisted.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &repaired,
            capsule("corrected cursor"),
            vec![PendingKnowledgeCall {
                call_id: "corrected".into(),
                input: KnowledgeToolInput::Search { query: browse },
            }],
        )
        .await
        .unwrap();
    match store
        .execute_knowledge_tool(&repaired, "corrected".into())
        .await
        .unwrap()
    {
        KnowledgeToolResult::Search(page) => {
            assert_eq!(page.hits.len(), 1);
            assert!(page.next_cursor.is_none());
        }
        _ => panic!("corrected cursor did not reach remaining memory"),
    }
    store
        .call(|db| {
            let used: (i64, i64) = db.conn.query_row(
                "SELECT tool_calls,remote_calls FROM agent_execution",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            assert_eq!(used, (3, 3));
            Ok(())
        })
        .await
        .unwrap();
    let context = store.agent_context(&claim.lease).await.unwrap();
    let final_step = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(&final_step, capsule("repaired answer"), vec![])
        .await
        .unwrap();
    store
        .finish_agent_run(&final_step, RunState::Completed, None)
        .await
        .unwrap();
    let completed = store
        .conversation(&user, thread.id, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(completed.run.state, RunState::Completed);
    assert!(completed.run.error.is_none());
}
