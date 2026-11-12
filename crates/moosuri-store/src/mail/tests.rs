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
