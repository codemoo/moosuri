use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_core::*;

pub(super) async fn configured(store: &Store) -> UserContext {
    let user = admin(store).await.user;
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
    user
}
fn remember() -> (String, MemoryCommandIntent) {
    (
        "Remember this: I prefer green tea.".into(),
        MemoryCommandIntent {
            action: MemoryCommandAction::Remember,
            intent_quote: "Remember this".into(),
            fact_quote: Some("I prefer green tea.".into()),
            subject: Some("Drink preference".into()),
            target_quote: None,
        },
    )
}
fn correct() -> (String, MemoryCommandIntent) {
    (
        "Correct green tea to I prefer black coffee.".into(),
        MemoryCommandIntent {
            action: MemoryCommandAction::Correct,
            intent_quote: "Correct green tea".into(),
            fact_quote: Some("I prefer black coffee.".into()),
            subject: Some("Drink preference".into()),
            target_quote: Some("green tea".into()),
        },
    )
}
fn forget() -> (String, MemoryCommandIntent) {
    (
        "Forget green tea.".into(),
        MemoryCommandIntent {
            action: MemoryCommandAction::Forget,
            intent_quote: "Forget green tea".into(),
            fact_quote: None,
            subject: None,
            target_quote: Some("green tea".into()),
        },
    )
}
pub(super) fn draft(text: &str) -> MemoryDraft {
    MemoryDraft {
        text: text.into(),
        subject: "Drink preference".into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    }
}
fn preview(text: &str) -> Vec<OutputPatch> {
    vec![OutputPatch::Append {
        item: "old-preview".into(),
        text: text.into(),
        phase: None,
        refusal: false,
    }]
}
pub(super) async fn planning(
    store: &Store,
    user: &UserContext,
    text: &str,
) -> (String, AgentClaim, StepLease) {
    let thread = store.create_thread(user, "Command".into()).await.unwrap();
    store
        .submit_turn(
            user,
            thread.id.clone(),
            Uuid::new_v4().to_string(),
            text.into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: br#"{"v":1,"private_plan":"synthetic"}"#.to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    (thread.id, claim, step)
}
pub(super) async fn memory_count(store: &Store) -> i64 {
    store
        .call(|db| {
            Ok(db.conn.query_row(
                "SELECT count(*) FROM knowledge_memories WHERE status='active'",
                [],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap()
}
pub(super) fn applied_id(outcome: &MemoryCommandOutcome) -> String {
    match outcome {
        MemoryCommandOutcome::Applied {
            memory_id: Some(id),
            ..
        } => id.clone(),
        _ => panic!("expected applied receipt"),
    }
}

#[tokio::test]
async fn remember_stable_receipt_reopen_literal_text_and_deleted_chat_do_not_duplicate() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (text, intent) = remember();
    let (thread, claim, step) = planning(&store, &user, &text).await;
    store
        .append_agent_output(&step, preview("Discard this untrusted old preview"))
        .await
        .unwrap();
    let cursor = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .cursor;
    let command = store
        .prepare_memory_command(&step, intent.clone())
        .await
        .unwrap();
    assert!(
        store
            .begin_agent_step(
                &claim.lease,
                store
                    .agent_context(&claim.lease)
                    .await
                    .unwrap()
                    .checkpoint_seq
            )
            .await
            .is_err()
    );
    assert!(
        store
            .append_agent_output(&step, preview("late preview"))
            .await
            .is_err()
    );
    assert!(matches!(
        store
            .finish_agent_run(&step, RunState::Completed, None)
            .await,
        Err(StoreError::Conflict)
    ));
    let repeated = store
        .prepare_memory_command(&step, intent.clone())
        .await
        .unwrap();
    assert_eq!(command.command, repeated.command);
    let mut changed = intent;
    changed.subject = Some("Changed subject".into());
    assert!(matches!(
        store.prepare_memory_command(&step, changed).await,
        Err(StoreError::Conflict)
    ));
    let outcome = store.execute_memory_command(&command).await.unwrap();
    let id = applied_id(&outcome);
    assert!(store.execute_memory_command(&command).await.unwrap() == outcome);
    assert_eq!(memory_count(&store).await, 1);
    let memory = store.memory(&user, id.clone(), None).await.unwrap();
    assert_eq!(memory.draft.text, "I prefer green tea.");
    assert!(memory.draft.evidence.is_empty());
    assert!(memory.draft.valid_from.is_none());
    let page = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap();
    assert_eq!(page.turns[0].assistant.len(), 1);
    assert_eq!(page.turns[0].assistant[0].text, "기억을 저장했습니다.");
    assert_eq!(page.turns[0].run.state, RunState::Completed);
    assert_eq!(page.turns[0].run.evidence[0].id, id);
    assert_eq!(page.turns[0].user_text, text);
    assert!(
        store
            .chat_events(&user, thread.clone(), cursor)
            .await
            .unwrap()
            .reset
    );
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM memory_command_plans", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM agent_steps WHERE checkpoint IS NOT NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.execute_memory_command(&command).await.unwrap() == outcome);
    assert_eq!(memory_count(&store).await, 1);
    store.delete_thread(&user, thread.clone()).await.unwrap();
    assert!(store.execute_memory_command(&command).await.unwrap() == outcome);
    assert!(
        store
            .memory_command_receipt(&user, command.run_id().into())
            .await
            .unwrap()
            == Some(outcome)
    );
    assert!(matches!(
        store.conversation(&user, thread, None).await,
        Err(StoreError::NotFound)
    ));
    store
        .update_agent_step_usage(
            &step,
            RunUsage {
                input_tokens: Some(7),
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
        7
    );
}

#[tokio::test]
async fn correction_cas_and_ambiguous_or_missing_target_have_zero_memory_writes() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let target = store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            draft("I prefer green tea."),
        )
        .await
        .unwrap();
    let (text, intent) = correct();
    let (_, _, step) = planning(&store, &user, &text).await;
    let command = store.prepare_memory_command(&step, intent).await.unwrap();
    store
        .correct_memory(
            &user,
            target.id.clone(),
            1,
            Uuid::new_v4().to_string(),
            draft("Newer unrelated preference"),
        )
        .await
        .unwrap();
    let outcome = store.execute_memory_command(&command).await.unwrap();
    assert!(matches!(
        outcome,
        MemoryCommandOutcome::Clarify {
            reason: MemoryCommandClarification::TargetChanged
        }
    ));
    assert_eq!(
        store
            .memory(&user, target.id.clone(), None)
            .await
            .unwrap()
            .revision,
        2
    );
    for ambiguous in [false, true] {
        if ambiguous {
            store
                .create_memory(&user, Uuid::new_v4().to_string(), draft("green tea one"))
                .await
                .unwrap();
            store
                .create_memory(&user, Uuid::new_v4().to_string(), draft("green tea two"))
                .await
                .unwrap();
        }
        let before = memory_count(&store).await;
        let (text, intent) = forget();
        let (thread, _, step) = planning(&store, &user, &text).await;
        let command = store.prepare_memory_command(&step, intent).await.unwrap();
        let outcome = store.execute_memory_command(&command).await.unwrap();
        assert!(matches!(
            outcome,
            MemoryCommandOutcome::Clarify {
                reason: MemoryCommandClarification::NoTarget
                    | MemoryCommandClarification::AmbiguousTarget
            }
        ));
        assert_eq!(memory_count(&store).await, before);
        let p = store.conversation(&user, thread, None).await.unwrap();
        assert_eq!(p.turns[0].run.state, RunState::Completed);
        assert!(p.turns[0].run.evidence.is_empty());
    }
}

#[tokio::test]
async fn prepared_retry_resumes_original_plan_without_new_dispatch_or_new_subject() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (text, intent) = remember();
    let (thread, claim, step) = planning(&store, &user, &text).await;
    let prepared = store.prepare_memory_command(&step, intent).await.unwrap();
    store
        .finish_agent_execution(
            &claim.lease,
            RunState::OutcomeUnknown,
            Some(RunFailure::ConnectionFailed),
        )
        .await
        .unwrap();
    store
        .retry_run(&user, claim.lease.run.clone(), 1)
        .await
        .unwrap();
    let new = store.claim_agent_run().await.unwrap().unwrap();
    let resumed = store
        .resume_memory_command(&new.lease)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.command, prepared.command);
    assert!(store.execute_memory_command(&prepared).await.is_err());
    let result = store.execute_memory_command(&resumed).await.unwrap();
    assert!(matches!(
        result,
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Remember,
            ..
        }
    ));
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        1
    );
    assert_eq!(memory_count(&store).await, 1);
    assert_eq!(
        store.conversation(&user, thread, None).await.unwrap().turns[0].assistant[0].text,
        "기억을 저장했습니다."
    );
}

