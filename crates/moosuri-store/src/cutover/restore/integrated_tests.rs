use super::*;
use crate::{
    KnowledgeToolInput, KnowledgeToolResult, PendingKnowledgeCall, PrivateCheckpoint, Store,
    UserContext,
    tests::{TEST_LOCK, admin, member},
};
use moosuri_core::*;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};
const TABLES: [&str; 7] = [
    "personal_artifacts",
    "personal_artifact_versions",
    "personal_artifact_dependencies",
    "personal_artifact_mutations",
    "personal_artifact_command_plans",
    "personal_artifact_run_dependencies",
    "personal_artifact_run_profiles",
];
type Rows = BTreeMap<String, Vec<Vec<String>>>;
fn artifact_state(conn: &Connection, committed_only: bool) -> crate::Result<Rows> {
    let mut state = BTreeMap::new();
    for table in TABLES {
        let filter = if committed_only && table == "personal_artifact_command_plans" {
            " WHERE used=1"
        } else {
            ""
        };
        let mut stmt = conn.prepare(&format!("SELECT * FROM {table}{filter}"))?;
        let width = stmt.column_count();
        let mut rows = stmt
            .query_map([], |r| {
                (0..width)
                    .map(|i| Ok(format!("{:?}", r.get_ref(i)?)))
                    .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.sort();
        state.insert(table.into(), rows);
    }
    Ok(state)
}
fn record(body: &str, refs: Vec<ArtifactDependency>) -> ArtifactWrite {
    ArtifactWrite {
        request_id: Uuid::new_v4().to_string(),
        id: None,
        expected_revision: None,
        draft: ArtifactDraft {
            title: "Restore proof document".into(),
            body: body.into(),
            payload: ArtifactPayload::Document,
            references: refs,
        },
    }
}
async fn plan(store: &Store, user: &UserContext) -> crate::agent_execution::AgentClaim {
    let t = store
        .create_thread(user, "Synthetic actual artifact command".into())
        .await
        .unwrap();
    store
        .submit_turn(
            user,
            t.id,
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
    store
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
    claim
}
async fn tool(
    store: &Store,
    claim: &crate::agent_execution::AgentClaim,
    input: ArtifactToolRequest,
) -> KnowledgeToolResult {
    let ctx = store.agent_context(&claim.lease).await.unwrap();
    let step = store
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "proof".into(),
                input: KnowledgeToolInput::Artifact { request: input },
            }],
        )
        .await
        .unwrap();
    store
        .execute_knowledge_tool(&step, "proof".into())
        .await
        .unwrap()
}
#[tokio::test]
async fn actual_current_schema_restores_artifacts_media_receipts_quarantine_and_suspended_auth() {
    let _g = TEST_LOCK.lock().await;
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let data = root.path().join("source");
    let dest = root.path().join("destination");
    let kit = root.path().join("kit");
    let heads = root.path().join("heads");
    for p in [&kit, &heads] {
        fs::DirBuilder::new().mode(0o700).create(p).unwrap()
    }
    let store = Store::open(&data).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "suspended@example.test").await.user;
    let provider = store
        .create_provider(
            &a,
            crate::ProviderChange {
                name: "Synthetic no-I/O restore binding".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: crate::CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(
            &a,
            provider.id.clone(),
            provider.revision,
            "synthetic".into(),
            None,
        )
        .await
        .unwrap();
    store
        .record_tool_probe(
            &a,
            provider.id.clone(),
            provider.revision,
            "synthetic".into(),
            None,
        )
        .await
        .unwrap();
    store
        .record_vision_probe(
            &a,
            provider.id.clone(),
            provider.revision,
            "synthetic".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &a,
            Some(ModelSelection {
                provider_id: provider.id,
                provider_revision: provider.revision,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let (pixels, _) = crate::media::vision_challenge().unwrap();
    let digest: [u8; 32] = Sha256::digest(&pixels).into();
    let mut image_ids = vec![];
    for owner in [&a, &b, &a] {
        let id = Uuid::new_v4().to_string();
        let crate::media::UploadAdmission::Process(upload) =
            store.media_begin(owner, id.clone(), digest).await.unwrap()
        else {
            panic!("new upload must be admitted")
        };
        store
            .media_finish(upload, Some(crate::media::normalize(&pixels).unwrap()))
            .await
            .unwrap();
        image_ids.push(id);
    }
    let ready_image = image_ids.remove(0);
    let other_image = image_ids.remove(0);
    let deleted_image = image_ids.remove(0);
    let canonical = store
        .media_bytes(&a, ready_image.clone(), false)
        .await
        .unwrap();
    let thumbnail = store
        .media_bytes(&a, ready_image.clone(), true)
        .await
        .unwrap();
    store.delete_media(&a, deleted_image.clone()).await.unwrap();
    let processing_image = Uuid::new_v4().to_string();
    let crate::media::UploadAdmission::Process(pending_upload) = store
        .media_begin(&a, processing_image.clone(), digest)
        .await
        .unwrap()
    else {
        panic!("pending upload")
    };
    let file_original = b"# Synthetic original\nquantity,37\n";
    let file_digest: [u8; 32] = Sha256::digest(file_original).into();
    let ready_file = Uuid::new_v4().to_string();
    let deleted_file = Uuid::new_v4().to_string();
    let other_file = Uuid::new_v4().to_string();
    for (owner, id) in [(&a, &ready_file), (&a, &deleted_file), (&b, &other_file)] {
        let crate::media::UploadAdmission::Process(upload) = store
            .file_begin(owner, id.clone(), file_digest, "restore.md".into())
            .await
            .unwrap()
        else {
            panic!("new file")
        };
        store
            .file_finish(
                upload,
                Some(
                    crate::private_files::extract(file_original.to_vec(), "text/markdown").unwrap(),
                ),
            )
            .await
            .unwrap();
    }
    store.delete_media(&a, deleted_file.clone()).await.unwrap();
    let processing_file = Uuid::new_v4().to_string();
    let crate::media::UploadAdmission::Process(pending_file) = store
        .file_begin(
            &a,
            processing_file.clone(),
            file_digest,
            "pending.txt".into(),
        )
        .await
        .unwrap()
    else {
        panic!("pending file")
    };
    let retained = store
        .write_artifact(&a, record("Retained editable current document", vec![]))
        .await
        .unwrap();
    let second = store
        .write_artifact(&b, record("Other tenant retained document", vec![]))
        .await
        .unwrap();
    let deleted = store
        .write_artifact(&a, record("Deleted body must not return", vec![]))
        .await
        .unwrap();
    let delete_input = ArtifactDelete {
        request_id: Uuid::new_v4().to_string(),
        expected_revision: 1,
    };
    let deletion = store
        .delete_artifact(&a, deleted.id.clone(), delete_input.clone())
        .await
        .unwrap();
    let collection = store
        .create_collection(
            &a,
            "Actual source withdrawal proof".into(),
            Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    let batch = |cursor, version: &str, text: &str, revision| IngestBatch {
        request_id: Uuid::new_v4().to_string(),
        expected_cursor: cursor,
        next_cursor: cursor + 1,
        complete_snapshot: false,
        entries: vec![SourceInput {
            external_id: "proof".into(),
            version: version.into(),
            expected_revision: revision,
            title: "Source proof".into(),
            text: text.into(),
            speaker: None,
            observed_at: 1,
            valid_from: None,
            valid_until: None,
        }],
    };
    let text = "Source version one withdrawn canary";
    let source = store
        .ingest_text(&a, collection.id.clone(), batch(0, "v1", text, None))
        .await
        .unwrap()
        .source_ids[0]
        .clone();
    let derived = store
        .write_artifact(
            &a,
            record(
                "Derived body must stay withdrawn",
                vec![ArtifactDependency {
                    kind: ArtifactDependencyKind::Source,
                    id: source.clone(),
                    revision: 1,
                }],
            ),
        )
        .await
        .unwrap();
    let memory = store
        .create_memory(
            &a,
            Uuid::new_v4().to_string(),
            MemoryDraft {
                text: "Derived memory withdrawn canary".into(),
                subject: "proof".into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![EvidenceRef {
                    source_id: source.clone(),
                    source_revision: 1,
                    start_byte: 0,
                    end_byte: text.len() as u32,
                    quote: text.into(),
                }],
            },
        )
        .await
        .unwrap();
    store
        .ingest_text(
            &a,
            collection.id,
            batch(1, "v2", "Current source replacement", Some(1)),
        )
        .await
        .unwrap();
    store.delete_source(&a, source.clone(), 2).await.unwrap();
    let committed = plan(&store, &a).await;
    tool(
        &store,
        &committed,
        ArtifactToolRequest::Read {
            id: retained.id.clone(),
            revision: 1,
            start_byte: 0,
            max_bytes: 1024,
        },
    )
    .await;
    let saved = tool(
        &store,
        &committed,
        ArtifactToolRequest::Write {
            title: "Actual command receipt".into(),
            body: "Stored once through actual command slot".into(),
            payload: ArtifactPayload::Document,
            anchor: None,
        },
    )
    .await;
    let KnowledgeToolResult::External(saved) = saved else {
        panic!("real receipt required")
    };
    let saved_id = saved["id"].as_str().unwrap().to_owned();
    let pending = plan(&store, &a).await;
    tool(
        &store,
        &pending,
        ArtifactToolRequest::Read {
            id: retained.id.clone(),
            revision: 1,
            start_byte: 0,
            max_bytes: 1024,
        },
    )
    .await;
    let reminder_thread = store
        .create_thread(&a, "Synthetic conversational restore receipt".into())
        .await
        .unwrap();
    let reminder = store
        .submit_turn_with_schedule_context(
            &a,
            reminder_thread.id.clone(),
            Uuid::new_v4().to_string(),
            "remind me in 1 day to keep the historical receipt".into(),
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(reminder.run.state, RunState::Completed);
    let reminder_receipt = reminder.run.schedule_receipts.clone().unwrap();
    assert_eq!(reminder_receipt.len(), 1);
    let vault_handoff = store
        .vault_handoff(&a, "https://vault.example.test".into())
        .await
        .unwrap();
    let vault_grant = store
        .vault_authority(moosuri_vault_protocol::AuthorityRequest::Exchange {
            handoff: vault_handoff.token,
            vault_origin: vault_handoff.vault_origin,
        })
        .await
        .unwrap()
        .grant
        .unwrap();
    let vault_operation = Uuid::new_v4();
    store
        .vault_authority(moosuri_vault_protocol::AuthorityRequest::Commit {
            grant: vault_grant.clone(),
            operation: vault_operation,
            digest: "a".repeat(64),
        })
        .await
        .unwrap();
    let push_device = Uuid::new_v4().to_string();
    let (_, p256dh) = moosuri_connectors::push::generate_vapid().unwrap();
    let push_binding = store
        .enroll_push(
            &a,
            crate::push::PushEnrollment {
                request_id: Uuid::new_v4().to_string(),
                device_id: push_device.clone(),
                operation_revision: 1,
                subscription: moosuri_connectors::push::PushSubscription {
                    endpoint: format!("https://fcm.googleapis.com/fcm/send/{}", Uuid::new_v4()),
                    p256dh,
                    auth: "_ordMnz7uTCmrpBTeUV4Bw".into(), // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
                },
            },
            "https://push-owner.example.test".into(),
        )
        .await
        .unwrap();
    let original_push_key = fs::read(data.join(crate::push::KEY_FILE)).unwrap();
    let at = crate::now().unwrap() + 60;
    let schedule = store
        .create_schedule(
            &a,
            ScheduleCreate {
                request_id: Uuid::new_v4().to_string(),
                draft: ScheduleDraft {
                    title: "Actual retained local reminder".into(),
                    message: "Notification is a committed local receipt".into(),
                    rule: ScheduleRule::Interval {
                        anchor: at,
                        seconds: 60,
                    },
                    valid_until: None,
                },
            },
        )
        .await
        .unwrap();
    store.enqueue_scheduled_reminders_at(at).await.unwrap();
    let delivered_lease = store
        .claim_scheduled_reminder_at(at)
        .await
        .unwrap()
        .unwrap();
    let notification = store
        .finish_scheduled_reminder_at(&delivered_lease, at)
        .await
        .unwrap();
    store.poll_push_queue().await.unwrap();
    let push_claim = store.claim_push().await.unwrap().unwrap();
    store.commit_push_dispatch(&push_claim.lease).await.unwrap();
    drop(push_claim); // Only a synthetic committed receipt; no network request is made.
    store.enqueue_scheduled_reminders_at(at + 60).await.unwrap();
    let old_claim = store
        .claim_scheduled_reminder_at(at + 60)
        .await
        .unwrap()
        .unwrap();
    let target = store
        .managed_accounts(&a)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.user.id == b.account().id)
        .unwrap();
    let prepared = store
        .prepare_account_management(
            &a,
            b.account().id.clone(),
            AccountManagementInput {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: target.revision,
                action: AccountManagementAction::Suspend,
            },
        )
        .await
        .unwrap();
    store.apply_account_management(prepared).await.unwrap();
    let (all, expected) = store
        .call(|db| {
            Ok((
                artifact_state(&db.conn, false)?,
                artifact_state(&db.conn, true)?,
            ))
        })
        .await
        .unwrap();
    for table in TABLES {
        assert!(!all[table].is_empty(), "must populate real table {table}")
    }
    assert_eq!(all["personal_artifact_command_plans"].len(), 2);
    let accounts = (a.account().id.clone(), b.account().id.clone());
    drop(store);
    let identity = kit.join("identity");
    let recipient = backup::keygen(&identity).unwrap();
    backup::setup(&data, &recipient, &identity).unwrap();
    let cipher = kit.join("snapshot.age");
    let trusted = heads.join("head.json");
    checkpoint(&data, &cipher, &trusted).unwrap();
    let before_db = fs::read(data.join("assistant.sqlite")).unwrap();
    restore(&cipher, &trusted, &identity, &dest).unwrap();
    assert_eq!(before_db, fs::read(data.join("assistant.sqlite")).unwrap());
    assert_eq!(
        fs::read(data.join(crate::push::KEY_FILE)).unwrap(),
        original_push_key
    );
    assert!(!dest.join(crate::push::KEY_FILE).exists());
    assert!(Store::open(&data).is_err());
    let conn = Connection::open(dest.join("assistant.sqlite")).unwrap();
    assert_eq!(conn.query_row("SELECT count(*) FROM push_subscriptions WHERE binding_id=?1 AND state='restore_required' AND credentials IS NULL AND endpoint_hash IS NULL AND session_digest IS NULL", [&push_binding.binding_id], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM push_queue WHERE state='quarantined' AND nonce IS NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT retired FROM push_vapid_meta", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM push_enrollment_receipts", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        1
    );
    for table in ["vault_handoffs", "vault_grants"] {
        assert_eq!(
            conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM vault_operation_commitments WHERE operation_id=?1",
            [vault_operation.to_string()],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(artifact_state(&conn, false).unwrap(), expected);
    assert_eq!(
        conn.query_row("SELECT count(*) FROM provider_vision_probes", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT state FROM media_objects WHERE id=?1",
            [&processing_image],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "retry_required"
    );
    assert!(
        conn.query_row(
            "SELECT nonce IS NULL FROM media_objects WHERE id=?1",
            [&processing_image],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert!(conn.query_row("SELECT state='deleted' AND bytes IS NULL AND thumbnail IS NULL FROM media_objects WHERE id=?1", [&deleted_image], |r| r.get::<_, bool>(0)).unwrap());
    assert_eq!(
        conn.query_row(
            "SELECT bytes FROM media_objects WHERE id=?1",
            [&other_image],
            |r| r.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        canonical
    );

    assert!(
        conn.query_row(
            "SELECT state='retry_required' AND nonce IS NULL FROM media_objects WHERE id=?1",
            [&processing_file],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert!(conn.query_row("SELECT state='deleted' AND bytes IS NULL AND extracted_text IS NULL FROM media_objects WHERE id=?1",[&deleted_file],|r|r.get::<_,bool>(0)).unwrap());
    assert_eq!(
        conn.query_row(
            "SELECT bytes FROM media_objects WHERE id=?1",
            [&ready_file],
            |r| r.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        file_original
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM personal_artifact_command_plans WHERE used=0",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT state FROM reminder_schedules WHERE id=?1",
            [&schedule.id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "paused"
    );
    assert_eq!(
        conn.query_row(
            "SELECT reason FROM reminder_schedules WHERE id=?1",
            [&schedule.id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "restore_quarantine"
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM reminder_occurrences WHERE state IN('queued','claimed')",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM local_notifications WHERE id=?1 AND occurrence_id=?2",
            params![notification.id, notification.occurrence_id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    drop(conn);
    crate::recover_account(
        &dest,
        accounts.0,
        Role::Admin,
        "New-proof-enrollment".into(),
    )
    .await
    .unwrap();
    crate::recover_account(
        &dest,
        accounts.1,
        Role::Member,
        "Suspended-proof-enrollment".into(),
    )
    .await
    .unwrap();
    let store = Store::open(&dest).unwrap();
    assert!(
        store
            .vault_authority(moosuri_vault_protocol::AuthorityRequest::Validate {
                grant: vault_grant
            })
            .await
            .is_err()
    );
    assert!(matches!(
        store
            .login(
                "suspended@example.test".into(),
                "Suspended-proof-enrollment".into()
            )
            .await,
        Err(crate::StoreError::Unauthorized)
    ));
    let restricted = store
        .login("admin@example.test".into(), "New-proof-enrollment".into())
        .await
        .unwrap();
    let current = store
        .change_password(&restricted.user, "New-proof-current-password".into())
        .await
        .unwrap()
        .user;
    assert!(!dest.join(crate::push::KEY_FILE).exists());
    let push_configuration = store
        .push_configuration(
            &current,
            push_device,
            "https://push-owner.example.test".into(),
        )
        .await
        .unwrap();
    assert_eq!(push_configuration.device.unwrap().state, "restore_required");
    assert_ne!(
        fs::read(dest.join(crate::push::KEY_FILE)).unwrap(),
        original_push_key
    );
    store.poll_push_queue().await.unwrap();
    assert!(store.claim_push().await.unwrap().is_none());
    assert_eq!(
        store
            .artifact(&current, retained.id.clone(), None)
            .await
            .unwrap()
            .draft
            .unwrap()
            .body,
        "Retained editable current document"
    );
    assert_eq!(
        store
            .artifact(&current, saved_id, None)
            .await
            .unwrap()
            .draft
            .unwrap()
            .body,
        "Stored once through actual command slot"
    );
    assert!(
        store
            .artifact(&current, derived.id.clone(), None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
    assert!(
        store
            .artifact_history(&current, derived.id, None)
            .await
            .unwrap()
            .items
            .into_iter()
            .all(|v| v.draft.is_none())
    );
    assert_eq!(
        store
            .artifact(&current, deleted.id.clone(), None)
            .await
            .unwrap()
            .artifact
            .status,
        ArtifactStatus::Deleted
    );
    assert!(
        store
            .artifact(&current, deleted.id.clone(), None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
    let replay = store
        .delete_artifact(&current, deleted.id, delete_input)
        .await
        .unwrap();
    assert_eq!(replay.request_id, deletion.request_id);
    assert_eq!(replay.revision, deletion.revision);
    assert!(matches!(
        store.artifact(&current, second.id, None).await,
        Err(crate::StoreError::NotFound)
    ));
    assert!(matches!(
        store.memory(&current, memory.id, None).await,
        Err(crate::StoreError::NotFound)
    ));
    assert!(store.source(&current, source, Some(1)).await.is_err());
    assert_eq!(
        store
            .notifications(&current, None, false)
            .await
            .unwrap()
            .items[0]
            .id,
        notification.id
    );
    let restored_schedules = store.schedules(&current).await.unwrap();
    assert_eq!(restored_schedules.len(), 2);
    assert!(restored_schedules.iter().all(|s| s.state == "paused"));
    let restored_reminder = store
        .conversation(&current, reminder_thread.id, None)
        .await
        .unwrap();
    assert_eq!(restored_reminder.turns[0].run.state, RunState::Completed);
    assert_eq!(
        serde_json::to_value(&restored_reminder.turns[0].run.schedule_receipts).unwrap(),
        serde_json::to_value(Some(reminder_receipt)).unwrap()
    );
    assert!(!restored_reminder.turns[0].run.can_retry);
    assert!(matches!(
        store
            .retry_run(&current, pending.lease.run_id().to_owned(), 1)
            .await,
        Err(crate::StoreError::Conflict)
    ));
    assert_eq!(
        store
            .media_bytes(&current, ready_image.clone(), false)
            .await
            .unwrap(),
        canonical
    );
    assert_eq!(
        store
            .media_bytes(&current, ready_image, true)
            .await
            .unwrap(),
        thumbnail
    );
    assert!(matches!(
        store.media_bytes(&current, deleted_image, false).await,
        Err(crate::StoreError::NotFound)
    ));
    assert!(matches!(
        store.media_bytes(&current, other_image, false).await,
        Err(crate::StoreError::NotFound)
    ));
    assert!(
        store
            .media_finish(
                pending_upload,
                Some(crate::media::normalize(&pixels).unwrap())
            )
            .await
            .is_err()
    );
    assert_eq!(
        store
            .file_bytes(&current, ready_file.clone())
            .await
            .unwrap(),
        file_original
    );
    assert_eq!(
        store
            .file_preview(&current, ready_file)
            .await
            .unwrap()
            .as_bytes(),
        file_original
    );
    for id in [deleted_file, other_file, processing_file] {
        assert!(matches!(
            store.file_bytes(&current, id.clone()).await,
            Err(crate::StoreError::NotFound)
        ));
        assert!(matches!(
            store.file_preview(&current, id).await,
            Err(crate::StoreError::NotFound)
        ));
    }
    assert!(
        store
            .file_finish(
                pending_file,
                Some(crate::private_files::extract(file_original.to_vec(), "text/plain").unwrap())
            )
            .await
            .is_err()
    );
    assert!(store.claim_agent_run().await.unwrap().is_none());
    assert!(
        store
            .finish_scheduled_reminder_at(&old_claim, at + 120)
            .await
            .is_err()
    );
    assert_eq!(
        store
            .enqueue_scheduled_reminders_at(at + 120)
            .await
            .unwrap(),
        0
    );
    assert!(
        store
            .claim_scheduled_reminder_at(at + 120)
            .await
            .unwrap()
            .is_none()
    );
    drop(store);
    let reopened = Store::open(&dest).unwrap();
    assert!(reopened.claim_agent_run().await.unwrap().is_none());
    assert!(
        reopened
            .claim_scheduled_reminder_at(at + 180)
            .await
            .unwrap()
            .is_none()
    );
}
