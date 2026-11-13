use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};
fn input(id: &str, text: &str) -> SourceInput {
    SourceInput {
        external_id: id.into(),
        version: "v1".into(),
        expected_revision: None,
        title: "Synthetic source".into(),
        text: text.into(),
        speaker: None,
        observed_at: 1_700_000_000,
        valid_from: None,
        valid_until: None,
    }
}
fn custom(name: &str) -> CreateSourceConnection {
    CreateSourceConnection {
        name: name.into(),
        kind: SourceConnectionKind::CustomHttp,
        scope: "synthetic immutable scope".into(),
        remote_account: "producer@example.test".into(),
        mail_connection_id: None,
        sync_enabled: false,
        poll_interval_seconds: 60,
    }
}
async fn connection_with_token(
    store: &Store,
    user: &UserContext,
    name: &str,
) -> (SourceConnection, SourceIngestCapability, String) {
    let c = store
        .create_source_connection(user, custom(name))
        .await
        .unwrap();
    let token = store
        .issue_source_token(user, c.id.clone(), 3600)
        .await
        .unwrap()
        .token;
    let cap = store
        .authenticate_source_token(token.clone())
        .await
        .unwrap();
    (c, cap, token)
}
fn batch(lease: &SourceRunLease, sequence: i64, entries: Vec<SourceInput>) -> SourceBatch {
    SourceBatch {
        protocol_version: 1,
        request_id: Uuid::new_v4().to_string(),
        config_revision: lease.cap.config_revision,
        run_id: lease.id.clone(),
        generation: lease.generation,
        expected_sequence: sequence,
        checkpoint_before: None,
        checkpoint_after: None,
        entries,
        metadata: vec![],
        tombstones: vec![],
        complete_snapshot: false,
    }
}
#[tokio::test]
async fn tenant_identity_exact_replay_rotation_and_revocation_are_fenced() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "source-member@example.test").await.user;
    let (ca, cap, token) = connection_with_token(&store, &a, "first").await;
    let (cb, capb, _) = connection_with_token(&store, &b, "second").await;
    assert!(matches!(
        store.source_connection(&b, ca.id.clone()).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.issue_source_token(&a, cb.id, 3600).await,
        Err(StoreError::NotFound)
    ));
    let lease = store.begin_source_run(&cap).await.unwrap();
    let mut page = batch(&lease, 0, vec![input("same remote id", "Synthetic text")]);
    page.checkpoint_after = Some("opaque:history:19".into());
    page.metadata.push(SourceObjectMetadata {
        external_id: "same remote id".into(),
        thread_id: Some("thread-parent".into()),
        parent_id: Some("remote-parent".into()),
    });
    let r = store
        .commit_source_batch(&lease, page.clone())
        .await
        .unwrap();
    let replay = store
        .commit_source_batch(&lease, page.clone())
        .await
        .unwrap();
    assert_eq!(r.sequence, replay.sequence);
    assert_eq!(r.source_ids, replay.source_ids);
    assert_eq!(
        store
            .source(&a, r.source_ids[0].clone(), None)
            .await
            .unwrap()
            .provenance
            .unwrap()
            .request_id,
        r.request_id
    );
    let mut changed = page.clone();
    changed.entries[0].text = "Changed".into();
    assert!(matches!(
        store.commit_source_batch(&lease, changed).await,
        Err(StoreError::Conflict)
    ));
    let other = store.begin_source_run(&capb).await.unwrap();
    let other_r = store
        .commit_source_batch(
            &other,
            batch(&other, 0, vec![input("same remote id", "Other tenant")]),
        )
        .await
        .unwrap();
    assert_ne!(r.source_ids, other_r.source_ids);
    assert!(matches!(
        store
            .source_ingest_receipt(&capb, r.request_id.clone())
            .await,
        Err(StoreError::NotFound)
    ));
    let replacement = store
        .issue_source_token(&a, ca.id.clone(), 3600)
        .await
        .unwrap();
    assert!(matches!(
        store.authenticate_source_token(token).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.commit_source_batch(&lease, page).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store
            .source_ingest_receipt(&cap, r.request_id.clone())
            .await,
        Err(StoreError::Unauthorized)
    ));
    let current = store
        .authenticate_source_token(replacement.token)
        .await
        .unwrap();
    assert_eq!(
        store
            .source_ingest_receipt(&current, r.request_id)
            .await
            .unwrap()
            .sequence,
        1
    );
    store.revoke_source_token(&a, ca.id).await.unwrap();
    assert!(matches!(
        store.source_ingest_status(&current).await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn batch_atomicity_no_change_partial_late_reports_and_lease_recovery() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (c, cap, _) = connection_with_token(&store, &user, "Atomic").await;
    let lease = store.begin_source_run(&cap).await.unwrap();
    assert!(matches!(
        store.begin_source_run(&cap).await,
        Err(StoreError::Conflict)
    ));
    store.source_heartbeat(&lease).await.unwrap();
    assert_eq!(
        store
            .source_ingest_status(&cap)
            .await
            .unwrap()
            .local_sequence,
        0
    );
    let mut first = batch(&lease, 0, vec![input("one", "First")]);
    first.checkpoint_after = Some("opaque-first".into());
    let receipt = store.commit_source_batch(&lease, first).await.unwrap();
    let mut invalid = batch(
        &lease,
        1,
        vec![
            input("two", "Must roll back"),
            input("one", "Same version changed text"),
        ],
    );
    invalid.checkpoint_before = Some("opaque-first".into());
    invalid.checkpoint_after = Some("must-not-commit".into());
    assert!(matches!(
        store.commit_source_batch(&lease, invalid.clone()).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store.source_ingest_receipt(&cap, invalid.request_id).await,
        Err(StoreError::NotFound)
    ));
    let status = store.source_ingest_status(&cap).await.unwrap();
    assert_eq!(status.local_sequence, 1);
    assert_eq!(status.checkpoint.as_deref(), Some("opaque-first"));
    assert_eq!(
        store
            .sources(&user, c.collection_id.clone(), None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    let mut empty = batch(&lease, 1, vec![]);
    empty.checkpoint_before = Some("opaque-first".into());
    empty.checkpoint_after = Some("opaque-next".into());
    let r = store.commit_source_batch(&lease, empty).await.unwrap();
    assert_eq!(r.accepted, 0);
    assert_eq!(r.sequence, 2);
    store
        .finish_source_run(
            &lease,
            SourceRunOutcome::Partial,
            Some("partial_coverage".into()),
            None,
        )
        .await
        .unwrap();
    assert!(
        store
            .source_connection(&user, c.id.clone())
            .await
            .unwrap()
            .last_completed_at
            .is_none()
    );
    assert!(matches!(
        store.source_heartbeat(&lease).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .finish_source_run(&lease, SourceRunOutcome::Completed, None, None)
            .await,
        Err(StoreError::Conflict)
    ));
    let next = store.begin_source_run(&cap).await.unwrap();
    assert!(next.generation() > lease.generation());
    let mut nochange = batch(&next, 2, vec![]);
    nochange.checkpoint_before = Some("opaque-next".into());
    nochange.checkpoint_after = Some("opaque-next".into());
    store.commit_source_batch(&next, nochange).await.unwrap();
    store
        .finish_source_run(&next, SourceRunOutcome::NoChange, None, None)
        .await
        .unwrap();
    assert!(
        store
            .source_connection(&user, c.id.clone())
            .await
            .unwrap()
            .last_completed_at
            .is_some()
    );
    assert_eq!(
        store
            .source(&user, receipt.source_ids[0].clone(), None)
            .await
            .unwrap()
            .text,
        "First"
    );
    let hung = store.begin_source_run(&cap).await.unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.source_heartbeat(&hung).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store.source_sync_runs(&user, c.id).await.unwrap()[0].state,
        "unknown"
    );
}
#[tokio::test]
async fn source_tombstone_withdraws_evidence_and_managed_collections_reject_manual_bypass() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (c, cap, _) = connection_with_token(&store, &user, "Delete").await;
    let lease = store.begin_source_run(&cap).await.unwrap();
    let r = store
        .commit_source_batch(&lease, batch(&lease, 0, vec![input("one", "Evidence")]))
        .await
        .unwrap();
    let source = r.source_ids[0].clone();
    let memory = store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            MemoryDraft {
                text: "Evidence".into(),
                subject: "Synthetic".into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![EvidenceRef {
                    source_id: source.clone(),
                    source_revision: 1,
                    start_byte: 0,
                    end_byte: 8,
                    quote: "Evidence".into(),
                }],
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .ingest_text(
                &user,
                c.collection_id.clone(),
                IngestBatch {
                    request_id: Uuid::new_v4().to_string(),
                    expected_cursor: 1,
                    next_cursor: 2,
                    complete_snapshot: false,
                    entries: vec![input("bypass", "Bad")]
                }
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let collection = store
        .collections(&user)
        .await
        .unwrap()
        .into_iter()
        .find(|k| k.id == c.collection_id)
        .unwrap();
    assert_eq!(
        collection.source_connection_id.as_deref(),
        Some(c.id.as_str())
    );
    assert!(matches!(
        store
            .disconnect_collection(
                &user,
                c.collection_id.clone(),
                collection.revision,
                DisconnectMode::Keep
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let mut delete = batch(&lease, 1, vec![]);
    delete.tombstones.push("one".into());
    store.commit_source_batch(&lease, delete).await.unwrap();
    assert!(matches!(
        store.source(&user, source.clone(), None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.memory(&user, memory.id, None).await,
        Err(StoreError::NotFound)
    ));
    let suppressed = store
        .commit_source_batch(
            &lease,
            batch(
                &lease,
                2,
                vec![
                    input("one", "Late revival"),
                    input("live", "Still imported"),
                ],
            ),
        )
        .await
        .unwrap();
    assert_eq!(suppressed.accepted, 1);
    assert_eq!(suppressed.unchanged, 1);
    assert_eq!(suppressed.source_ids.len(), 1);
    assert!(matches!(
        store.source(&user, source.clone(), None).await,
        Err(StoreError::NotFound)
    ));
    let mut unknown = batch(&lease, 3, vec![input("other", "Scoped upsert")]);
    unknown.tombstones.push("unknown".into());
    let ack = store.commit_source_batch(&lease, unknown).await.unwrap();
    assert_eq!(ack.unchanged, 1);
    assert_eq!(ack.accepted, 1);
    let mut snapshot = batch(&lease, 2, vec![input("new", "New")]);
    snapshot.complete_snapshot = true;
    assert!(matches!(
        store.commit_source_batch(&lease, snapshot).await,
        Err(StoreError::InvalidInput)
    ));
    let c = store
        .disconnect_source_connection(&user, c.id, c.config_revision, DisconnectMode::Delete)
        .await
        .unwrap();
    assert_eq!(c.state, "deleted");
    assert!(matches!(
        store.source_ingest_status(&cap).await,
        Err(StoreError::Unauthorized)
    ));
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.source(&user, source, None).await,
        Err(StoreError::NotFound)
    ));
}
#[tokio::test]
async fn journal_restore_token_epochs_account_password_and_disconnect_keep() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (c, cap, token) = connection_with_token(&store, &user, "Restore").await;
    let lease = store.begin_source_run(&cap).await.unwrap();
    let r = store
        .commit_source_batch(
            &lease,
            batch(&lease, 0, vec![input("kept", "Retained text")]),
        )
        .await
        .unwrap();
    let backup_dir = directory();
    let backup = backup_dir.path().join("synthetic-backup.sqlite");
    let backup_arg = backup.to_str().unwrap().to_owned();
    store
        .call(move |db| {
            db.conn.execute("VACUUM INTO ?1", [backup_arg])?;
            Ok(())
        })
        .await
        .unwrap();
    store
        .revoke_source_token(&user, c.id.clone())
        .await
        .unwrap();
    drop(store);
    std::fs::copy(&backup, dir.path().join("assistant.sqlite")).unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.authenticate_source_token(token).await,
        Err(StoreError::Unauthorized)
    ));
    let token = store
        .issue_source_token(&user, c.id.clone(), 3600)
        .await
        .unwrap();
    let cap = store
        .authenticate_source_token(token.token.clone())
        .await
        .unwrap();
    let new_user = store
        .change_password(&user, "Synthetic-second-password-2026".into())
        .await
        .unwrap()
        .user;
    assert!(matches!(
        store.source_ingest_status(&cap).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.authenticate_source_token(token.token).await,
        Err(StoreError::Unauthorized)
    ));
    let current = store
        .source_connection(&new_user, c.id.clone())
        .await
        .unwrap();
    let c = store
        .disconnect_source_connection(
            &new_user,
            c.id,
            current.config_revision,
            DisconnectMode::Keep,
        )
        .await
        .unwrap();
    assert_eq!(c.state, "disconnected");
    assert_eq!(
        store
            .source(&new_user, r.source_ids[0].clone(), None)
            .await
            .unwrap()
            .text,
        "Retained text"
    );
    assert!(matches!(
        store.issue_source_token(&new_user, c.id, 3600).await,
        Err(StoreError::InvalidInput)
    ));
}
#[tokio::test]
async fn scheduler_capability_slack_ciphertext_config_cas_and_backoff() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let mut request = custom("Slack synthetic");
    request.kind = SourceConnectionKind::Slack;
    request.sync_enabled = false;
    let c = store
        .create_source_connection(&user, request)
        .await
        .unwrap();
    store
        .set_source_slack_token(
            &user,
            c.id.clone(),
            SecretString::new("xoxp-synthetic-secret-not-a-real-token".into()).unwrap(),
        )
        .await
        .unwrap();
    assert!(store.next_source_connection().await.unwrap().is_none());
    let c = store
        .configure_source_connection(
            &user,
            c.id.clone(),
            c.config_revision,
            "Renamed".into(),
            true,
            120,
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .pause_source_connection(&user, c.id.clone(), 1, true)
            .await,
        Err(StoreError::Conflict)
    ));
    let cap = store.next_source_connection().await.unwrap().unwrap();
    let binding = store.source_runtime_binding(&cap).await.unwrap();
    assert_eq!(
        binding.slack_token.unwrap().expose(),
        "xoxp-synthetic-secret-not-a-real-token"
    );
    let raw: Vec<u8> = store
        .call({
            let id = c.id.clone();
            move |db| {
                Ok(db.conn.query_row(
                    "SELECT credential FROM source_connections WHERE id=?1",
                    [id],
                    |r| r.get(0),
                )?)
            }
        })
        .await
        .unwrap();
    assert!(!raw.windows(10).any(|s| s == b"xoxp-synth"));
    let lease = store.begin_source_run(&cap).await.unwrap();
    store
        .finish_source_run(
            &lease,
            SourceRunOutcome::Failed,
            Some("rate_limited".into()),
            Some(600),
        )
        .await
        .unwrap();
    assert!(store.next_source_connection().await.unwrap().is_none());
    assert!(
        store
            .source_connection(&user, c.id.clone())
            .await
            .unwrap()
            .next_due_at
            >= now().unwrap() + 599
    );
    let paused = store
        .pause_source_connection(&user, c.id.clone(), c.config_revision, true)
        .await
        .unwrap();
    assert!(matches!(
        store.source_runtime_binding(&cap).await,
        Err(StoreError::Unauthorized)
    ));
    let resumed = store
        .pause_source_connection(&user, c.id.clone(), paused.config_revision, false)
        .await
        .unwrap();
    store
        .trigger_source_sync(&user, c.id.clone(), resumed.config_revision)
        .await
        .unwrap();
    let cap = store.next_source_connection().await.unwrap().unwrap();
    let lease = store.begin_source_run(&cap).await.unwrap();
    store
        .set_source_slack_token(
            &user,
            c.id,
            SecretString::new("xoxp-new-synthetic-token".into()).unwrap(),
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .commit_source_batch(
                &lease,
                batch(&lease, 0, vec![input("late", "Old token result")])
            )
            .await,
        Err(StoreError::Unauthorized)
    ));
    let _new = store
        .change_password(&user, PASSWORD.to_owned() + "-changed")
        .await
        .unwrap();
    assert!(store.next_source_connection().await.unwrap().is_none());
}
#[tokio::test]
async fn gmail_background_credentials_refresh_use_current_mail_and_account_epochs() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let mail_id = Uuid::new_v4().to_string();
    let mail_id_arg = mail_id.clone();
    let owner = user.clone();
    store.call(move|db|{
        let aad=serde_json::to_vec(&("moosuri/gmail/v1","tokens",&owner.tenant.0,&owner.account.id,&mail_id_arg,1)).unwrap();
        let plain=Zeroizing::new(serde_json::to_vec(&("synthetic-access","synthetic-refresh")).unwrap());
        let cipher=providers::encrypt(&db.provider_key,&plain,&aad)?;
        db.conn.execute("INSERT INTO mail_connections(tenant_id,account_id,id,email,epoch,credential_revision,status,credential,expires_at,scopes) VALUES(?1,?2,?3,'gmail@example.test',1,1,'active',?4,?5,?6)",params![owner.tenant.0,owner.account.id,mail_id_arg,cipher,now()?+3600,json(&moosuri_connectors::gmail::MINIMAL_SCOPES)?])?;Ok(())
    }).await.unwrap();
    let mut request = custom("Gmail knowledge");
    request.kind = SourceConnectionKind::Gmail;
    request.remote_account = "forged@example.test".into();
    request.mail_connection_id = Some(mail_id.clone());
    request.sync_enabled = true;
    assert!(matches!(
        store.create_source_connection(&user, request.clone()).await,
        Err(StoreError::Conflict)
    ));
    request.remote_account = "gmail@example.test".into();
    let c = store
        .create_source_connection(&user, request)
        .await
        .unwrap();
    let cap = store.next_source_connection().await.unwrap().unwrap();
    let credentials = store.source_mail_credentials(&cap).await.unwrap();
    assert_eq!(credentials.access_token.expose(), "synthetic-access");
    let tokens = TokenSet {
        access_token: SecretString::new("synthetic-fresh-access".into()).unwrap(),
        refresh_token: None,
        expires_in: 1200,
        scopes: None,
    };
    store
        .save_source_mail_refresh(&cap, credentials.revision, tokens)
        .await
        .unwrap();
    let next = store.source_mail_credentials(&cap).await.unwrap();
    assert_eq!(next.refresh_token.expose(), "synthetic-refresh");
    assert_eq!(next.revision, 2);
    let tokens = TokenSet {
        access_token: SecretString::new("synthetic-late-access".into()).unwrap(),
        refresh_token: None,
        expires_in: 1200,
        scopes: None,
    };
    assert!(matches!(
        store
            .save_source_mail_refresh(&cap, credentials.revision, tokens)
            .await,
        Err(StoreError::Conflict)
    ));
    let lease = store.begin_source_run(&cap).await.unwrap();
    let page = batch(&lease, 0, vec![input("mail-1", "Synthetic mail")]);
    store
        .commit_source_batch(&lease, page.clone())
        .await
        .unwrap();
    store
        .finish_source_run(&lease, SourceRunOutcome::Completed, None, None)
        .await
        .unwrap();
    store
        .finish_source_run(&lease, SourceRunOutcome::Completed, None, None)
        .await
        .unwrap();
    assert_eq!(
        store
            .commit_source_batch(&lease, page)
            .await
            .unwrap()
            .sequence,
        1
    );
    // Reauthorization changes the mail epoch. The held source capability stays
    // invalid until the owner explicitly rebinds the same immutable account.
    let mail_epoch_id = mail_id.clone();
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE mail_connections SET epoch=epoch+1 WHERE id=?1",
                [mail_epoch_id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store.source_mail_credentials(&cap).await,
        Err(StoreError::Unauthorized)
    ));
    let rebound = store
        .reconnect_source_gmail(&user, c.id.clone(), c.config_revision, mail_id.clone())
        .await
        .unwrap();
    assert_eq!(rebound.config_revision, c.config_revision + 1);
    let new_cap = store.next_source_connection().await.unwrap().unwrap();
    assert_eq!(
        store
            .source_mail_credentials(&new_cap)
            .await
            .unwrap()
            .mail_epoch,
        2
    );
    store.disconnect_mail(&user, mail_id).await.unwrap();
    assert!(matches!(
        store.source_mail_credentials(&cap).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(store.next_source_connection().await.unwrap().is_none());
    assert!(
        store
            .source_connection(&user, c.id)
            .await
            .unwrap()
            .last_completed_at
            .is_some()
    );
}
#[tokio::test]
async fn polling_and_partial_continuation_do_not_overstate_scope_completion() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (c, cap, _) = connection_with_token(&store, &user, "Coverage").await;
    let lease = store.begin_source_run(&cap).await.unwrap();
    store
        .commit_source_batch(&lease, batch(&lease, 0, vec![]))
        .await
        .unwrap();
    store
        .finish_source_run(&lease, SourceRunOutcome::Polled, None, None)
        .await
        .unwrap();
    let current = store.source_connection(&user, c.id.clone()).await.unwrap();
    assert!(current.last_completed_at.is_none());
    let next = store.begin_source_run(&cap).await.unwrap();
    store
        .commit_source_batch(&next, batch(&next, 1, vec![]))
        .await
        .unwrap();
    store
        .finish_source_run(&next, SourceRunOutcome::Partial, None, None)
        .await
        .unwrap();
    let current = store.source_connection(&user, c.id).await.unwrap();
    assert!(current.last_completed_at.is_none());
    assert!(current.next_due_at <= now().unwrap() + 30);
}
#[tokio::test]
async fn deletion_intent_replays_after_pre_receipt_backup_without_reviving_source() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let backup_dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (c, cap, token) = connection_with_token(&store, &user, "Crash fence").await;
    let lease = store.begin_source_run(&cap).await.unwrap();
    let r = store
        .commit_source_batch(
            &lease,
            batch(&lease, 0, vec![input("one", "Before delete")]),
        )
        .await
        .unwrap();
    let backup = backup_dir.path().join("db.sqlite");
    let arg = backup.to_string_lossy().into_owned();
    store
        .call(move |db| {
            db.conn.execute("VACUUM INTO ?1", [arg])?;
            Ok(())
        })
        .await
        .unwrap();
    let mut delete = batch(&lease, 1, vec![]);
    delete.tombstones.push("one".into());
    let request = delete.request_id.clone();
    store.commit_source_batch(&lease, delete).await.unwrap();
    drop(store);
    std::fs::copy(backup, dir.path().join("assistant.sqlite")).unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.source(&user, r.source_ids[0].clone(), None).await,
        Err(StoreError::NotFound)
    ));
    let cap = store.authenticate_source_token(token).await.unwrap();
    assert_eq!(
        store
            .source_ingest_status(&cap)
            .await
            .unwrap()
            .local_sequence,
        1
    );
    assert!(matches!(
        store.source_ingest_receipt(&cap, request).await,
        Err(StoreError::NotFound)
    ));
    let run = store.begin_source_run(&cap).await.unwrap();
    let mut retry = batch(&run, 1, vec![]);
    retry.tombstones.push("one".into());
    store.commit_source_batch(&run, retry).await.unwrap();
    let suppressed = store
        .commit_source_batch(&run, batch(&run, 2, vec![input("one", "No revival")]))
        .await
        .unwrap();
    assert_eq!(suppressed.accepted, 0);
    assert_eq!(suppressed.unchanged, 1);
    assert!(suppressed.source_ids.is_empty());
    // Collection disconnect is the same durable journal boundary, with receipt
    // and grant lookup denied even when old source data were retained.
    let c = store.source_connection(&user, c.id.clone()).await.unwrap();
    store
        .disconnect_source_connection(&user, c.id, c.config_revision, DisconnectMode::Delete)
        .await
        .unwrap();
    assert!(matches!(
        store.source_ingest_status(&cap).await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn token_expiry_is_checked_again_at_commit_and_receipt_replay() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (c, cap, token) = connection_with_token(&store, &user, "Expiry").await;
    let lease = store.begin_source_run(&cap).await.unwrap();
    let page = batch(&lease, 0, vec![]);
    let r = store
        .commit_source_batch(&lease, page.clone())
        .await
        .unwrap();
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE source_connections SET token_expires=?2 WHERE id=?1",
                params![c.id, now()? - 1],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store.authenticate_source_token(token).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.commit_source_batch(&lease, page).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.source_ingest_receipt(&cap, r.request_id).await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn queued_commit_rechecks_token_after_rotation_and_bounds_run_history() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (c, old_cap, _) = connection_with_token(&store, &user, "Queued fence").await;
    let held = store.begin_source_run(&old_cap).await.unwrap();
    let page = batch(&held, 0, vec![input("held", "Never written")]);
    // This models a producer holding a valid pre-network lease while a user
    // rotates its grant. Commit is deliberately queued after the mutation.
    let fresh = store
        .issue_source_token(&user, c.id.clone(), 3600)
        .await
        .unwrap();
    let current = store.authenticate_source_token(fresh.token).await.unwrap();
    assert!(matches!(
        store.commit_source_batch(&held, page).await,
        Err(StoreError::Unauthorized)
    ));
    assert_eq!(
        store
            .source_ingest_status(&current)
            .await
            .unwrap()
            .local_sequence,
        0
    );
    let stale = store.begin_source_run(&current).await.unwrap();
    let run_id = stale.id.clone();
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE source_sync_runs SET lease_until=?2 WHERE id=?1",
                params![run_id, now()? - 1],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let replacement = store.begin_source_run(&current).await.unwrap();
    assert!(replacement.generation() > stale.generation());
    assert!(matches!(
        store.source_heartbeat(&stale).await,
        Err(StoreError::Conflict)
    ));
    store
        .finish_source_run(&replacement, SourceRunOutcome::Partial, None, None)
        .await
        .unwrap();
    for _ in 0..70 {
        let run = store.begin_source_run(&current).await.unwrap();
        store
            .finish_source_run(&run, SourceRunOutcome::Partial, None, None)
            .await
            .unwrap();
    }
    assert_eq!(store.source_sync_runs(&user, c.id).await.unwrap().len(), 64);
}
#[tokio::test]
async fn checkpoint_reset_fences_old_run_preserves_sequence_and_reencrypts_slack() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (c, cap, token) = connection_with_token(&store, &user, "Reset custom").await;
    let held = store.begin_source_run(&cap).await.unwrap();
    let mut page = batch(&held, 0, vec![input("one", "Retained original")]);
    page.checkpoint_after = Some("expired-provider-history".into());
    store
        .commit_source_batch(&held, page.clone())
        .await
        .unwrap();
    store
        .finish_source_run(
            &held,
            SourceRunOutcome::NeedsReconnect,
            Some("checkpoint_invalid".into()),
            None,
        )
        .await
        .unwrap();
    let reset = store
        .reset_source_checkpoint(&user, c.id.clone(), c.config_revision)
        .await
        .unwrap();
    assert_eq!(reset.config_revision, c.config_revision + 1);
    assert_eq!(reset.local_sequence, 1);
    assert!(matches!(
        store.commit_source_batch(&held, page).await,
        Err(StoreError::Unauthorized)
    ));
    let current = store.authenticate_source_token(token).await.unwrap();
    let status = store.source_ingest_status(&current).await.unwrap();
    assert!(status.checkpoint.is_none());
    assert_eq!(status.local_sequence, 1);
    assert_eq!(
        store
            .sources(&user, c.collection_id, None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    let mut request = custom("Reset Slack");
    request.kind = SourceConnectionKind::Slack;
    request.sync_enabled = true;
    let slack = store
        .create_source_connection(&user, request)
        .await
        .unwrap();
    store
        .set_source_slack_token(
            &user,
            slack.id.clone(),
            SecretString::new("xoxp-synthetic-retained".into()).unwrap(),
        )
        .await
        .unwrap();
    let old = store.next_source_connection().await.unwrap().unwrap();
    let reset = store
        .reset_source_checkpoint(&user, slack.id, slack.config_revision)
        .await
        .unwrap();
    assert!(matches!(
        store.source_runtime_binding(&old).await,
        Err(StoreError::Unauthorized)
    ));
    let current = store.next_source_connection().await.unwrap().unwrap();
    assert_eq!(
        store
            .source_runtime_binding(&current)
            .await
            .unwrap()
            .slack_token
            .unwrap()
            .expose(),
        "xoxp-synthetic-retained"
    );
    assert!(matches!(
        store.reset_source_checkpoint(&user, reset.id, 1).await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn editing_configuration_cannot_renew_grants_after_password_epoch_change() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (c, _, token) = connection_with_token(&store, &user, "Epoch").await;
    let user = store
        .change_password(&user, "Synthetic-account-epoch-changed".into())
        .await
        .unwrap()
        .user;
    let c = store
        .configure_source_connection(
            &user,
            c.id,
            c.config_revision,
            "Name edit".into(),
            false,
            120,
        )
        .await
        .unwrap();
    assert_eq!(c.state, "needs_reconnect");
    assert!(matches!(
        store.authenticate_source_token(token.clone()).await,
        Err(StoreError::Unauthorized)
    ));
    store
        .reset_source_checkpoint(&user, c.id, c.config_revision)
        .await
        .unwrap();
    assert!(matches!(
        store.authenticate_source_token(token).await,
        Err(StoreError::Unauthorized)
    ));
}

#[tokio::test]
async fn scheduler_filters_credential_ineligible_connections_before_candidate_limit() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let first = admin(&store).await.user;
    let second = member(&store, &first, "eligible-source@example.test")
        .await
        .user;
    let mail_id = Uuid::new_v4().to_string();
    let mail_arg = mail_id.clone();
    let owner = first.clone();
    store.call(move |db| {
        db.conn.execute(
            "INSERT INTO mail_connections(tenant_id,account_id,id,email,epoch,credential_revision,status,credential,expires_at,scopes)
             VALUES(?1,?2,?3,'ineligible@example.test',1,1,'active',zeroblob(32),?4,'[]')",
            params![owner.tenant.0, owner.account.id, mail_arg, now()? + 3600],
        )?;
        Ok(())
    }).await.unwrap();
    // The first account fills its 32-collection quota. Every row precedes the
    // eligible second account in due/rowid order, reproducing the old LIMIT bug.
    for index in 0..32 {
        let mut request = custom(&format!("Earlier ineligible {index}"));
        request.sync_enabled = true;
        if index < 16 {
            request.kind = SourceConnectionKind::Slack; // Missing personal token.
        } else {
            request.kind = SourceConnectionKind::Gmail;
            request.mail_connection_id = Some(mail_id.clone());
            request.remote_account = "ineligible@example.test".into();
        }
        store
            .create_source_connection(&first, request)
            .await
            .unwrap();
    }
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE mail_connections SET status='needs_reconnect',epoch=epoch+1 WHERE id=?1",
                [mail_id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let mut request = custom("Later eligible Slack");
    request.kind = SourceConnectionKind::Slack;
    request.sync_enabled = true;
    let eligible = store
        .create_source_connection(&second, request)
        .await
        .unwrap();
    store
        .set_source_slack_token(
            &second,
            eligible.id.clone(),
            SecretString::new("xoxp-synthetic-eligible".into()).unwrap(),
        )
        .await
        .unwrap();
    let before = store.call(|db| {
        Ok(db.conn.prepare("SELECT c.id,c.auth_epoch,c.state,c.credential_epoch,a.auth_epoch FROM source_connections c JOIN accounts a ON a.tenant_id=c.tenant_id AND a.id=c.account_id ORDER BY c.rowid")?
            .query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?)))?
            .collect::<std::result::Result<Vec<_>,_>>()?)
    }).await.unwrap();
    let cap = store
        .next_source_connection()
        .await
        .unwrap()
        .expect("later eligible connection must not starve");
    assert_eq!(cap.connection, eligible.id);
    let after = store.call(|db| {
        Ok(db.conn.prepare("SELECT c.id,c.auth_epoch,c.state,c.credential_epoch,a.auth_epoch FROM source_connections c JOIN accounts a ON a.tenant_id=c.tenant_id AND a.id=c.account_id ORDER BY c.rowid")?
            .query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?)))?
            .collect::<std::result::Result<Vec<_>,_>>()?)
    }).await.unwrap();
    assert_eq!(
        before, after,
        "selection must not rewrite stale authentication state"
    );
    assert_eq!(
        store
            .source_runtime_binding(&cap)
            .await
            .unwrap()
            .connection
            .id,
        eligible.id
    );
}