#[tokio::test]
async fn no_append_gap_never_autoexecutes_and_explicit_authenticated_resume_converges() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let memory = store
        .create_memory(&user, Uuid::new_v4().to_string(), draft("green tea"))
        .await
        .unwrap();
    let (text, intent) = forget();
    let (_, claim, step) = planning(&store, &user, &text).await;
    let command = store.prepare_memory_command(&step, intent).await.unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.memory(&user, memory.id.clone(), None).await.is_ok());
    assert!(
        store
            .memory_command_receipt(&user, command.run_id().into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        store.execute_memory_command(&command).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        std::fs::metadata(dir.path().join(destructive_intents::FILE_NAME))
            .unwrap()
            .len(),
        96
    );
    store.retry_run(&user, claim.lease.run, 1).await.unwrap();
    let next = store.claim_agent_run().await.unwrap().unwrap();
    let resumed = store
        .resume_memory_command(&next.lease)
        .await
        .unwrap()
        .unwrap();
    let outcome = store.execute_memory_command(&resumed).await.unwrap();
    assert!(matches!(
        outcome,
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Forget,
            ..
        }
    ));
    assert!(matches!(
        store.memory(&user, memory.id, None).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        1
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.execute_memory_command(&resumed).await.unwrap() == outcome);
}

#[tokio::test]
async fn adopted_forget_replays_after_auth_revocation_and_deleted_chat_without_recreation() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let target = store
        .create_memory(&user, Uuid::new_v4().to_string(), draft("green tea"))
        .await
        .unwrap();
    let (text, intent) = forget();
    let (thread, _, step) = planning(&store, &user, &text).await;
    let command = store.prepare_memory_command(&step, intent).await.unwrap();
    let tenant = user.tenant.0.clone();
    let id = command.command.clone();
    // Model an adopted direct-forget/SQLite-apply gap and a snapshot whose
    // conversation was already deleted. Preserve the opaque slot/receipt row.
    let deleted_thread = thread.clone();
    store
        .call(move |db| {
            destructive_intents::adopt_unapplied_for_test(db, &tenant, &id, 5)?;
            let tx = db.conn.transaction()?;
            mail::purge_thread(&tx, &tenant, &deleted_thread)?;
            tx.execute(
                "DELETE FROM threads WHERE tenant_id=?1 AND id=?2",
                params![tenant, deleted_thread],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
    store.logout(&user).await.unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let current = store
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(matches!(
        store.memory(&current, target.id, None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.conversation(&current, thread, None).await,
        Err(StoreError::NotFound)
    ));
    let outcome = store
        .memory_command_receipt(&current, command.run_id().into())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        outcome,
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Forget,
            ..
        }
    ));
    assert!(matches!(
        store.execute_memory_command(&command).await,
        Err(StoreError::Unauthorized)
    ));
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM chat_runs", [], |r| r.get::<_, i64>(0))?,
                0
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM chat_output", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM memory_command_slots WHERE state='committed'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            Ok(())
        })
        .await
        .unwrap();
    let journal = std::fs::read(dir.path().join(destructive_intents::FILE_NAME)).unwrap();
    assert!(
        !journal
            .windows(b"green tea".len())
            .any(|b| b == b"green tea")
    );
}

