use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};
fn tokens() -> TokenSet {
    TokenSet {
        access_token: SecretString::new("SYNTHETIC-GMAIL-ACCESS-CANARY".into()).unwrap(),
        refresh_token: Some(SecretString::new("SYNTHETIC-GMAIL-REFRESH-CANARY".into()).unwrap()),
        expires_in: 3600,
        scopes: Some(
            moosuri_connectors::gmail::MINIMAL_SCOPES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        ),
    }
}
async fn connect(store: &Store, user: &UserContext) -> MailConnection {
    let start = store.start_mail_oauth(user).await.unwrap();
    let handle = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let exchange = store.claim_mail_oauth(user, handle).await.unwrap();
    store
        .complete_mail_oauth(exchange, tokens(), "sender@example.test".into())
        .await
        .unwrap()
}
fn input(c: &MailConnection) -> MailDraftInput {
    MailDraftInput {
        connection_id: c.id.clone(),
        content: MailDraftContent {
            from: c.email.clone(),
            to: vec!["to@example.test".into()],
            cc: vec!["cc@example.test".into()],
            bcc: vec!["hidden@example.test".into()],
            subject: "합성 회의 ✓".into(),
            text: "합성 본문\n안녕하세요".into(),
            reply: None,
        },
        origin_thread_id: None,
    }
}
fn approval(a: &MailAction) -> MailApproval {
    MailApproval {
        revision: a.revision,
        digest: a.digest.clone(),
    }
}
#[tokio::test]
async fn oauth_is_one_use_session_bound_and_tokens_never_enter_dtos() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "member@example.test").await;
    let start = store.start_mail_oauth(&a.user).await.unwrap();
    let state = start.state.clone();
    let handle = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    assert!(
        store
            .capture_mail_oauth(state, SecretString::new("another-code".into()).unwrap())
            .await
            .is_err()
    );
    assert!(
        store
            .claim_mail_oauth(&b.user, handle.clone())
            .await
            .is_err()
    );
    let second_session = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert!(
        store
            .claim_mail_oauth(&second_session.user, handle.clone())
            .await
            .is_err()
    );
    let ex = store
        .claim_mail_oauth(&a.user, handle.clone())
        .await
        .unwrap();
    assert!(store.claim_mail_oauth(&a.user, handle).await.is_err());
    let c = store
        .complete_mail_oauth(ex, tokens(), "sender@example.test".into())
        .await
        .unwrap();
    let dto = json(&store.mail_connections(&a.user).await.unwrap()).unwrap();
    assert!(!dto.contains("CANARY"));
    assert_eq!(c.status, "active");
    assert!(store.mail_binding(&b.user, c.id.clone()).await.is_err());
    let bytes = std::fs::read(dir.path().join("assistant.sqlite-wal")).unwrap_or_default();
    assert!(
        !bytes
            .windows(b"SYNTHETIC-GMAIL".len())
            .any(|w| w == b"SYNTHETIC-GMAIL")
    );
}
#[tokio::test]
async fn preview_approval_and_exact_frozen_send_have_no_unapproved_path() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let c = connect(&store, &a.user).await;
    let action = store
        .create_mail_draft(&a.user, input(&c), None)
        .await
        .unwrap();
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    assert_eq!(
        action.content.as_ref().unwrap().bcc,
        vec!["hidden@example.test"]
    );
    store
        .decide_mail(&a.user, action.id.clone(), approval(&action), true)
        .await
        .unwrap();
    store
        .decide_mail(&a.user, action.id.clone(), approval(&action), true)
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let p = moosuri_connectors::gmail::parse_mail(d.frozen.raw_mime()).unwrap();
    assert_eq!(p.subject, action.content.unwrap().subject);
    assert_eq!(p.bcc, vec!["hidden@example.test"]);
    assert_eq!(p.message_id.as_deref(), Some(action.message_id.as_str()));
    assert_eq!(digest_string(d.frozen.digest()), action.digest);
    store
        .finish_mail_dispatch(
            &d,
            Some(("gmail-message".into(), "gmail-thread".into())),
            false,
        )
        .await
        .unwrap();
    let done = store.mail_action(&a.user, action.id).await.unwrap();
    assert_eq!(done.state, "sent");
    assert_eq!(done.provider_message_id.as_deref(), Some("gmail-message"));
}
#[tokio::test]
async fn changes_rejection_other_user_and_disconnect_fence_uncommitted_mail() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "member@example.test").await;
    let c = connect(&store, &a.user).await;
    let old = store
        .create_mail_draft(&a.user, input(&c), None)
        .await
        .unwrap();
    assert!(
        store
            .decide_mail(&b.user, old.id.clone(), approval(&old), true)
            .await
            .is_err()
    );
    let mut changed = input(&c);
    changed.content.bcc = vec!["changed@example.test".into()];
    let new = store
        .create_mail_draft(&a.user, changed, Some((old.id.clone(), old.revision)))
        .await
        .unwrap();
    assert_ne!(new.digest, old.digest);
    assert!(
        store
            .decide_mail(&a.user, old.id.clone(), approval(&old), true)
            .await
            .is_err()
    );
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    store
        .decide_mail(&a.user, new.id.clone(), approval(&new), false)
        .await
        .unwrap();
    assert!(
        store
            .decide_mail(&a.user, new.id.clone(), approval(&new), true)
            .await
            .is_err()
    );
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let pending = store
        .create_mail_draft(&a.user, input(&c), None)
        .await
        .unwrap();
    store
        .decide_mail(&a.user, pending.id.clone(), approval(&pending), true)
        .await
        .unwrap();
    let stale = store.mail_binding(&a.user, c.id.clone()).await.unwrap();
    store.disconnect_mail(&a.user, c.id).await.unwrap();
    assert!(store.save_mail_refresh(&stale, tokens()).await.is_err());
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    assert_eq!(
        store.mail_action(&a.user, pending.id).await.unwrap().state,
        "cancelled"
    );
}
#[tokio::test]
async fn late_receipt_survives_disconnect_and_thread_purge() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let c = connect(&store, &a.user).await;
    let thread = store
        .create_thread(&a.user, "Synthetic".into())
        .await
        .unwrap();
    let mut i = input(&c);
    i.origin_thread_id = Some(thread.id.clone());
    let action = store.create_mail_draft(&a.user, i, None).await.unwrap();
    store
        .decide_mail(&a.user, action.id.clone(), approval(&action), true)
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    store.disconnect_mail(&a.user, c.id).await.unwrap();
    store.delete_thread(&a.user, thread.id).await.unwrap();
    assert!(store.mail_dispatch_binding(&d).await.is_err());
    store
        .finish_mail_dispatch(
            &d,
            Some(("late-message".into(), "late-thread".into())),
            false,
        )
        .await
        .unwrap();
    let a = store.mail_action(&a.user, action.id).await.unwrap();
    assert_eq!(a.state, "sent");
    assert!(a.content.is_none());
    assert_eq!(a.provider_message_id.as_deref(), Some("late-message"));
}
#[tokio::test]
async fn committed_crash_recovers_unknown_without_requeue() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let c = connect(&store, &a.user).await;
    let action = store
        .create_mail_draft(&a.user, input(&c), None)
        .await
        .unwrap();
    store
        .decide_mail(&a.user, action.id.clone(), approval(&action), true)
        .await
        .unwrap();
    let dispatch = store.next_mail_dispatch().await.unwrap().unwrap();
    drop(dispatch);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let user = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert_eq!(
        store.mail_action(&user, action.id).await.unwrap().state,
        "outcome_unknown"
    );
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
}
#[tokio::test]
async fn thread_purge_removes_pending_content_and_expiry_blocks_approval() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let c = connect(&store, &a.user).await;
    let t = store
        .create_thread(&a.user, "Synthetic".into())
        .await
        .unwrap();
    let mut i = input(&c);
    i.origin_thread_id = Some(t.id.clone());
    let action = store.create_mail_draft(&a.user, i, None).await.unwrap();
    store.delete_thread(&a.user, t.id).await.unwrap();
    let purged = store.mail_action(&a.user, action.id.clone()).await.unwrap();
    assert!(purged.content.is_none());
    assert_eq!(purged.state, "cancelled");
    assert!(
        store
            .decide_mail(&a.user, action.id, approval(&purged), true)
            .await
            .is_err()
    );
    let expired = store
        .create_mail_draft(&a.user, input(&c), None)
        .await
        .unwrap();
    let id = expired.id.clone();
    store
        .call(move |db| {
            db.conn
                .execute("UPDATE mail_actions SET expires_at=0 WHERE id=?1", [id])?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        store
            .decide_mail(&a.user, expired.id.clone(), approval(&expired), true)
            .await
            .is_err()
    );
}

