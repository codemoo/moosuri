use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
fn new(body: &str) -> ArtifactWrite {
    ArtifactWrite {
        request_id: Uuid::new_v4().to_string(),
        id: None,
        expected_revision: None,
        draft: ArtifactDraft {
            title: "합성 보고서".into(),
            body: body.into(),
            payload: ArtifactPayload::Document,
            references: vec![],
        },
    }
}
#[tokio::test]
async fn durable_versions_receipts_and_stale_editor_cas() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let input = new("견적 A 10, B 12. A가 2 낮음.");
    let a = store.write_artifact(&user, input.clone()).await.unwrap();
    let duplicate = store.write_artifact(&user, input.clone()).await.unwrap();
    assert_eq!(a.id, duplicate.id);
    let mut changed = input;
    changed.draft.body = "다른 요청".into();
    assert!(matches!(
        store.write_artifact(&user, changed).await,
        Err(StoreError::Conflict)
    ));
    let mut edit = new("확인한 수정");
    edit.id = Some(a.id.clone());
    edit.expected_revision = Some(1);
    let b = store.write_artifact(&user, edit.clone()).await.unwrap();
    assert_eq!(b.revision, 2);
    edit.request_id = Uuid::new_v4().to_string();
    edit.draft.body = "보존할 로컬 초안".into();
    assert!(matches!(
        store.write_artifact(&user, edit).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store
            .artifact(&user, a.id.clone(), None)
            .await
            .unwrap()
            .draft
            .unwrap()
            .body,
        "확인한 수정"
    );
    assert_eq!(
        store
            .artifact_history(&user, a.id.clone(), None)
            .await
            .unwrap()
            .items
            .len(),
        2
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap(); // Restart persistence uses the same captured synthetic session.
    let persisted = store.artifact(&user, a.id, None).await.unwrap();
    assert_eq!(persisted.artifact.revision, 2);
}
#[tokio::test]
async fn administrator_and_member_cannot_read_write_or_export_each_others_records() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await.user;
    let member = member(&store, &admin, "member@example.test").await.user;
    let a = store
        .write_artifact(&member, new("member private"))
        .await
        .unwrap();
    assert!(
        store
            .artifacts(&admin, None, None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(matches!(
        store.artifact(&admin, a.id.clone(), None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.export_artifact(&admin, a.id.clone(), 1, false).await,
        Err(StoreError::NotFound)
    ));
    let mut input = new("admin overwrite");
    input.id = Some(a.id);
    input.expected_revision = Some(1);
    assert!(matches!(
        store.write_artifact(&admin, input).await,
        Err(StoreError::NotFound)
    ));
}
#[tokio::test]
async fn memory_and_transitive_artifact_withdrawal_purge_history_but_keep_independent_text() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let memory = store
        .create_memory(
            &u,
            Uuid::new_v4().to_string(),
            MemoryDraft {
                text: "합성 비공개 사실".into(),
                subject: "합성".into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![],
            },
        )
        .await
        .unwrap();
    let mut first = new("derived secret");
    first.draft.references.push(ArtifactDependency {
        kind: ArtifactDependencyKind::Memory,
        id: memory.id.clone(),
        revision: 1,
    });
    let a = store.write_artifact(&u, first).await.unwrap();
    let mut next = new("transitive secret");
    next.draft.references.push(ArtifactDependency {
        kind: ArtifactDependencyKind::Artifact,
        id: a.id.clone(),
        revision: 1,
    });
    let b = store.write_artifact(&u, next).await.unwrap();
    let independent = store
        .write_artifact(&u, new("independent assertion"))
        .await
        .unwrap();
    let mut edit = new("manually edited derived secret");
    edit.id = Some(a.id.clone());
    edit.expected_revision = Some(1);
    let a2 = store.write_artifact(&u, edit).await.unwrap();
    // Editing a parent invalidates dependent current text; editor replacement retains provenance.
    assert!(
        store
            .artifact(&u, b.id.clone(), None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
    store.forget_memory(&u, memory.id, 1).await.unwrap();
    assert!(
        store
            .artifact(&u, a.id.clone(), None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
    assert_eq!(a2.revision, 2);
    for v in store
        .artifact_history(&u, a.id.clone(), None)
        .await
        .unwrap()
        .items
    {
        assert!(v.draft.is_none())
    }
    assert!(matches!(
        store.export_artifact(&u, a.id, 1, true).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        store
            .artifact(&u, independent.id, None)
            .await
            .unwrap()
            .draft
            .unwrap()
            .body,
        "independent assertion"
    );
}
#[tokio::test]
async fn adopted_artifact_delete_replays_exact_receipt() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let a = store
        .write_artifact(&user, new("purge from every version"))
        .await
        .unwrap();
    let deletion = ArtifactDelete {
        request_id: Uuid::new_v4().to_string(),
        expected_revision: 1,
    };
    let tenant = user.tenant.0.clone();
    let target = a.id.clone();
    let request = deletion.request_id.clone();
    store
        .call(move |db| {
            let p = db.destructive_journal.prepare(
                &tenant,
                destructive_intents::Effect::DeleteArtifact {
                    target,
                    request,
                    revision: 1,
                },
            )?;
            db.destructive_journal.append(p)?;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let r = store
        .delete_artifact(&user, a.id.clone(), deletion.clone())
        .await
        .unwrap();
    assert_eq!(r.revision, 2);
    assert_eq!(r.status, ArtifactStatus::Deleted);
    let again = store
        .delete_artifact(&user, a.id.clone(), deletion)
        .await
        .unwrap();
    assert_eq!(r.request_id, again.request_id);
    assert!(
        store
            .artifact(&user, a.id, None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
}
#[tokio::test]
async fn honest_briefing_filters_lifecycle_mute_and_snooze_and_safe_exports_are_literal() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    for n in 0..9 {
        let mut input = new("No message sent; local record only");
        input.draft.title = format!("request {n}");
        input.draft.payload = ArtifactPayload::Followup {
            record_kind: FollowupKind::WaitingForReply,
            person: "Reported label".into(),
            lifecycle: if n == 0 {
                FollowupLifecycle::Resolved
            } else {
                FollowupLifecycle::Waiting
            },
            progress: if n == 1 {
                FollowupProgress::Delegated
            } else {
                FollowupProgress::Partial
            },
            muted: n == 2,
            due: None,
            review: None,
            snooze: if n == 3 {
                Some(ArtifactDate {
                    utc: now().unwrap() + 3600,
                    timezone: "UTC".into(),
                })
            } else {
                None
            },
        };
        store.write_artifact(&u, input).await.unwrap();
    }
    let p = store.followup_briefing(&u, None).await.unwrap();
    assert_eq!(p.items.len(), 5);
    assert!(!p.coverage_complete);
    let exported = store
        .export_artifact(&u, p.items[0].id.clone(), p.items[0].revision, false)
        .await
        .unwrap();
    assert!(exported.contains("Reported label") && exported.contains("waiting_for_reply"));
    let more = store.followup_briefing(&u, p.next_after).await.unwrap();
    assert_eq!(more.items.len(), 1);
    assert!(more.coverage_complete);
    assert!(
        !p.items
            .iter()
            .any(|r| ["request 0", "request 2", "request 3"].contains(&r.title.as_str()))
    );
    let x=store.write_artifact(&u,new("<script>alert(1)</script> ![x](https://private.invalid/x) [go](javascript:bad) <img onerror=bad>" )).await.unwrap();
    let html = store.export_artifact(&u, x.id, 1, true).await.unwrap();
    assert!(!html.contains("<script>"));
    assert!(!html.contains("<img"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("default-src 'none'"));
}
#[tokio::test]
async fn kind_bounds_and_cursor_scope_are_enforced_without_truncation() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let mut large = new(&"x".repeat(32768));
    large.draft.title = "가".repeat(66);
    let a = store.write_artifact(&u, large.clone()).await.unwrap();
    large.request_id = Uuid::new_v4().to_string();
    large.draft.body.push('x');
    assert!(matches!(
        store.write_artifact(&u, large).await,
        Err(StoreError::InvalidInput)
    ));
    let mut wrong = new("change immutable kind");
    wrong.id = Some(a.id);
    wrong.expected_revision = Some(1);
    wrong.draft.payload = ArtifactPayload::Goal {
        outcome: "outcome".into(),
        success_criteria: "measurable".into(),
        progress: "none".into(),
        lifecycle: GoalLifecycle::Active,
    };
    assert!(matches!(
        store.write_artifact(&u, wrong).await,
        Err(StoreError::Conflict)
    ));
    for n in 0..21 {
        let mut r = new("independent");
        r.draft.title = format!("record {n}");
        store.write_artifact(&u, r).await.unwrap();
    }
    let page = store
        .artifacts(&u, Some(ArtifactKind::Document), None)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 20);
    assert!(!page.coverage_complete);
    let cursor = page.next_after.unwrap();
    assert!(matches!(
        store
            .artifacts(&u, Some(ArtifactKind::Goal), Some(cursor.clone()))
            .await,
        Err(StoreError::Conflict)
    ));
    store
        .write_artifact(&u, new("new generation"))
        .await
        .unwrap();
    assert!(matches!(
        store
            .artifacts(&u, Some(ArtifactKind::Document), Some(cursor))
            .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn cyclic_derivation_rejection_and_late_sql_failure_leave_no_partial_mutation() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let a = store.write_artifact(&u, new("parent")).await.unwrap();
    let mut child = new("derived child");
    child.draft.references.push(ArtifactDependency {
        kind: ArtifactDependencyKind::Artifact,
        id: a.id.clone(),
        revision: 1,
    });
    let b = store.write_artifact(&u, child).await.unwrap();
    let mut looped = new("must not purge child or create this version");
    looped.id = Some(a.id.clone());
    looped.expected_revision = Some(1);
    looped.draft.references.push(ArtifactDependency {
        kind: ArtifactDependencyKind::Artifact,
        id: b.id.clone(),
        revision: 1,
    });
    assert!(matches!(
        store.write_artifact(&u, looped).await,
        Err(StoreError::InvalidInput)
    ));
    assert!(
        store
            .artifact(&u, b.id.clone(), None)
            .await
            .unwrap()
            .draft
            .is_some()
    );
    let tenant = u.tenant.0.clone();
    let target = a.id.clone();
    store.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;tx.execute_batch("CREATE TEMP TRIGGER fail_artifact_version BEFORE INSERT ON main.personal_artifact_versions BEGIN SELECT RAISE(ABORT,'synthetic constraint'); END;")?;let mut input=new("failed partial mutation");input.id=Some(target.clone());input.expected_revision=Some(1);assert!(write(&tx,&tenant,input,vec![],None).is_err());tx.execute_batch("DROP TRIGGER fail_artifact_version")?;tx.commit()?;Ok(())}).await.unwrap();
    assert_eq!(
        store
            .artifact(&u, a.id, None)
            .await
            .unwrap()
            .draft
            .unwrap()
            .body,
        "parent"
    );
    assert_eq!(
        store
            .artifact(&u, b.id, None)
            .await
            .unwrap()
            .draft
            .unwrap()
            .body,
        "derived child"
    );
}
#[tokio::test]
async fn sealed_write_refuses_external_context_until_its_withdrawal_edges_are_supported() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let p = store
        .create_provider(
            &u,
            ProviderChange {
                name: "synthetic no-I/O binding".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&u, p.id.clone(), p.revision, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(&u, p.id.clone(), p.revision, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &u,
            Some(ModelSelection {
                provider_id: p.id,
                provider_revision: p.revision,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let t = store
        .create_thread(&u, "synthetic context".into())
        .await
        .unwrap()
        .id;
    store
        .submit_turn(
            &u,
            t,
            Uuid::new_v4().to_string(),
            "보고서로 저장해 줘".into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    let ctx = store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    store
        .prepare_artifact_intent(
            &step,
            Some(ArtifactIntent {
                action: "create".into(),
                kind: ArtifactKind::Document,
                intent_quote: "보고서로 저장해 줘".into(),
                target_quote: None,
            }),
        )
        .await
        .unwrap();
    let tenant = u.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    store
        .call(move |db| {
            db.conn.execute(
                "INSERT INTO mail_run_dependencies VALUES(?1,?2,?3,0)",
                params![tenant, run, Uuid::new_v4().to_string()],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let next = store
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &next,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "record".into(),
                input: KnowledgeToolInput::Artifact {
                    request: ArtifactToolRequest::Write {
                        title: "must not persist".into(),
                        body: "external private content".into(),
                        payload: ArtifactPayload::Document,
                        anchor: None,
                    },
                },
            }],
        )
        .await
        .unwrap();
    let result = store
        .execute_knowledge_tool(&next, "record".into())
        .await
        .unwrap();
    assert!(
        matches!(result,KnowledgeToolResult::External(v) if v["error"]=="external_provenance_unavailable")
    );
    assert!(
        store
            .artifacts(&u, None, None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn overlapping_literal_anchor_occurrences_are_ambiguous_including_utf8() {
    assert!(!super::agent::unique_anchor("aaa", "aa"));
    assert!(!super::agent::unique_anchor("가가가", "가가"));
    assert!(!super::agent::unique_anchor("anything", ""));
    assert!(!super::agent::unique_anchor("no anchor", "missing"));
    assert!(super::agent::unique_anchor("left 가나 right", "가나"));
}

#[tokio::test]
async fn write_confirmation_is_atomic_and_lost_ack_reopens_completed_without_another_call() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let p = store
        .create_provider(
            &u,
            ProviderChange {
                name: "synthetic no-I/O binding".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&u, p.id.clone(), p.revision, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(&u, p.id.clone(), p.revision, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &u,
            Some(ModelSelection {
                provider_id: p.id,
                provider_revision: p.revision,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let t = store
        .create_thread(&u, "synthetic context".into())
        .await
        .unwrap()
        .id;
    store
        .submit_turn(
            &u,
            t.clone(),
            Uuid::new_v4().to_string(),
            "보고서로 저장해 줘".into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    let ctx = store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    store
        .prepare_artifact_intent(
            &step,
            Some(ArtifactIntent {
                action: "create".into(),
                kind: ArtifactKind::Document,
                intent_quote: "보고서로 저장해 줘".into(),
                target_quote: None,
            }),
        )
        .await
        .unwrap();

    let run_id = claim.lease.run_id().to_owned();
    let next = store
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &next,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "record".into(),
                input: KnowledgeToolInput::Artifact {
                    request: ArtifactToolRequest::Write {
                        title: "Durable report".into(),
                        body: "Written once".into(),
                        payload: ArtifactPayload::Document,
                        anchor: None,
                    },
                },
            }],
        )
        .await
        .unwrap();
    store.call(|db|{db.conn.execute_batch("CREATE TEMP TRIGGER fail_confirmation BEFORE INSERT ON main.chat_output BEGIN SELECT RAISE(ABORT,'synthetic confirmation failure'); END;")?;Ok(())}).await.unwrap();
    assert!(
        store
            .execute_knowledge_tool(&next, "record".into())
            .await
            .is_err()
    );
    assert!(
        store
            .artifacts(&u, None, None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let failed = store.conversation(&u, t.clone(), None).await.unwrap();
    assert_eq!(failed.turns[0].run.state, RunState::Running);
    assert!(failed.turns[0].run.artifact_receipts.is_none());
    store
        .call(|db| {
            db.conn.execute_batch("DROP TRIGGER fail_confirmation")?;
            Ok(())
        })
        .await
        .unwrap();
    // Discard the tool ACK exactly where the old two-transaction implementation
    // could leave a committed record with no final answer. No Runtime/model runs.
    drop(
        store
            .execute_knowledge_tool(&next, "record".into())
            .await
            .unwrap(),
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let page = store.conversation(&u, t, None).await.unwrap();
    assert_eq!(page.turns[0].run.state, RunState::Completed);
    let receipts = page.turns[0].run.artifact_receipts.as_ref().unwrap();
    assert_eq!(receipts.len(), 1);
    let records = store.artifacts(&u, None, None).await.unwrap().items;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, receipts[0].id);
    assert_eq!(records[0].revision, 1);
    assert!(matches!(
        store.retry_run(&u, run_id, page.turns[0].run.attempt).await,
        Err(StoreError::Conflict)
    ));
    assert!(store.claim_agent_run().await.unwrap().is_none());
    assert_eq!(
        store
            .artifact(&u, records[0].id.clone(), None)
            .await
            .unwrap()
            .draft
            .unwrap()
            .body,
        "Written once"
    );
}