#[tokio::test]
async fn pruning_pins_source_receipt_and_run_but_bounds_unreferenced_history() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let (connection, cap, _) = connection_with_token(&store, &user, "Pinned provenance").await;
    let first_run = store.begin_source_run(&cap).await.unwrap();
    let first_page = batch(
        &first_run,
        0,
        vec![input("quiet-source", "Long-lived original")],
    );
    let first = store
        .commit_source_batch(&first_run, first_page.clone())
        .await
        .unwrap();
    store
        .finish_source_run(&first_run, SourceRunOutcome::Completed, None, None)
        .await
        .unwrap();
    let mut oldest_unreferenced = None;
    for sequence in 1..=8 {
        let run = store.begin_source_run(&cap).await.unwrap();
        let receipt = store
            .commit_source_batch(&run, batch(&run, sequence, vec![]))
            .await
            .unwrap();
        oldest_unreferenced.get_or_insert(receipt.request_id);
        store
            .finish_source_run(&run, SourceRunOutcome::NoChange, None, None)
            .await
            .unwrap();
    }
    // Exercise the production pruning function with smaller local limits. Public
    // limits stay unchanged; this reproduces >1024 no-change pages cheaply.
    let tenant = cap.tenant.clone();
    let id = connection.id.clone();
    store
        .call(move |db| prune_source_history(&db.conn, &tenant, &id, 3, 2))
        .await
        .unwrap();
    let document = store
        .source(&user, first.source_ids[0].clone(), None)
        .await
        .unwrap();
    assert_eq!(document.provenance.unwrap().request_id, first.request_id);
    assert_eq!(
        store
            .source_batch_receipt(&user, connection.id.clone(), first.request_id.clone())
            .await
            .unwrap()
            .sequence,
        1
    );
    assert_eq!(
        store
            .source_ingest_receipt(&cap, first.request_id.clone())
            .await
            .unwrap()
            .run_id,
        first_run.id
    );
    assert_eq!(
        store
            .commit_source_batch(&first_run, first_page)
            .await
            .unwrap()
            .sequence,
        1
    );
    let retained = store
        .source_run_lease(&cap, first_run.id.clone(), first_run.generation)
        .await
        .unwrap();
    assert_eq!(retained.generation, first_run.generation);
    assert!(matches!(
        store
            .source_ingest_receipt(&cap, oldest_unreferenced.unwrap())
            .await,
        Err(StoreError::NotFound)
    ));
    let tenant = cap.tenant.clone();
    let id = connection.id.clone();
    let counts = store.call(move |db| {
        let receipts:i64=db.conn.query_row("SELECT count(*) FROM source_batch_receipts WHERE tenant_id=?1 AND connection_id=?2",params![tenant,id],|r|r.get(0))?;
        let runs:i64=db.conn.query_row("SELECT count(*) FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2",params![tenant,id],|r|r.get(0))?;
        Ok((receipts,runs))
    }).await.unwrap();
    assert_eq!(counts, (4, 4)); // One pinned receipt/run plus three recent receipts.
    assert!(
        store
            .source_sync_runs(&user, connection.id.clone())
            .await
            .unwrap()
            .len()
            <= RUN_LIMIT as usize
    );
    store
        .delete_source(&user, first.source_ids[0].clone(), 1)
        .await
        .unwrap();
    let tenant = cap.tenant.clone();
    let id = connection.id;
    store
        .call(move |db| prune_source_history(&db.conn, &tenant, &id, 3, 2))
        .await
        .unwrap();
    assert!(matches!(
        store.source_ingest_receipt(&cap, first.request_id).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store
            .source_run_lease(&cap, first_run.id, first_run.generation)
            .await,
        Err(StoreError::Conflict)
    ));
}
