use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_core::*;
async fn configured(store: &Store) -> UserContext {
    let u = admin(store).await.user;
    let p = store
        .create_provider(
            &u,
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
        .record_model_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &u,
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
    u
}
fn change(revision: i64, enabled: bool) -> MemoryExtractionChange {
    MemoryExtractionChange {
        expected_revision: revision,
        enabled,
        attribution_mode: "specified".into(),
        reported_speaker: "Reported author".into(),
        daily_calls: 10,
        daily_input_bytes: 24000,
        daily_output_tokens: 4000,
    }
}
async fn collection(store: &Store, u: &UserContext) -> KnowledgeCollection {
    store
        .create_collection(u, "Synthetic statements".into(), Uuid::new_v4().to_string())
        .await
        .unwrap()
}
async fn source(
    store: &Store,
    u: &UserContext,
    c: &KnowledgeCollection,
    text: &str,
    speaker: Option<&str>,
) -> String {
    store
        .ingest_text(
            u,
            c.id.clone(),
            IngestBatch {
                request_id: Uuid::new_v4().to_string(),
                expected_cursor: c.cursor,
                next_cursor: c.cursor + 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: Uuid::new_v4().to_string(),
                    version: "1".into(),
                    expected_revision: None,
                    title: "Synthetic".into(),
                    text: text.into(),
                    speaker: speaker.map(str::to_owned),
                    observed_at: 100,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap()
        .source_ids
        .remove(0)
}
async fn updated_collection(store: &Store, u: &UserContext, id: &str) -> KnowledgeCollection {
    store
        .collections(u)
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.id == id)
        .unwrap()
}
#[tokio::test]
async fn opt_in_has_no_backfill_explicit_source_and_changed_revision_are_scoped() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let c = collection(&store, &u).await;
    let old = source(&store, &u, &c, "I avoid peanuts.", None).await;
    assert!(store.claim_memory_extraction().await.unwrap().is_none());
    store
        .set_memory_extraction(&u, c.id.clone(), change(0, true))
        .await
        .unwrap();
    assert!(store.claim_memory_extraction().await.unwrap().is_none());
    store
        .request_memory_extraction(&u, old.clone(), 1)
        .await
        .unwrap();
    let claim = store.claim_memory_extraction().await.unwrap().unwrap();
    assert_eq!(claim.reported_speaker, "Reported author");
    assert_eq!(claim.units[0].text, "I avoid peanuts.");
    assert!(
        store
            .admit_memory_extraction(&claim.lease, 3000)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .finish_memory_extraction(&claim.lease, vec![0], RunUsage::default())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .finish_memory_extraction(&claim.lease, vec![0], RunUsage::default())
            .await
            .unwrap(),
        1
    );
    let m = store.memories(&u, None).await.unwrap().items.remove(0);
    assert_eq!(m.draft.subject, "Reported author");
    assert_eq!(m.draft.speaker.as_deref(), Some("Reported author"));
    assert_eq!(m.draft.evidence[0].source_id, old);
    assert_eq!(m.extraction_origin.as_ref().unwrap().model, "synthetic");
    store.request_memory_extraction(&u, old, 1).await.unwrap();
    assert!(store.claim_memory_extraction().await.unwrap().is_none());
    let c = updated_collection(&store, &u, &c.id).await;
    source(&store, &u, &c, "I travel by train.", None).await;
    assert!(store.claim_memory_extraction().await.unwrap().is_some());
}
#[tokio::test]
async fn reported_speakers_are_literal_not_self_and_unknown_or_composite_labels_skip() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let c = collection(&store, &u).await;
    let mut setting = change(0, true);
    setting.attribution_mode = "reported".into();
    setting.reported_speaker.clear();
    store
        .set_memory_extraction(&u, c.id.clone(), setting)
        .await
        .unwrap();
    for label in [None, Some("Mina and Junho"), Some("mina@example.test")] {
        let c = updated_collection(&store, &u, &c.id).await;
        source(&store, &u, &c, "I avoid caffeine after 3 p.m.", label).await;
        let claim = store.claim_memory_extraction().await.unwrap();
        if label == Some("mina@example.test") {
            let claim = claim.unwrap();
            assert_eq!(claim.reported_speaker, "mina@example.test");
            store
                .admit_memory_extraction(&claim.lease, 3000)
                .await
                .unwrap();
            store
                .finish_memory_extraction(&claim.lease, vec![0], RunUsage::default())
                .await
                .unwrap();
        } else {
            assert!(claim.is_none());
            assert_eq!(
                store
                    .memory_extraction_status(&u, c.id)
                    .await
                    .unwrap()
                    .reason
                    .as_deref(),
                Some("unknown_attribution")
            );
        }
    }
    let m = store.memories(&u, None).await.unwrap().items.remove(0);
    assert_eq!(m.draft.speaker.as_deref(), Some("mina@example.test"));
    assert_ne!(m.draft.subject, "self");
}
#[tokio::test]
async fn revoked_background_grants_never_commit_held_or_new_work() {
    let _g = TEST_LOCK.lock().await;
    for boundary in 0..5 {
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let u = configured(&store).await;
        let c = collection(&store, &u).await;
        store
            .set_memory_extraction(&u, c.id.clone(), change(0, true))
            .await
            .unwrap();
        let id = source(&store, &u, &c, "I need a step-free entrance.", None).await;
        let claim = store.claim_memory_extraction().await.unwrap().unwrap();
        store
            .admit_memory_extraction(&claim.lease, 3000)
            .await
            .unwrap();
        match boundary {
            0 => {
                store
                    .set_memory_extraction(&u, c.id, change(1, false))
                    .await
                    .unwrap();
            }
            1 => {
                store.delete_source(&u, id, 1).await.unwrap();
            }
            2 => {
                let c = updated_collection(&store, &u, &c.id).await;
                store
                    .disconnect_collection(&u, c.id, c.revision, DisconnectMode::Keep)
                    .await
                    .unwrap();
            }
            3 => {
                store
                    .change_password(&u, "Synthetic-changed-owner-password".into())
                    .await
                    .unwrap();
            }
            _ => {
                let p = store.list_providers(&u).await.unwrap().providers.remove(0);
                store
                    .update_provider(
                        &u,
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
        }
        assert!(store.check_memory_extraction(&claim.lease).await.is_err());
        assert!(
            store
                .finish_memory_extraction(&claim.lease, vec![0], RunUsage::default())
                .await
                .is_err()
        );
        assert_eq!(
            store
                .call(|db| Ok(db.conn.query_row(
                    "SELECT count(*) FROM automatic_memory_origins",
                    [],
                    |r| r.get::<_, i64>(0)
                )?))
                .await
                .unwrap(),
            0
        );
    }
}
#[tokio::test]
async fn independent_principal_survives_logout_but_other_tenant_has_no_control() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let other = member(&store, &u, "other@example.test").await.user;
    let c = collection(&store, &other).await;
    assert!(matches!(
        store.memory_extraction_status(&u, c.id.clone()).await,
        Err(StoreError::NotFound)
    ));
    assert!(
        store
            .set_memory_extraction(&u, c.id.clone(), change(0, true))
            .await
            .is_err()
    );
    store
        .set_memory_extraction(&other, c.id.clone(), change(0, true))
        .await
        .unwrap();
    source(&store, &other, &c, "I commute by bicycle.", None).await;
    store.logout(&other).await.unwrap();
    let claim = store.claim_memory_extraction().await.unwrap().unwrap();
    assert!(
        store
            .admit_memory_extraction(&claim.lease, 3000)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .finish_memory_extraction(&claim.lease, vec![0], RunUsage::default())
            .await
            .unwrap(),
        1
    );
    assert!(store.memories(&u, None).await.unwrap().items.is_empty());
}
#[tokio::test]
async fn budget_is_reserved_once_and_ambiguous_dispatch_never_retries_after_restart() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let c = collection(&store, &u).await;
    let mut settings = change(0, true);
    settings.daily_calls = 1;
    store
        .set_memory_extraction(&u, c.id.clone(), settings)
        .await
        .unwrap();
    source(&store, &u, &c, "I prefer quiet rooms.", None).await;
    let claim = store.claim_memory_extraction().await.unwrap().unwrap();
    assert!(
        store
            .admit_memory_extraction(&claim.lease, 3500)
            .await
            .unwrap()
    );
    assert!(
        store
            .admit_memory_extraction(&claim.lease, 3500)
            .await
            .is_err()
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.claim_memory_extraction().await.unwrap().is_none());
    assert!(
        store
            .finish_memory_extraction(&claim.lease, vec![0], RunUsage::default())
            .await
            .is_err()
    );
    let c = updated_collection(&store, &u, &c.id).await;
    source(&store, &u, &c, "I use a screen reader.", None).await;
    let next = store.claim_memory_extraction().await.unwrap().unwrap();
    assert!(
        !store
            .admit_memory_extraction(&next.lease, 3000)
            .await
            .unwrap()
    );
    let s = store.memory_extraction_status(&u, c.id).await.unwrap();
    assert_eq!(s.state, "queued");
    assert_eq!(s.reason.as_deref(), Some("daily_budget"));
    store
        .call(|db| {
            let r: (u32, u32, u32) = db.conn.query_row(
                "SELECT calls,input_bytes,output_tokens FROM automatic_memory_budget",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            assert_eq!(r, (1, 3500, 1024));
            Ok(())
        })
        .await
        .unwrap();
}
#[tokio::test]
async fn forgetting_suppresses_same_source_quote_across_reenable_and_new_source_revision() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let c = collection(&store, &u).await;
    store
        .set_memory_extraction(&u, c.id.clone(), change(0, true))
        .await
        .unwrap();
    let id = source(
        &store,
        &u,
        &c,
        "I do not eat pork.\n\nI cycle to work.",
        None,
    )
    .await;
    let claim = store.claim_memory_extraction().await.unwrap().unwrap();
    store
        .admit_memory_extraction(&claim.lease, 3000)
        .await
        .unwrap();
    store
        .finish_memory_extraction(&claim.lease, vec![0], RunUsage::default())
        .await
        .unwrap();
    let memory = store.memories(&u, None).await.unwrap().items.remove(0);
    store.forget_memory(&u, memory.id, 1).await.unwrap();
    store.call(|db|{
        assert_eq!(db.conn.query_row("SELECT count(*) FROM automatic_memory_origins",[],|r|r.get::<_,i64>(0))?,0);
        let raw:i64=db.conn.query_row("SELECT count(*) FROM automatic_memory_facts WHERE typeof(reported_speaker_hash)!='blob' OR length(reported_speaker_hash)!=32",[],|r|r.get(0))?;
        assert_eq!(raw,0);Ok(())
    }).await.unwrap();
    store
        .set_memory_extraction(&u, c.id.clone(), change(1, false))
        .await
        .unwrap();
    store
        .set_memory_extraction(&u, c.id.clone(), change(2, true))
        .await
        .unwrap();
    store
        .request_memory_extraction(&u, id.clone(), 1)
        .await
        .unwrap();
    let claim = store.claim_memory_extraction().await.unwrap().unwrap();
    assert_eq!(claim.units.len(), 1);
    assert_eq!(claim.units[0].text, "I cycle to work.");
    store
        .admit_memory_extraction(&claim.lease, 3000)
        .await
        .unwrap();
    store
        .finish_memory_extraction(&claim.lease, vec![0], RunUsage::default())
        .await
        .unwrap();
    assert_eq!(store.memories(&u, None).await.unwrap().items.len(), 1);
    let explicit = MemoryDraft {
        text: "I do not eat pork.".into(),
        subject: "Explicit".into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    };
    store
        .create_memory(&u, Uuid::new_v4().to_string(), explicit)
        .await
        .unwrap();
    assert_eq!(store.memories(&u, None).await.unwrap().items.len(), 2);
}
#[tokio::test]
async fn suppression_and_source_change_fence_an_acquired_namespace_and_atomic_bad_selection() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let c = collection(&store, &u).await;
    store
        .set_memory_extraction(&u, c.id.clone(), change(0, true))
        .await
        .unwrap();
    let id = source(&store, &u, &c, "I avoid pork.\n\nI prefer trains.", None).await;
    let claim = store.claim_memory_extraction().await.unwrap().unwrap();
    store
        .admit_memory_extraction(&claim.lease, 3000)
        .await
        .unwrap();
    assert!(
        store
            .finish_memory_extraction(&claim.lease, vec![0, 99], RunUsage::default())
            .await
            .is_err()
    );
    assert!(store.memories(&u, None).await.unwrap().items.is_empty());
    store.delete_source(&u, id, 1).await.unwrap();
    assert!(
        store
            .finish_memory_extraction(&claim.lease, vec![1], RunUsage::default())
            .await
            .is_err()
    );
    store
        .call(|db| {
            let columns = db
                .conn
                .prepare("PRAGMA table_info(automatic_memory_jobs)")?
                .query_map([], |r| r.get::<_, String>(1))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            assert!(
                !columns
                    .iter()
                    .any(|c| ["text", "prompt", "response", "checkpoint"].contains(&c.as_str()))
            );
            Ok(())
        })
        .await
        .unwrap();
}
#[test]
fn complete_unicode_paragraphs_keep_negation_offsets_and_bounds() {
    let text = "나는 홍차를 좋아하지 않는다.\n\nOnly at work, I drink tea.\n\n";
    let u = units(text);
    assert_eq!(u.len(), 2);
    for unit in u {
        assert_eq!(
            &text[unit.start_byte as usize..unit.end_byte as usize],
            unit.text
        )
    }
    assert!(units(&"가".repeat(1400)).is_empty());
    assert!(!speaker_label("Mina, Junho"));
}
#[tokio::test]
async fn source_revision_change_supersedes_held_work_and_identical_fact_is_not_revived() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let c = collection(&store, &u).await;
    store
        .set_memory_extraction(&u, c.id.clone(), change(0, true))
        .await
        .unwrap();
    let id = source(&store, &u, &c, "I avoid caffeine.", None).await;
    let held = store.claim_memory_extraction().await.unwrap().unwrap();
    store
        .admit_memory_extraction(&held.lease, 3000)
        .await
        .unwrap();
    let c = updated_collection(&store, &u, &c.id).await;
    store
        .ingest_text(
            &u,
            c.id.clone(),
            IngestBatch {
                request_id: Uuid::new_v4().to_string(),
                expected_cursor: c.cursor,
                next_cursor: c.cursor + 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: store
                        .source(&u, id.clone(), None)
                        .await
                        .unwrap()
                        .source
                        .external_id,
                    version: "2".into(),
                    expected_revision: Some(1),
                    title: "Updated".into(),
                    text: "I avoid caffeine.".into(),
                    speaker: None,
                    observed_at: 101,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .finish_memory_extraction(&held.lease, vec![0], RunUsage::default())
            .await
            .is_err()
    );
    let current = store.claim_memory_extraction().await.unwrap().unwrap();
    store
        .admit_memory_extraction(&current.lease, 3000)
        .await
        .unwrap();
    assert_eq!(
        store
            .finish_memory_extraction(&current.lease, vec![0], RunUsage::default())
            .await
            .unwrap(),
        1
    );
    let m = store.memories(&u, None).await.unwrap().items.remove(0);
    assert_eq!(m.draft.evidence[0].source_revision, 2);
    let c = updated_collection(&store, &u, &c.id).await;
    store
        .ingest_text(
            &u,
            c.id.clone(),
            IngestBatch {
                request_id: Uuid::new_v4().to_string(),
                expected_cursor: c.cursor,
                next_cursor: c.cursor + 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: store
                        .source(&u, id.clone(), None)
                        .await
                        .unwrap()
                        .source
                        .external_id,
                    version: "3".into(),
                    expected_revision: Some(2),
                    title: "New version same fact".into(),
                    text: "I avoid caffeine.".into(),
                    speaker: None,
                    observed_at: 102,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap();
    let current = store.claim_memory_extraction().await.unwrap().unwrap();
    store
        .admit_memory_extraction(&current.lease, 3000)
        .await
        .unwrap();
    assert_eq!(
        store
            .finish_memory_extraction(&current.lease, vec![0], RunUsage::default())
            .await
            .unwrap(),
        0
    );
    let items = store.memories(&u, None).await.unwrap().items;
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, m.id);
    assert_eq!(items[0].status, MemoryStatus::Invalidated);
}
#[tokio::test]
async fn byte_budget_and_late_usage_survive_revocation_without_reauthorizing_mutation() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let c = collection(&store, &u).await;
    let mut setting = change(0, true);
    setting.daily_input_bytes = 3000;
    store
        .set_memory_extraction(&u, c.id.clone(), setting)
        .await
        .unwrap();
    source(&store, &u, &c, "I prefer quiet rooms.", None).await;
    let held = store.claim_memory_extraction().await.unwrap().unwrap();
    assert!(
        !store
            .admit_memory_extraction(&held.lease, 3001)
            .await
            .unwrap()
    );
    assert!(store.claim_memory_extraction().await.unwrap().is_none());
    store
        .set_memory_extraction(&u, c.id.clone(), change(1, true))
        .await
        .unwrap();
    let c = updated_collection(&store, &u, &c.id).await;
    let id = source(&store, &u, &c, "I need captions.", None).await;
    let held = store.claim_memory_extraction().await.unwrap().unwrap();
    store
        .admit_memory_extraction(&held.lease, 3000)
        .await
        .unwrap();
    store.delete_source(&u, id, 1).await.unwrap();
    store
        .record_memory_extraction_usage(
            &held.lease,
            RunUsage {
                input_tokens: Some(123),
                output_tokens: Some(7),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    store
        .call(|db| {
            let usage: String = db.conn.query_row(
                "SELECT usage FROM automatic_memory_jobs WHERE dispatch_nonce IS NOT NULL",
                [],
                |r| r.get(0),
            )?;
            let u: RunUsage = decode(&usage)?;
            assert_eq!(u.input_tokens, Some(123));
            assert_eq!(u.output_tokens, Some(7));
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        store
            .finish_memory_extraction(&held.lease, vec![0], RunUsage::default())
            .await
            .is_err()
    );
}

#[test]
fn crlf_and_mixed_paragraphs_preserve_literal_bytes_and_skip_only_oversized_blocks() {
    let text = format!(
        "나는 홍차를 마시지 않는다.\r\n\r\n{}\r\n\r\nI drink water.\n\n끝 문단",
        "가".repeat(1400)
    );
    let extracted = units(&text);
    assert_eq!(extracted.len(), 3);
    assert_eq!(extracted[0].text, "나는 홍차를 마시지 않는다.\r\n\r\n");
    assert_eq!(extracted[1].text, "I drink water.\n\n");
    assert_eq!(extracted[2].text, "끝 문단");
    for unit in &extracted {
        assert_eq!(
            unit.text,
            text[unit.start_byte as usize..unit.end_byte as usize]
        );
        assert!(unit.text.len() <= 4096);
    }
}

#[tokio::test]
async fn crlf_mail_larger_than_one_unit_keeps_exact_selected_paragraph_evidence() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let group = collection(&store, &user).await;
    store
        .set_memory_extraction(&user, group.id.clone(), change(0, true))
        .await
        .unwrap();
    let first = format!("{}\r\n\r\n", "Public meeting notes. ".repeat(110));
    let second = "나는 홍차를 마시지 않는다. ".repeat(55);
    let text = format!("{first}{second}");
    assert!(text.len() > 4096 && text.len() < 8192);
    assert!(first.len() <= 4096 && second.len() <= 4096);
    let id = source(&store, &user, &group, &text, None).await;
    let claim = store.claim_memory_extraction().await.unwrap().unwrap();
    assert_eq!(claim.units.len(), 2);
    assert_eq!(claim.units[1].text, second);
    assert!(
        store
            .admit_memory_extraction(&claim.lease, 7000)
            .await
            .unwrap()
    );
    assert_eq!(
        store
            .finish_memory_extraction(&claim.lease, vec![1], RunUsage::default())
            .await
            .unwrap(),
        1
    );
    let saved = store.memories(&user, None).await.unwrap().items.remove(0);
    assert_eq!(saved.draft.text, second);
    let evidence = &saved.draft.evidence[0];
    assert_eq!(evidence.source_id, id);
    assert_eq!(evidence.start_byte as usize, first.len());
    assert_eq!(evidence.end_byte as usize, text.len());
    assert_eq!(evidence.quote, text[first.len()..]);
    assert_eq!(
        store
            .memory_extraction_status(&user, group.id)
            .await
            .unwrap()
            .reason,
        None
    );
}