#[tokio::test]
async fn post_append_failure_poison_replays_the_same_forget_and_pre_append_sql_rolls_back() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let target = store
        .create_memory(&user, Uuid::new_v4().to_string(), draft("green tea"))
        .await
        .unwrap();
    let (text, intent) = forget();
    let (thread, _, step) = planning(&store, &user, &text).await;
    let command = store.prepare_memory_command(&step, intent).await.unwrap();
    store.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_command_receipt BEFORE UPDATE OF state ON memory_command_slots BEGIN SELECT RAISE(ABORT,'synthetic disk fault'); END")?;Ok(())}).await.unwrap();
    assert!(store.execute_memory_command(&command).await.is_err());
    assert!(matches!(
        store.memory(&user, target.id.clone(), None).await,
        Err(StoreError::Unavailable)
    ));
    drop(store);
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch("DROP TRIGGER fail_command_receipt")
        .unwrap();
    drop(conn);
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.memory(&user, target.id, None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.execute_memory_command(&command).await.unwrap(),
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Forget,
            ..
        }
    ));
    assert_eq!(
        store.conversation(&user, thread, None).await.unwrap().turns[0].assistant[0].text,
        "기억과 수정 이력을 지웠습니다. 원문 자료는 유지됩니다."
    );
    drop(store);
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (text, intent) = remember();
    let (_, _, step) = planning(&store, &user, &text).await;
    let command = store.prepare_memory_command(&step, intent).await.unwrap();
    store.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_command_receipt BEFORE UPDATE OF state ON memory_command_slots BEGIN SELECT RAISE(ABORT,'synthetic SQL fault'); END")?;Ok(())}).await.unwrap();
    assert!(store.execute_memory_command(&command).await.is_err());
    assert_eq!(memory_count(&store).await, 0);
    assert!(
        store
            .memory_command_receipt(&user, command.run_id().into())
            .await
            .unwrap()
            .is_none()
    );
    store
        .call(|db| {
            db.conn.execute_batch("DROP TRIGGER fail_command_receipt")?;
            Ok(())
        })
        .await
        .unwrap();
    store.execute_memory_command(&command).await.unwrap();
    assert_eq!(memory_count(&store).await, 1);
}