async fn configured(store: &Store, user: &UserContext) {
    let p = store
        .create_provider(
            user,
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
        .record_model_probe(user, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(user, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            user,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: p.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
}
async fn claim(store: &Store, user: &UserContext, thread: &str) -> crate::AgentClaim {
    store
        .submit_turn(
            user,
            thread.into(),
            Uuid::new_v4().to_string(),
            "Synthetic mail request".into(),
        )
        .await
        .unwrap();
    store.claim_agent_run().await.unwrap().unwrap()
}
async fn step(
    store: &Store,
    claim: &crate::AgentClaim,
    input: crate::KnowledgeToolInput,
) -> crate::StepLease {
    let context = store.agent_context(&claim.lease).await.unwrap();
    let s = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &s,
            crate::PrivateCheckpoint {
                bytes: br#"{"v":1,"fixture":true}"#.to_vec(),
            },
            vec![crate::PendingKnowledgeCall {
                call_id: "call".into(),
                input,
            }],
        )
        .await
        .unwrap();
    s
}
#[tokio::test]
async fn external_admission_charges_once_and_disconnect_fences_late_result() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    configured(&store, &a.user).await;
    let c = connect(&store, &a.user).await;
    let t = store
        .create_thread(&a.user, "Synthetic".into())
        .await
        .unwrap();
    let claimed = claim(&store, &a.user, &t.id).await;
    assert_eq!(claimed.tool_profile.as_ref().unwrap().version, 2);
    let s = step(
        &store,
        &claimed,
        crate::KnowledgeToolInput::External {
            request: ExternalToolRequest::Mail(MailToolRequest::Read {
                connection_id: c.id.clone(),
                message_id: "m1".into(),
            }),
        },
    )
    .await;
    let permit = match store
        .prepare_external_tool(&s, "call".into())
        .await
        .unwrap()
    {
        ExternalToolStart::Dispatch(p) => p,
        _ => panic!("missing dispatch"),
    };
    let again = store
        .prepare_external_tool(&s, "call".into())
        .await
        .unwrap();
    assert!(matches!(
        again,
        ExternalToolStart::Complete(crate::KnowledgeToolResult::Error(
            crate::ToolReadError::OutcomeUnknown
        ))
    ));
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT tool_calls FROM agent_execution", [], |r| r
                        .get::<_, i64>(0))?,
                1
            );
            Ok(())
        })
        .await
        .unwrap();
    store.disconnect_mail(&a.user, c.id).await.unwrap();
    assert!(
        store
            .complete_external_tool(
                &permit,
                crate::KnowledgeToolResult::External(
                    serde_json::json!({"text":"late private mail"})
                )
            )
            .await
            .is_err()
    );
    assert!(store.agent_context(&claimed.lease).await.is_err());
}
#[tokio::test]
async fn knowledge_withdrawal_cancels_derived_draft_even_after_manual_edit() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    configured(&store, &a.user).await;
    let c = connect(&store, &a.user).await;
    let group = store
        .create_collection(&a.user, "Synthetic".into(), "synthetic".into())
        .await
        .unwrap();
    let source = store
        .ingest_text(
            &a.user,
            group.id,
            IngestBatch {
                request_id: Uuid::new_v4().to_string(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: "one".into(),
                    version: "v1".into(),
                    expected_revision: None,
                    title: "Source".into(),
                    text: "Synthetic promise".into(),
                    speaker: None,
                    observed_at: 1,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap()
        .source_ids[0]
        .clone();
    let t = store
        .create_thread(&a.user, "Synthetic".into())
        .await
        .unwrap();
    let claimed = claim(&store, &a.user, &t.id).await;
    let s = step(
        &store,
        &claimed,
        crate::KnowledgeToolInput::ReadSource {
            id: source.clone(),
            revision: 1,
            start_byte: 0,
            max_bytes: 100,
        },
    )
    .await;
    store
        .execute_knowledge_tool(&s, "call".into())
        .await
        .unwrap();
    let s = step(
        &store,
        &claimed,
        crate::KnowledgeToolInput::External {
            request: ExternalToolRequest::Mail(MailToolRequest::Draft { input: input(&c) }),
        },
    )
    .await;
    store
        .execute_knowledge_tool(&s, "call".into())
        .await
        .unwrap();
    let action = store.mail_actions(&a.user).await.unwrap().remove(0);
    let edited = store
        .create_mail_draft(
            &a.user,
            input(&c),
            Some((action.id.clone(), action.revision)),
        )
        .await
        .unwrap();
    store.delete_source(&a.user, source, 1).await.unwrap();
    let cancelled = store.mail_action(&a.user, edited.id.clone()).await.unwrap();
    assert_eq!(cancelled.state, "cancelled");
    assert!(cancelled.content.is_none());
    assert!(
        store
            .decide_mail(&a.user, edited.id, approval(&cancelled), true)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn successor_disconnect_replays_full_credential_and_draft_purge_after_failed_commit() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let other = member(&store, &user, "other@example.test").await.user;
    let own = connect(&store, &user).await;
    let retained = connect(&store, &other).await;
    let draft = store
        .create_mail_draft(&user, input(&own), None)
        .await
        .unwrap();
    store.call(|db|{db.conn.execute_batch("CREATE TEMP TRIGGER fail_disconnect BEFORE UPDATE OF credential ON mail_connections BEGIN SELECT RAISE(ABORT,'synthetic interruption'); END;")?;Ok(())}).await.unwrap();
    assert!(store.disconnect_mail(&user, own.id.clone()).await.is_err());
    assert_eq!(store.initialized().await, Err(StoreError::Unavailable));
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(
        store.mail_connections(&user).await.unwrap()[0].status,
        "disconnected"
    );
    assert!(store.mail_binding(&user, own.id.clone()).await.is_err());
    // Repeating after a lost response is an explicit no-op, not a new adoption.
    store.disconnect_mail(&user, own.id).await.unwrap();
    let action = store.mail_action(&user, draft.id).await.unwrap();
    assert_eq!(action.state, "cancelled");
    assert!(action.content.is_none());
    assert_eq!(
        store.mail_connections(&other).await.unwrap()[0].id,
        retained.id
    );
    let tenant = user.tenant.0.clone();
    store.call(move|db|{assert_eq!(db.destructive_journal.records.len(),1);assert!(db.knowledge_journal.records.is_empty());let count:i64=db.conn.query_row("SELECT count(*) FROM mail_connections WHERE tenant_id=?1 AND credential IS NOT NULL",[tenant],|r|r.get(0))?;assert_eq!(count,0);Ok(())}).await.unwrap();
}

#[tokio::test]
async fn reconciliation_is_read_only_bounded_and_original_identity_is_immutable() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let owner = admin(&store).await;
    let other = member(&store, &owner.user, "other-reconcile@example.test").await;
    let c = connect(&store, &owner.user).await;
    let a = store
        .create_mail_draft(&owner.user, input(&c), None)
        .await
        .unwrap();
    store
        .decide_mail(&owner.user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    store.finish_mail_dispatch(&d, None, false).await.unwrap();
    assert!(
        store
            .claim_mail_reconciliation(&other.user, a.id.clone())
            .await
            .is_err()
    );
    let p = store.next_mail_reconciliation().await.unwrap().unwrap();
    assert!(p.fingerprint().is_some());
    assert!(store.next_mail_reconciliation().await.unwrap().is_none());
    store
        .finish_mail_reconciliation(
            &p,
            Some(moosuri_connectors::gmail::SentObservation::Matching {
                message_id: "observed".into(),
                thread_id: "observed-thread".into(),
            }),
        )
        .await
        .unwrap();
    let seen = store.mail_action(&owner.user, a.id.clone()).await.unwrap();
    assert_eq!(seen.state, "outcome_unknown");
    assert!(seen.provider_message_id.is_none());
    assert_eq!(
        seen.reconciliation.unwrap().status,
        "matching_sent_observed"
    );
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let p = store
        .claim_mail_reconciliation(&owner.user, a.id.clone())
        .await
        .unwrap();
    store.finish_mail_reconciliation(&p, None).await.unwrap();
    assert!(matches!(
        store
            .claim_mail_reconciliation(&owner.user, a.id.clone())
            .await,
        Err(StoreError::Busy)
    ));
    assert!(store.finish_mail_reconciliation(&p, None).await.is_err()); // stale lease CAS
    // A late actual send response remains authoritative; conflicting lookup is retained visibly.
    store
        .finish_mail_dispatch(&d, Some(("actual".into(), "actual-thread".into())), false)
        .await
        .unwrap();
    let sent = store.mail_action(&owner.user, a.id.clone()).await.unwrap();
    assert_eq!(sent.state, "sent");
    assert_eq!(sent.provider_message_id.as_deref(), Some("actual"));
    assert_eq!(sent.reconciliation.unwrap().status, "receipt_conflict");
    assert!(
        store
            .claim_mail_reconciliation(&owner.user, a.id)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn reconciliation_survives_restart_without_plaintext_or_new_send_and_budget_cannot_reset() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let owner = admin(&store).await;
    let c = connect(&store, &owner.user).await;
    let a = store
        .create_mail_draft(&owner.user, input(&c), None)
        .await
        .unwrap();
    store
        .decide_mail(&owner.user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    let tenant = owner.user.tenant.0.clone();
    let id = a.id.clone();
    store.call(move|db|{
        let (raw,evidence):(Option<Vec<u8>>,String)=db.conn.query_row("SELECT a.raw,e.fingerprint FROM mail_actions a JOIN mail_send_evidence e ON e.tenant_id=a.tenant_id AND e.action_id=a.id WHERE a.tenant_id=?1 AND a.id=?2",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?)))?;
        assert!(raw.is_none());assert_eq!(evidence.len(),64);
        assert!(db.conn.execute("UPDATE mail_send_evidence SET fingerprint=printf('%064d',0)",[]).is_err());
        Ok(())
    }).await.unwrap();
    drop(d);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let a = store.mail_action(&owner.user, a.id.clone()).await.unwrap();
    assert_eq!(a.state, "outcome_unknown");
    assert!(a.content.is_none());
    for expected in 1..=4 {
        let p = store.next_mail_reconciliation().await.unwrap().unwrap();
        store.finish_mail_reconciliation(&p, None).await.unwrap();
        assert_eq!(
            store
                .mail_action(&owner.user, a.id.clone())
                .await
                .unwrap()
                .reconciliation
                .unwrap()
                .automatic_attempts,
            expected
        );
        let id = a.id.clone();
        store.call(move|db|{db.conn.execute("UPDATE mail_reconciliations SET next_due=0 WHERE action_id=?1 AND next_due IS NOT NULL",[id])?;Ok(())}).await.unwrap();
    }
    assert!(store.next_mail_reconciliation().await.unwrap().is_none());
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.next_mail_reconciliation().await.unwrap().is_none());
}

#[tokio::test]
async fn revoked_lookup_cannot_publish_or_refresh_and_withdrawal_is_irreversible() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let owner = admin(&store).await;
    let c = connect(&store, &owner.user).await;
    let a = store
        .create_mail_draft(&owner.user, input(&c), None)
        .await
        .unwrap();
    store
        .decide_mail(&owner.user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    store.finish_mail_dispatch(&d, None, false).await.unwrap();
    let p = store.next_mail_reconciliation().await.unwrap().unwrap();
    store.disconnect_mail(&owner.user, c.id).await.unwrap();
    assert!(
        store
            .finish_mail_reconciliation(
                &p,
                Some(moosuri_connectors::gmail::SentObservation::Matching {
                    message_id: "late-private".into(),
                    thread_id: "late-private-thread".into()
                })
            )
            .await
            .is_err()
    );
    assert_eq!(
        store
            .mail_action(&owner.user, a.id.clone())
            .await
            .unwrap()
            .reconciliation
            .unwrap()
            .status,
        "not_checked"
    );
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    store
        .finish_mail_dispatch(
            &d,
            Some(("late-send-response".into(), "late-thread".into())),
            false,
        )
        .await
        .unwrap();
    assert_eq!(
        store.mail_action(&owner.user, a.id).await.unwrap().state,
        "sent"
    );
}

#[tokio::test]
async fn reconciliation_current_account_and_original_scope_are_checked_without_a_session() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let administrator = admin(&store).await;
    let mut owner = member(&store, &administrator.user, "member-guards@example.test").await;
    let c = connect(&store, &owner.user).await;
    for change in [
        "status='suspended'",
        "recovery_required=1",
        "must_change_password=1,temporary_expires_at=4102444800",
        "must_change_password=1,temporary_expires_at=1",
    ] {
        // Each guard needs fresh due work: rejecting invalid original authority
        // permanently retires that action's automatic schedule.
        let a = store
            .create_mail_draft(&owner.user, input(&c), None)
            .await
            .unwrap();
        store
            .decide_mail(&owner.user, a.id.clone(), approval(&a), true)
            .await
            .unwrap();
        let d = store.next_mail_dispatch().await.unwrap().unwrap();
        store.finish_mail_dispatch(&d, None, false).await.unwrap();
        store.logout(&owner.user).await.unwrap();
        let id = owner.user.account.id.clone();
        let sql = format!("UPDATE accounts SET {change} WHERE id=?1");
        store
            .call(move |db| {
                db.conn.execute(&sql, [id])?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(
            store.next_mail_reconciliation().await.unwrap().is_none(),
            "{change}"
        );
        let id = a.id.clone();
        store.call(move|db|{
            let (due,attempts,manual_after,nonce,until):(Option<i64>,i64,i64,Option<Vec<u8>>,Option<i64>)=db.conn.query_row("SELECT next_due,automatic_attempts,manual_after,lease_nonce,lease_until FROM mail_reconciliations WHERE action_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            assert_eq!((due,attempts,manual_after,nonce,until),(None,0,0,None,None),"{change}");
            Ok(())
        }).await.unwrap();
        let id = owner.user.account.id.clone();
        store.call(move|db|{db.conn.execute("UPDATE accounts SET status='active',recovery_required=0,must_change_password=0,temporary_expires_at=NULL WHERE id=?1",[id])?;Ok(())}).await.unwrap();
        assert!(store.next_mail_reconciliation().await.unwrap().is_none());
        assert!(matches!(
            store
                .claim_mail_reconciliation(&owner.user, a.id.clone())
                .await,
            Err(StoreError::Unauthorized)
        ));
        owner = store
            .login("member-guards@example.test".into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(matches!(
            store
                .claim_mail_reconciliation(&administrator.user, a.id.clone())
                .await,
            Err(StoreError::NotFound)
        ));
        // A new authenticated session can explicitly check the same original
        // action while its account/connection epochs remain current.
        let p = store
            .claim_mail_reconciliation(&owner.user, a.id.clone())
            .await
            .unwrap();
        assert_eq!(p.message_id(), a.message_id);
        assert_eq!(p.connection_id(), c.id);
        assert!(p.fingerprint().is_some());
        store.finish_mail_reconciliation(&p, None).await.unwrap();
        assert!(matches!(
            store
                .claim_mail_reconciliation(&owner.user, a.id.clone())
                .await,
            Err(StoreError::Busy)
        ));
        let row = store.mail_action(&owner.user, a.id.clone()).await.unwrap();
        assert_eq!(row.state, "outcome_unknown");
        assert!(row.provider_message_id.is_none());
        let reconciliation = row.reconciliation.unwrap();
        assert_eq!(reconciliation.status, "lookup_unavailable");
        assert_eq!(reconciliation.automatic_attempts, 0);
        assert!(reconciliation.manual_after > 0);
        let id = a.id;
        store.call(move|db|{
            let (due,lease):(Option<i64>,bool)=db.conn.query_row("SELECT next_due,lease_nonce IS NOT NULL OR lease_until IS NOT NULL FROM mail_reconciliations WHERE action_id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            assert!(due.is_none());assert!(!lease);Ok(())
        }).await.unwrap();
        assert!(store.next_mail_reconciliation().await.unwrap().is_none());
        assert!(store.next_mail_dispatch().await.unwrap().is_none());
    }
    // Use a fresh, currently held permit so the late CAS is rejected for the
    // removed read scope, rather than an expired, settled or retired lease.
    let a = store
        .create_mail_draft(&owner.user, input(&c), None)
        .await
        .unwrap();
    store
        .decide_mail(&owner.user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    store.finish_mail_dispatch(&d, None, false).await.unwrap();
    store.logout(&owner.user).await.unwrap();
    let p = store.next_mail_reconciliation().await.unwrap().unwrap();
    assert_eq!(p.message_id(), a.message_id);
    let id = c.id;
    store
        .call(move |db| {
            db.conn
                .execute("UPDATE mail_connections SET scopes='[]' WHERE id=?1", [id])?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store
            .finish_mail_reconciliation(
                &p,
                Some(moosuri_connectors::gmail::SentObservation::Matching {
                    message_id: "late".into(),
                    thread_id: "late-thread".into()
                })
            )
            .await,
        Err(StoreError::Forbidden)
    ));
    owner = store
        .login("member-guards@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let row = store.mail_action(&owner.user, a.id).await.unwrap();
    assert_eq!(row.state, "outcome_unknown");
    assert!(row.provider_message_id.is_none());
    assert_eq!(row.reconciliation.unwrap().status, "not_checked");
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
}

#[tokio::test]
async fn schema29_and30_upgrade_keeps_unknown_legacy_without_self_derived_oracle() {
    let _g = TEST_LOCK.lock().await;
    for predecessor in [29, 30] {
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let owner = admin(&store).await;
        let c = connect(&store, &owner.user).await;
        let a = store
            .create_mail_draft(&owner.user, input(&c), None)
            .await
            .unwrap();
        store
            .decide_mail(&owner.user, a.id.clone(), approval(&a), true)
            .await
            .unwrap();
        let d = store.next_mail_dispatch().await.unwrap().unwrap();
        store.finish_mail_dispatch(&d, None, false).await.unwrap();
        drop(d);
        drop(store);
        let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
        conn.execute_batch("DROP TABLE community_catalog_cache;DROP TRIGGER chatgpt_handoff_session_logout;DROP TRIGGER chatgpt_handoff_account_epoch;DROP TRIGGER chatgpt_handoff_binding_change;DROP TRIGGER session_purpose_valid_insert;DROP TRIGGER session_purpose_immutable;ALTER TABLE sessions DROP COLUMN absolute_deadline;ALTER TABLE sessions DROP COLUMN purpose;DROP TABLE chatgpt_handoffs;DROP TRIGGER audio_authority_no_delete;DROP TRIGGER audio_authority_monotonic;DROP TABLE audio_jobs;DROP TABLE audio_selection;DROP TABLE audio_authority;DROP TRIGGER personal_subscription_exclusive;DROP TRIGGER personal_shared_exclusive;DROP TRIGGER personal_shared_exclusive_update;DROP TABLE personal_subscription_selections;DROP TABLE chatgpt_model_probes;DROP TABLE chatgpt_auth_attempts;DROP TABLE chatgpt_sessions;DROP TABLE chatgpt_binding_revisions;DROP TABLE chatgpt_bindings;DROP TABLE subscription_runtime;DROP TABLE mail_reconciliations;DROP TABLE mail_send_evidence;")
            .unwrap();
        if predecessor == 29 {
            conn.execute_batch("DROP TABLE calendar_agent_action_origins;DROP TABLE calendar_agent_action_links;DROP TABLE calendar_command_plans;DROP TABLE calendar_agent_observations;DROP TABLE calendar_run_dependencies;DROP TABLE calendar_agent_profiles;").unwrap();
        }
        conn.pragma_update(None, "user_version", predecessor)
            .unwrap();
        drop(conn);
        let store = Store::open(dir.path()).unwrap();
        let p = store.next_mail_reconciliation().await.unwrap().unwrap();
        assert!(p.fingerprint().is_none());
        store
            .finish_mail_reconciliation(
                &p,
                Some(moosuri_connectors::gmail::SentObservation::Unresolved {
                    status: "legacy_evidence_unavailable",
                    candidates: 1,
                }),
            )
            .await
            .unwrap();
        let row = store.mail_action(&owner.user, a.id).await.unwrap();
        assert_eq!(row.state, "outcome_unknown");
        assert_eq!(
            row.reconciliation.unwrap().status,
            "legacy_evidence_unavailable"
        );
        assert!(store.next_mail_dispatch().await.unwrap().is_none());
    }
}

#[tokio::test]
async fn unknown_lookup_cannot_outlive_source_thread_withdrawal() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let owner = admin(&store).await;
    let c = connect(&store, &owner.user).await;
    let thread = store
        .create_thread(&owner.user, "Synthetic origin".into())
        .await
        .unwrap();
    let mut draft = input(&c);
    draft.origin_thread_id = Some(thread.id.clone());
    let a = store
        .create_mail_draft(&owner.user, draft, None)
        .await
        .unwrap();
    store
        .decide_mail(&owner.user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    store.finish_mail_dispatch(&d, None, false).await.unwrap();
    let p = store.next_mail_reconciliation().await.unwrap().unwrap();
    store.delete_thread(&owner.user, thread.id).await.unwrap();
    assert!(
        store
            .finish_mail_reconciliation(
                &p,
                Some(moosuri_connectors::gmail::SentObservation::Matching {
                    message_id: "withdrawn".into(),
                    thread_id: "withdrawn-thread".into()
                })
            )
            .await
            .is_err()
    );
    assert!(
        store
            .claim_mail_reconciliation(&owner.user, a.id.clone())
            .await
            .is_err()
    );
    assert_eq!(
        store.mail_connections(&owner.user).await.unwrap()[0].status,
        "active"
    );
    assert_eq!(
        store
            .mail_action(&owner.user, a.id)
            .await
            .unwrap()
            .reconciliation
            .unwrap()
            .status,
        "not_checked"
    );
}

#[tokio::test]
async fn reconciliation_retires_invalid_due_authority_without_starving_another_owner() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let old = admin(&store).await;
    let current = member(&store, &old.user, "fair-current@example.test").await;
    let withdrawn = connect(&store, &old.user).await;
    let disconnected = connect(&store, &old.user).await;
    let good = connect(&store, &current.user).await;
    let source = store
        .create_thread(&old.user, "Synthetic withdrawn source".into())
        .await
        .unwrap();
    let mut invalid = Vec::new();
    for n in 0..16 {
        let mut draft = input(if n < 8 { &withdrawn } else { &disconnected });
        if n < 8 {
            draft.origin_thread_id = Some(source.id.clone());
        }
        let a = store
            .create_mail_draft(&old.user, draft, None)
            .await
            .unwrap();
        store
            .decide_mail(&old.user, a.id.clone(), approval(&a), true)
            .await
            .unwrap();
        let d = store.next_mail_dispatch().await.unwrap().unwrap();
        store.finish_mail_dispatch(&d, None, false).await.unwrap();
        invalid.push(a.id);
    }
    store.delete_thread(&old.user, source.id).await.unwrap();
    store
        .disconnect_mail(&old.user, disconnected.id)
        .await
        .unwrap();
    // An active manual read lease is transient; automatic work must leave its
    // budget/due time/lease/manual rate limit intact while checking other rows.
    let held = store
        .create_mail_draft(&current.user, input(&good), None)
        .await
        .unwrap();
    store
        .decide_mail(&current.user, held.id.clone(), approval(&held), true)
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    store.finish_mail_dispatch(&d, None, false).await.unwrap();
    let manual = store
        .claim_mail_reconciliation(&current.user, held.id.clone())
        .await
        .unwrap();
    let later = store
        .create_mail_draft(&current.user, input(&good), None)
        .await
        .unwrap();
    store
        .decide_mail(&current.user, later.id.clone(), approval(&later), true)
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    store.finish_mail_dispatch(&d, None, false).await.unwrap();
    store.call(|db|{
        // Explicit due ordering, independent of wall-clock timestamp ties.
        db.conn.execute("UPDATE mail_reconciliations SET next_due=CASE WHEN action_id IN(SELECT id FROM mail_actions WHERE account_id=(SELECT id FROM accounts WHERE role='admin')) THEN 1 ELSE 2 END",[])?;
        Ok(())
    }).await.unwrap();
    let mut admitted = None;
    for _ in 0..2 {
        if let Some(p) = store.next_mail_reconciliation().await.unwrap() {
            admitted = Some(p);
            break;
        }
    }
    let p = admitted.expect(
        "16 withdrawn/disconnected rows must not indefinitely block a later eligible owner",
    );
    assert_eq!(p.message_id(), later.message_id);
    store.finish_mail_reconciliation(&p, None).await.unwrap();
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let tenant = old.user.tenant.0.clone();
    let held_id = held.id.clone();
    store.call(move|db|{
        assert_eq!(db.conn.query_row("SELECT count(*) FROM mail_reconciliations WHERE tenant_id=?1 AND next_due IS NULL AND automatic_attempts=0 AND lease_nonce IS NULL AND manual_after=0",[tenant],|r|r.get::<_,i64>(0))?,16);
        let (due,attempts,lease,manual_after):(Option<i64>,i64,bool,i64)=db.conn.query_row("SELECT next_due,automatic_attempts,lease_nonce IS NOT NULL,manual_after FROM mail_reconciliations WHERE action_id=?1",[held_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        assert_eq!(due,Some(2));assert_eq!(attempts,0);assert!(lease);assert!(manual_after>0);Ok(())
    }).await.unwrap();
    assert!(matches!(
        store
            .claim_mail_reconciliation(&current.user, held.id.clone())
            .await,
        Err(StoreError::Busy)
    ));
    // Retiring automatic authority does not alter immutable evidence or the
    // original unknown send. Restarts cannot silently revive that authority.
    for id in invalid {
        assert_eq!(
            store.mail_action(&old.user, id).await.unwrap().state,
            "outcome_unknown"
        );
    }
    drop(p);
    drop(manual);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    for _ in 0..3 {
        assert!(store.next_mail_reconciliation().await.unwrap().is_none());
        assert!(store.next_mail_dispatch().await.unwrap().is_none());
    }
    assert!(matches!(
        store
            .claim_mail_reconciliation(&current.user, held.id)
            .await,
        Err(StoreError::Busy)
    ));
    assert_eq!(
        store
            .mail_action(&current.user, later.id)
            .await
            .unwrap()
            .reconciliation
            .unwrap()
            .automatic_attempts,
        1
    );
}