#[tokio::test]
async fn cancellation_expiry_provider_change_and_other_tenant_cannot_mutate_a_prepared_slot() {
    let _lock = TEST_LOCK.lock().await;
    for boundary in 0..5 {
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = configured(&store).await;
        let (text, intent) = remember();
        let (_, claim, step) = planning(&store, &user, &text).await;
        let command = store.prepare_memory_command(&step, intent).await.unwrap();
        match boundary {
            0 => {
                store.cancel_run(&user, claim.lease.run, 1).await.unwrap();
            }
            1 => {
                store
                    .call(|db| {
                        db.conn.execute("UPDATE sessions SET expires_at=0", [])?;
                        Ok(())
                    })
                    .await
                    .unwrap();
            }
            2 => {
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
            }
            3 => {
                let other = member(&store, &user, "foreign@example.test").await.user;
                let mut foreign = command.clone();
                foreign.lease.tenant = other.tenant.0.clone();
                assert!(store.execute_memory_command(&foreign).await.is_err());
                assert!(
                    store
                        .memory_command_receipt(&other, command.run_id().into())
                        .await
                        .unwrap()
                        .is_none()
                );
                store.cancel_run(&user, claim.lease.run, 1).await.unwrap();
            }
            _ => {
                store
                    .call(|db| {
                        db.conn
                            .execute("UPDATE agent_execution SET deadline_ms=0", [])?;
                        Ok(())
                    })
                    .await
                    .unwrap();
            }
        }
        assert!(store.execute_memory_command(&command).await.is_err());
        assert_eq!(memory_count(&store).await, 0);
        store
            .call(|db| {
                assert_eq!(
                    db.conn.query_row(
                        "SELECT count(*) FROM memory_command_slots WHERE state='committed'",
                        [],
                        |r| r.get::<_, i64>(0)
                    )?,
                    0
                );
                Ok(())
            })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn retrieved_knowledge_and_nonliteral_or_oversized_plans_cannot_authorize_commands() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let (text, intent) = remember();
    let (_, claim, step) = planning(&store, &user, &text).await;
    assert!(claim.context.memory_planning_allowed);
    for which in 0..6 {
        let mut bad = intent.clone();
        match which {
            0 => bad.intent_quote = "not in current user message".into(),
            1 => bad.fact_quote = Some("injected retrieved source instruction".into()),
            2 => bad.subject = Some("가".repeat(67)),
            3 => bad.target_quote = Some("unexpected selector".into()),
            4 => bad.intent_quote = " ".into(),
            _ => bad.subject = Some("control\nlabel".into()),
        }
        assert!(matches!(
            store.prepare_memory_command(&step, bad).await,
            Err(StoreError::InvalidInput)
        ));
    }
    let context = store.agent_context(&claim.lease).await.unwrap();
    let knowledge = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    let query = KnowledgeQuery {
        kind: KnowledgeKind::Memory,
        text: "green".into(),
        collection_id: None,
        subject: None,
        speaker: None,
        valid_at: None,
        cursor: None,
    };
    store
        .commit_agent_step(
            &knowledge,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "search".into(),
                input: KnowledgeToolInput::Search { query },
            }],
        )
        .await
        .unwrap();
    store
        .execute_knowledge_tool(&knowledge, "search".into())
        .await
        .unwrap();
    let context = store.agent_context(&claim.lease).await.unwrap();
    let later = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &later,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    assert!(matches!(
        store.prepare_memory_command(&later, intent).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(memory_count(&store).await, 0);
    assert!(
        !store
            .agent_context(&claim.lease)
            .await
            .unwrap()
            .memory_planning_allowed
    );
    store
        .finish_agent_run(&later, RunState::Failed, None)
        .await
        .unwrap();
    store
        .retry_run(&user, claim.lease.run.clone(), claim.lease.attempt())
        .await
        .unwrap();
    let retried = store.claim_agent_run().await.unwrap().unwrap();
    assert!(retried.context.calls.is_empty());
    assert!(!retried.context.memory_planning_allowed);
}

#[tokio::test]
async fn self_invalidating_correction_replaces_old_context_with_independent_receipt_confirmation() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let collection = store
        .create_collection(&user, "Synthetic source".into(), Uuid::new_v4().to_string())
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
                    external_id: "source".into(),
                    version: "1".into(),
                    expected_revision: None,
                    title: "Old source".into(),
                    text: "green tea source evidence".into(),
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
    let mut old = draft("I prefer green tea.");
    old.evidence.push(EvidenceRef {
        source_id: source.clone(),
        source_revision: 1,
        start_byte: 0,
        end_byte: 9,
        quote: "green tea".into(),
    });
    let target = store
        .create_memory(&user, Uuid::new_v4().to_string(), old)
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Inherited context".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &user,
            thread.id.clone(),
            Uuid::new_v4().to_string(),
            "Recall my beverage".into(),
        )
        .await
        .unwrap();
    let first = store.claim_agent_run().await.unwrap().unwrap();
    let read = store.begin_agent_step(&first.lease, 0).await.unwrap();
    store
        .commit_agent_step(
            &read,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "read-memory".into(),
                input: KnowledgeToolInput::ReadMemory {
                    id: target.id.clone(),
                    revision: 1,
                },
            }],
        )
        .await
        .unwrap();
    store
        .execute_knowledge_tool(&read, "read-memory".into())
        .await
        .unwrap();
    let context = store.agent_context(&first.lease).await.unwrap();
    let answer = store
        .begin_agent_step(&first.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .append_agent_output(&answer, preview("Old source-dependent answer"))
        .await
        .unwrap();
    store
        .commit_agent_step(
            &answer,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    store
        .finish_agent_run(&answer, RunState::Completed, None)
        .await
        .unwrap();
    let (text, intent) = correct();
    store
        .submit_turn(&user, thread.id.clone(), Uuid::new_v4().to_string(), text)
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    assert!(
        claim
            .messages
            .iter()
            .any(|m| m.text == "Old source-dependent answer")
    );
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .append_agent_output(&step, preview("Obsolete planning preview"))
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: b"{\"private\":\"obsolete\"}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    let cursor = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap()
        .cursor;
    let command = store.prepare_memory_command(&step, intent).await.unwrap();
    let outcome = store.execute_memory_command(&command).await.unwrap();
    assert!(matches!(
        outcome,
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Correct,
            revision: Some(2),
            ..
        }
    ));
    let page = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap();
    assert!(page.turns[0].assistant.is_empty());
    let result = &page.turns[1];
    assert_eq!(result.assistant.len(), 1);
    assert_eq!(result.assistant[0].text, "기억을 바로잡았습니다.");
    assert_eq!(result.run.state, RunState::Completed);
    assert_eq!(result.run.evidence.len(), 1);
    assert_eq!(result.run.evidence[0].kind, KnowledgeKind::Memory);
    assert_eq!(result.run.evidence[0].id, target.id);
    assert_eq!(result.run.evidence[0].revision, 2);
    assert!(
        store
            .chat_events(&user, thread.id.clone(), cursor)
            .await
            .unwrap()
            .reset
    );
    store.delete_source(&user, source, 1).await.unwrap();
    assert!(matches!(
        store.memory(&user, target.id.clone(), Some(1)).await,
        Err(StoreError::NotFound)
    ));
    let memory = store.memory(&user, target.id.clone(), None).await.unwrap();
    assert_eq!(memory.draft.text, "I prefer black coffee.");
    assert!(memory.draft.evidence.is_empty());
    let page = store.conversation(&user, thread.id, None).await.unwrap();
    assert_eq!(page.turns[1].assistant[0].text, "기억을 바로잡았습니다.");
    assert_eq!(page.turns[1].run.evidence[0].revision, 2);
    assert!(store.execute_memory_command(&command).await.unwrap() == outcome);
    store
        .update_agent_step_usage(
            &step,
            RunUsage {
                input_tokens: Some(12),
                output_tokens: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let usage = store.usage_summary(&user).await.unwrap();
    assert_eq!(usage.dispatched_calls, 3);
    assert_eq!(usage.reported_input_tokens, 12);
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
                    .query_row("SELECT count(*) FROM memory_command_plans", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn literal_unicode_fact_and_bounds_are_preserved_without_inferred_dates_or_extra_fields() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let fact = "나는 🌱 녹차를\n좋아한다.";
    let text = format!("기억해 줘: {fact}");
    let intent = MemoryCommandIntent {
        action: MemoryCommandAction::Remember,
        intent_quote: "기억해 줘".into(),
        fact_quote: Some(fact.into()),
        subject: Some("내 음료 취향".into()),
        target_quote: None,
    };
    let (_, _, step) = planning(&store, &user, &text).await;
    let command = store.prepare_memory_command(&step, intent).await.unwrap();
    let id = applied_id(&store.execute_memory_command(&command).await.unwrap());
    let memory = store.memory(&user, id, None).await.unwrap();
    assert_eq!(memory.draft.text, fact);
    assert!(memory.draft.speaker.is_none());
    assert!(memory.draft.valid_from.is_none() && memory.draft.valid_until.is_none());
    assert!(serde_json::from_value::<MemoryCommandIntent>(serde_json::json!({"action":"remember","intent_quote":"x","fact_quote":"x","subject":"x","target_quote":null,"evidence":[]})).is_err());
    let oversized = "x".repeat(4097);
    let text = format!("Remember {oversized}");
    let (_, _, step) = planning(&store, &user, &text).await;
    let intent = MemoryCommandIntent {
        action: MemoryCommandAction::Remember,
        intent_quote: "Remember".into(),
        fact_quote: Some(oversized),
        subject: Some("subject".into()),
        target_quote: None,
    };
    assert!(matches!(
        store.prepare_memory_command(&step, intent).await,
        Err(StoreError::InvalidInput)
    ));
    store
        .finish_agent_run(&step, RunState::Failed, None)
        .await
        .unwrap();
    let target = "가".repeat(43);
    let text = format!("Forget {target}");
    let (_, _, step) = planning(&store, &user, &text).await;
    let intent = MemoryCommandIntent {
        action: MemoryCommandAction::Forget,
        intent_quote: "Forget".into(),
        fact_quote: None,
        subject: None,
        target_quote: Some(target),
    };
    assert!(matches!(
        store.prepare_memory_command(&step, intent).await,
        Err(StoreError::InvalidInput)
    ));
}

#[tokio::test]
async fn journal_adopted_before_target_expiry_replays_after_expiry_without_new_eligibility() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let expires = now().unwrap() + 2;
    let mut value = draft("green tea");
    value.valid_until = Some(expires);
    let target = store
        .create_memory(&user, Uuid::new_v4().to_string(), value)
        .await
        .unwrap();
    let (text, intent) = forget();
    let (thread, _, step) = planning(&store, &user, &text).await;
    let prepared = store.prepare_memory_command(&step, intent).await.unwrap();
    let tenant = user.tenant.0.clone();
    let command = prepared.command.clone();
    let run = prepared.run_id().to_owned();
    store
        .call(move |db| {
            let value = slot(&db.conn, &tenant, &run)?.ok_or(StoreError::Corrupt)?;
            assert!(value.clarification.is_none());
            assert!(current_target(&db.conn, &tenant, &value, "green tea")?);
            destructive_intents::adopt_unapplied_for_test(db, &tenant, &command, 5)?;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    tokio::time::sleep(std::time::Duration::from_secs(
        (expires - now().unwrap()).max(0) as u64 + 1,
    ))
    .await;
    assert!(now().unwrap() > expires);
    let store = Store::open(dir.path()).expect("adopted forget must not get stuck on later expiry");
    assert!(matches!(
        store.memory(&user, target.id, None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.execute_memory_command(&prepared).await.unwrap(),
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Forget,
            ..
        }
    ));
    assert_eq!(
        store.conversation(&user, thread, None).await.unwrap().turns[0].assistant[0].text,
        "기억과 수정 이력을 지웠습니다. 원문 자료는 유지됩니다."
    );
}

fn forget_green_tea() -> (String, MemoryCommandIntent) {
    (
        "녹차 기억을 잊어 줘.".into(),
        MemoryCommandIntent {
            action: MemoryCommandAction::Forget,
            intent_quote: "잊어 줘".into(),
            fact_quote: None,
            subject: None,
            target_quote: Some("녹차".into()),
        },
    )
}
async fn future_green_tea(store: &Store, user: &UserContext, begins: i64, text: &str) -> String {
    let first = store
        .create_memory(user, Uuid::new_v4().to_string(), draft("녹차"))
        .await
        .unwrap();
    let mut future = draft(text);
    future.valid_from = Some(begins);
    store
        .correct_memory(
            user,
            first.id.clone(),
            1,
            Uuid::new_v4().to_string(),
            future,
        )
        .await
        .unwrap();
    first.id
}

#[tokio::test]
async fn effective_past_revision_and_second_current_match_remain_ambiguous_without_writes() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let a = future_green_tea(&store, &user, now().unwrap() + 3600, "커피").await;
    let b = store
        .create_memory(&user, Uuid::new_v4().to_string(), draft("녹차"))
        .await
        .unwrap();
    let search = store
        .search_knowledge(
            &user,
            KnowledgeQuery {
                kind: KnowledgeKind::Memory,
                text: "녹차".into(),
                collection_id: None,
                subject: None,
                speaker: None,
                valid_at: None,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(search.hits.len(), 2);
    let (text, intent) = forget_green_tea();
    let (_, _, step) = planning(&store, &user, &text).await;
    let prepared = store.prepare_memory_command(&step, intent).await.unwrap();
    let result = store.execute_memory_command(&prepared).await.unwrap();
    assert!(matches!(
        result,
        MemoryCommandOutcome::Clarify {
            reason: MemoryCommandClarification::AmbiguousTarget
        }
    ));
    assert_eq!(
        store.memory(&user, a, Some(1)).await.unwrap().draft.text,
        "녹차"
    );
    assert_eq!(store.memory(&user, b.id, None).await.unwrap().revision, 1);
    assert_eq!(memory_count(&store).await, 2);
    assert_eq!(
        std::fs::metadata(dir.path().join(destructive_intents::FILE_NAME))
            .unwrap()
            .len(),
        96
    );
}

#[tokio::test]
async fn a_unique_effective_old_revision_uses_the_separate_future_head_for_forget_cas() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let id = future_green_tea(&store, &user, now().unwrap() + 3600, "커피").await;
    let (text, intent) = forget_green_tea();
    let (_, _, step) = planning(&store, &user, &text).await;
    let prepared = store.prepare_memory_command(&step, intent).await.unwrap();
    let tenant = user.tenant.0.clone();
    let run = prepared.run_id().to_owned();
    let expected = id.clone();
    store
        .call(move |db| {
            let value = slot(&db.conn, &tenant, &run)?.ok_or(StoreError::Corrupt)?;
            assert_eq!(value.target.as_deref(), Some(expected.as_str()));
            assert_eq!(value.revision, Some(2));
            assert_eq!(value.effective_revision, Some(1));
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store.execute_memory_command(&prepared).await.unwrap(),
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Forget,
            revision: Some(3),
            ..
        }
    ));
    assert!(matches!(
        store.memory(&user, id, None).await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn a_clock_activated_effective_revision_is_fenced_even_when_head_and_query_match_stay_the_same()
 {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let begins = now().unwrap() + 2;
    let id = future_green_tea(&store, &user, begins, "녹차와 커피를 좋아한다.").await;
    let (text, intent) = forget_green_tea();
    let (_, _, step) = planning(&store, &user, &text).await;
    let prepared = store.prepare_memory_command(&step, intent).await.unwrap();
    let tenant = user.tenant.0.clone();
    let run = prepared.run_id().to_owned();
    store
        .call(move |db| {
            let value = slot(&db.conn, &tenant, &run)?.ok_or(StoreError::Corrupt)?;
            assert_eq!(value.revision, Some(2));
            assert_eq!(value.effective_revision, Some(1));
            assert!(current_target_at(
                &db.conn,
                &tenant,
                &value,
                "녹차",
                begins - 1
            )?);
            assert!(!current_target_at(
                &db.conn, &tenant, &value, "녹차", begins
            )?);
            let (target, reason) = resolve_at(&db.conn, &tenant, "녹차", begins)?;
            assert!(reason.is_none());
            assert_eq!(target.unwrap().2, 2);
            Ok(())
        })
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_secs(
        (begins - now().unwrap()).max(0) as u64 + 1,
    ))
    .await;
    assert!(matches!(
        store.execute_memory_command(&prepared).await.unwrap(),
        MemoryCommandOutcome::Clarify {
            reason: MemoryCommandClarification::TargetChanged
        }
    ));
    assert_eq!(store.memory(&user, id, None).await.unwrap().revision, 2);
    assert_eq!(
        std::fs::metadata(dir.path().join(destructive_intents::FILE_NAME))
            .unwrap()
            .len(),
        96
    );
}

#[tokio::test]
async fn an_active_effective_revision_can_be_forgotten_with_an_invalidated_future_head() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let group = store
        .create_collection(&user, "Future evidence".into(), Uuid::new_v4().to_string())
        .await
        .unwrap();
    let input = SourceInput {
        external_id: "future".into(),
        version: "1".into(),
        expected_revision: None,
        title: "Future source".into(),
        text: "coffee".into(),
        speaker: None,
        observed_at: 100,
        valid_from: None,
        valid_until: None,
    };
    let receipt = store
        .ingest_text(
            &user,
            group.id.clone(),
            IngestBatch {
                request_id: Uuid::new_v4().to_string(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries: vec![input.clone()],
            },
        )
        .await
        .unwrap();
    let first = store
        .create_memory(&user, Uuid::new_v4().to_string(), draft("녹차"))
        .await
        .unwrap();
    let mut future = draft("커피");
    future.valid_from = Some(now().unwrap() + 3600);
    future.evidence.push(EvidenceRef {
        source_id: receipt.source_ids[0].clone(),
        source_revision: 1,
        start_byte: 0,
        end_byte: 6,
        quote: "coffee".into(),
    });
    store
        .correct_memory(
            &user,
            first.id.clone(),
            1,
            Uuid::new_v4().to_string(),
            future,
        )
        .await
        .unwrap();
    let mut changed = input;
    changed.version = "2".into();
    changed.expected_revision = Some(1);
    changed.text = "replaced coffee evidence".into();
    store
        .ingest_text(
            &user,
            group.id,
            IngestBatch {
                request_id: Uuid::new_v4().to_string(),
                expected_cursor: 1,
                next_cursor: 2,
                complete_snapshot: false,
                entries: vec![changed],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .memory(&user, first.id.clone(), None)
            .await
            .unwrap()
            .status,
        MemoryStatus::Invalidated
    );
    let (text, intent) = forget_green_tea();
    let (_, _, step) = planning(&store, &user, &text).await;
    let prepared = store.prepare_memory_command(&step, intent).await.unwrap();
    assert!(matches!(
        store.execute_memory_command(&prepared).await.unwrap(),
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Forget,
            revision: Some(3),
            ..
        }
    ));
    assert!(matches!(
        store.memory(&user, first.id, None).await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn successor_forget_records_direct_target_independent_of_post_snapshot_command_slot() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let memory = store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            draft("I prefer green tea."),
        )
        .await
        .unwrap();
    let snapshot = directory();
    let path = snapshot.path().join("before-command.sqlite");
    let path_copy = path.clone();
    store
        .call(move |db| {
            let mut saved = Connection::open(path_copy)?;
            let backup = rusqlite::backup::Backup::new(&db.conn, &mut saved)?;
            backup.run_to_completion(128, std::time::Duration::from_millis(1), None)?;
            Ok(())
        })
        .await
        .unwrap();
    let (text, intent) = forget();
    let (_, _, step) = planning(&store, &user, &text).await;
    let prepared = store.prepare_memory_command(&step, intent).await.unwrap();
    store.execute_memory_command(&prepared).await.unwrap();
    let expected = memory.id.clone();
    let direct = store
        .call(move |db| {
            assert!(db.knowledge_journal.records.is_empty());
            let record = db.destructive_journal.records.last().unwrap();
            match &record.effect {
                destructive_intents::Effect::ForgetCommand {
                    target,
                    command,
                    revision,
                    ..
                } => {
                    assert_eq!(target, &expected);
                    assert_ne!(target, command);
                    assert_eq!(*revision, 1);
                    Ok(target.clone())
                }
                _ => panic!("missing direct forget record"),
            }
        })
        .await
        .unwrap();
    let saved = Connection::open(path).unwrap();
    assert_eq!(
        saved
            .query_row("SELECT count(*) FROM memory_command_slots", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    let tx = saved.unchecked_transaction().unwrap();
    knowledge::destructive::forget(&tx, &user.tenant.0, &direct).unwrap();
    tx.commit().unwrap();
    assert_eq!(
        saved
            .query_row(
                "SELECT status FROM knowledge_memories WHERE id=?1",
                [direct],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "forgotten"
    );
    // This is only a component proof of the direct mapping, not a restore API.
}
