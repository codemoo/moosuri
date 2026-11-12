use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
fn source_input(external: &str, text: &str) -> SourceInput {
    SourceInput {
        external_id: external.into(),
        version: "v1".into(),
        expected_revision: None,
        title: "합성 자료".into(),
        text: text.into(),
        speaker: Some("본인".into()),
        observed_at: 1_700_000_000,
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
        subject: "본인 선호".into(),
        speaker: Some("본인".into()),
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    }
}
fn query(kind: KnowledgeKind, text: &str) -> KnowledgeQuery {
    KnowledgeQuery {
        kind,
        text: text.into(),
        collection_id: None,
        subject: None,
        speaker: None,
        valid_at: None,
        cursor: None,
    }
}
async fn group(store: &Store, user: &UserContext) -> KnowledgeCollection {
    store
        .create_collection(user, "수동 자료".into(), Uuid::new_v4().to_string())
        .await
        .unwrap()
}
async fn new_memory(store: &Store, user: &UserContext, text: &str) -> MemoryDocument {
    store
        .create_memory(user, Uuid::new_v4().to_string(), draft(text))
        .await
        .unwrap()
}
async fn backed(store: &Store, user: &UserContext, source: &str, text: &str) -> MemoryDocument {
    let mut value = draft(text);
    value.evidence.push(EvidenceRef {
        source_id: source.into(),
        source_revision: 1,
        start_byte: 0,
        end_byte: text.len() as u32,
        quote: text.into(),
    });
    store
        .create_memory(user, Uuid::new_v4().to_string(), value)
        .await
        .unwrap()
}
#[tokio::test]
async fn batch_receipt_cursor_cas_and_failed_batch_are_atomic() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let group = group(&store, &user).await;
    let input = batch(0, vec![source_input("one", "원문 하나")]);
    let receipt = store
        .ingest_text(&user, group.id.clone(), input.clone())
        .await
        .unwrap();
    let duplicate = store
        .ingest_text(&user, group.id.clone(), input.clone())
        .await
        .unwrap();
    assert_eq!(receipt.source_ids, duplicate.source_ids);
    let mut altered = input.clone();
    altered.entries[0].text = "changed payload".into();
    assert!(matches!(
        store.ingest_text(&user, group.id.clone(), altered).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .ingest_text(
                &user,
                group.id.clone(),
                batch(0, vec![source_input("two", "stale")])
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let mut first = source_input("one", "새 원문");
    first.version = "v2".into();
    first.expected_revision = Some(1);
    let mut invalid = source_input("absent", "invalid expected revision");
    invalid.expected_revision = Some(1);
    assert!(matches!(
        store
            .ingest_text(&user, group.id.clone(), batch(1, vec![first, invalid]))
            .await,
        Err(StoreError::Conflict)
    ));
    let saved = store
        .source(&user, receipt.source_ids[0].clone(), None)
        .await
        .unwrap();
    assert_eq!(saved.source.revision, 1);
    assert_eq!(saved.text, "원문 하나");
    assert_eq!(store.collections(&user).await.unwrap()[0].cursor, 1);
    let mut snapshot = batch(1, vec![source_input("two", "partial")]);
    snapshot.complete_snapshot = true;
    assert!(matches!(
        store.ingest_text(&user, group.id.clone(), snapshot).await,
        Err(StoreError::InvalidInput)
    ));
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(
        store
            .ingest_text(&user, group.id, input)
            .await
            .unwrap()
            .source_ids,
        receipt.source_ids
    );
}
#[tokio::test]
async fn tenant_scoping_admin_has_no_override_and_korean_search_is_literal() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "member@example.test").await.user;
    let ga = group(&store, &a).await;
    let gb = group(&store, &b).await;
    let ra = store
        .ingest_text(
            &a,
            ga.id.clone(),
            batch(
                0,
                vec![source_input("same", "오늘 회의 일정은 합성자료입니다")],
            ),
        )
        .await
        .unwrap();
    let rb = store
        .ingest_text(
            &b,
            gb.id.clone(),
            batch(0, vec![source_input("same", "다른 사용자의 회의 일정")]),
        )
        .await
        .unwrap();
    let ma = new_memory(&store, &a, "공유 문구 A").await;
    let mb = new_memory(&store, &b, "공유 문구 B").await;
    for text in ["회의일정", "회의 일정", "일정"] {
        let hits = store
            .search_knowledge(&a, query(KnowledgeKind::Source, text))
            .await
            .unwrap();
        assert_eq!(hits.hits.len(), 1);
        assert_eq!(hits.hits[0].id, ra.source_ids[0]);
    }
    let hits = store
        .search_knowledge(&a, query(KnowledgeKind::Memory, "공유"))
        .await
        .unwrap();
    assert_eq!(hits.hits.len(), 1);
    assert_eq!(hits.hits[0].id, ma.id);
    assert!(matches!(
        store.source(&a, rb.source_ids[0].clone(), None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.memory(&a, mb.id.clone(), None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.forget_memory(&a, mb.id, 1).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.sources(&a, gb.id, None).await,
        Err(StoreError::NotFound)
    ));
    let injected = store
        .search_knowledge(&a, query(KnowledgeKind::Source, "\" OR tenant_key:*"))
        .await
        .unwrap();
    assert!(injected.hits.is_empty());
    store.logout(&a).await.unwrap();
    assert!(matches!(
        store
            .search_knowledge(&a, query(KnowledgeKind::Memory, "공유"))
            .await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn exact_evidence_invalidates_on_source_change_and_correction_keeps_identity() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let group = group(&store, &user).await;
    let receipt = store
        .ingest_text(
            &user,
            group.id.clone(),
            batch(0, vec![source_input("one", "나는 차를 좋아합니다")]),
        )
        .await
        .unwrap();
    let object = receipt.source_ids[0].clone();
    let memory = backed(&store, &user, &object, "나는 차를 좋아합니다").await;
    let mut wrong = memory.draft.clone();
    wrong.evidence[0].start_byte = 1;
    assert!(matches!(
        store
            .create_memory(&user, Uuid::new_v4().to_string(), wrong)
            .await,
        Err(StoreError::InvalidInput)
    ));
    let mut next = source_input("one", "나는 커피를 좋아합니다");
    next.expected_revision = Some(1);
    next.version = "v2".into();
    store
        .ingest_text(&user, group.id, batch(1, vec![next]))
        .await
        .unwrap();
    assert_eq!(
        store
            .memory(&user, memory.id.clone(), None)
            .await
            .unwrap()
            .status,
        MemoryStatus::Invalidated
    );
    assert!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Memory, "차를"))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    let mut corrected = draft("나는 커피를 좋아합니다");
    corrected.evidence.push(EvidenceRef {
        source_id: object,
        source_revision: 2,
        start_byte: 0,
        end_byte: corrected.text.len() as u32,
        quote: corrected.text.clone(),
    });
    let request = Uuid::new_v4().to_string();
    let corrected = store
        .correct_memory(
            &user,
            memory.id.clone(),
            1,
            request.clone(),
            corrected.clone(),
        )
        .await
        .unwrap();
    assert_eq!(corrected.id, memory.id);
    assert_eq!(corrected.revision, 2);
    assert_eq!(corrected.status, MemoryStatus::Active);
    assert_eq!(
        store
            .correct_memory(
                &user,
                memory.id.clone(),
                1,
                request,
                corrected.draft.clone()
            )
            .await
            .unwrap()
            .revision,
        2
    );
    assert!(matches!(
        store
            .correct_memory(
                &user,
                memory.id.clone(),
                1,
                Uuid::new_v4().to_string(),
                draft("stale editor draft")
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let history = store.memory_history(&user, memory.id, None).await.unwrap();
    assert_eq!(history.items.len(), 2);
    assert_eq!(history.items[1].draft.text, "나는 차를 좋아합니다");
}
#[tokio::test]
async fn current_and_historical_validity_filters_do_not_expose_future_facts() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let time = now().unwrap();
    let mut future = draft("미래 선호 커피");
    future.valid_from = Some(time + 1000);
    let saved = store
        .create_memory(&user, Uuid::new_v4().to_string(), future)
        .await
        .unwrap();
    assert!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Memory, "커피"))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    let mut later = query(KnowledgeKind::Memory, "커피");
    later.valid_at = Some(time + 1001);
    assert_eq!(
        store.search_knowledge(&user, later).await.unwrap().hits[0].id,
        saved.id
    );
    let mut changed = draft("현재 선호 녹차");
    changed.valid_until = Some(time + 10);
    store
        .correct_memory(
            &user,
            saved.id.clone(),
            1,
            Uuid::new_v4().to_string(),
            changed,
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Memory, "녹차"))
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    let mut expired = query(KnowledgeKind::Memory, "녹차");
    expired.valid_at = Some(time + 10);
    assert!(
        store
            .search_knowledge(&user, expired)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        store
            .memory(&user, saved.id, Some(1))
            .await
            .unwrap()
            .draft
            .text,
        "미래 선호 커피"
    );
}
#[tokio::test]
async fn forget_source_deletion_and_disconnect_have_distinct_scopes_without_resurrection() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let group = group(&store, &user).await;
    let input = batch(0, vec![source_input("one", "합성 망각 카나리")]);
    let receipt = store
        .ingest_text(&user, group.id.clone(), input.clone())
        .await
        .unwrap();
    let source = receipt.source_ids[0].clone();
    let memory = backed(&store, &user, &source, "합성 망각 카나리").await;
    let independent = new_memory(&store, &user, "별도 사용자 주장").await;
    store
        .forget_memory(&user, memory.id.clone(), 1)
        .await
        .unwrap();
    store
        .forget_memory(&user, memory.id.clone(), 1)
        .await
        .unwrap();
    assert!(matches!(
        store.memory_history(&user, memory.id.clone(), None).await,
        Err(StoreError::NotFound)
    ));
    assert!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Memory, "카나리"))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Source, "카나리"))
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    assert!(matches!(
        store
            .correct_memory(
                &user,
                memory.id,
                1,
                Uuid::new_v4().to_string(),
                draft("late forgotten text")
            )
            .await,
        Err(StoreError::NotFound)
    ));
    let dependent = backed(&store, &user, &source, "합성 망각 카나리").await;
    let group = store.collections(&user).await.unwrap().remove(0);
    let kept = store
        .disconnect_collection(
            &user,
            group.id.clone(),
            group.revision,
            DisconnectMode::Keep,
        )
        .await
        .unwrap();
    assert!(!kept.connected);
    assert!(!kept.deleted);
    assert_eq!(
        store
            .source(&user, source.clone(), None)
            .await
            .unwrap()
            .text,
        "합성 망각 카나리"
    );
    assert!(matches!(
        store
            .ingest_text(
                &user,
                group.id.clone(),
                batch(1, vec![source_input("two", "not ingested")])
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let deleted = store
        .disconnect_collection(
            &user,
            group.id.clone(),
            kept.revision,
            DisconnectMode::Delete,
        )
        .await
        .unwrap();
    assert!(deleted.deleted);
    assert!(matches!(
        store.memory(&user, dependent.id, None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.source(&user, source, None).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        store
            .memory(&user, independent.id, None)
            .await
            .unwrap()
            .draft
            .text,
        "별도 사용자 주장"
    );
    assert_eq!(
        store
            .ingest_text(&user, group.id, input)
            .await
            .unwrap()
            .source_ids,
        receipt.source_ids
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Source, "카나리"))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
}
#[tokio::test]
async fn search_and_list_cursors_are_bound_to_generation_query_and_tenant() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    for _ in 0..21 {
        new_memory(&store, &user, "검색 페이지 공유 문구").await;
    }
    let first = store
        .search_knowledge(&user, query(KnowledgeKind::Memory, "공유"))
        .await
        .unwrap();
    assert_eq!(first.hits.len(), 20);
    let cursor = first.next_cursor.unwrap();
    let mut next = query(KnowledgeKind::Memory, "공유");
    next.cursor = Some(cursor.clone());
    assert_eq!(
        store
            .search_knowledge(&user, next.clone())
            .await
            .unwrap()
            .hits
            .len(),
        1
    );
    next.text = "다른 검색".into();
    assert!(matches!(
        store.search_knowledge(&user, next).await,
        Err(StoreError::Conflict)
    ));
    let page = store.memories(&user, None).await.unwrap();
    new_memory(&store, &user, "새 항목").await;
    let mut stale = query(KnowledgeKind::Memory, "공유");
    stale.cursor = Some(cursor);
    assert!(matches!(
        store.search_knowledge(&user, stale).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store.memories(&user, page.next_after).await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn destructive_journal_replays_append_before_db_commit_and_rejects_truncation() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let memory = new_memory(&store, &user, "合成 journal private canary").await;
    let tenant = user.tenant.0.clone();
    let object = memory.id.clone();
    store
        .call(move |db| {
            db.knowledge_journal.append(&tenant, &object, 1)?;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.memory(&user, memory.id, None).await,
        Err(StoreError::NotFound)
    ));
    let journal = std::fs::read(dir.path().join("knowledge-invalidations.journal")).unwrap();
    assert!(
        !journal
            .windows(b"private canary".len())
            .any(|w| w == b"private canary")
    );
    drop(store);
    std::fs::OpenOptions::new()
        .write(true)
        .open(dir.path().join("knowledge-invalidations.journal"))
        .unwrap()
        .set_len(0)
        .unwrap();
    assert!(matches!(Store::open(dir.path()), Err(StoreError::Corrupt)));
}

#[tokio::test]
async fn future_correction_preserves_prior_fact_until_start_but_expiry_does_not_revive_it() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let time = now().unwrap();
    let old = new_memory(&store, &user, "현재 선호 녹차입니다").await;
    let mut future = draft("미래 선호 커피입니다");
    future.valid_from = Some(time + 100);
    future.valid_until = Some(time + 200);
    store
        .correct_memory(&user, old.id.clone(), 1, Uuid::new_v4().to_string(), future)
        .await
        .unwrap();
    let current = store
        .search_knowledge(&user, query(KnowledgeKind::Memory, "녹차입니다"))
        .await
        .unwrap();
    assert_eq!(current.hits.len(), 1);
    assert_eq!(current.hits[0].revision, 1);
    assert!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Memory, "커피입니다"))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    let mut middle = query(KnowledgeKind::Memory, "커피입니다");
    middle.valid_at = Some(time + 150);
    let found = store.search_knowledge(&user, middle).await.unwrap();
    assert_eq!(found.hits[0].revision, 2);
    for text in ["녹차입니다", "커피입니다"] {
        let mut expired = query(KnowledgeKind::Memory, text);
        expired.valid_at = Some(time + 250);
        assert!(
            store
                .search_knowledge(&user, expired)
                .await
                .unwrap()
                .hits
                .is_empty()
        );
    }
}

#[tokio::test]
async fn source_delete_purges_dependent_history_but_keeps_independent_correction() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let group = group(&store, &user).await;
    let input = batch(0, vec![source_input("one", "합성 과거 근거")]);
    let receipt = store
        .ingest_text(&user, group.id.clone(), input.clone())
        .await
        .unwrap();
    let source = receipt.source_ids[0].clone();
    let memory = backed(&store, &user, &source, "합성 과거 근거").await;
    let dependent = backed(&store, &user, &source, "합성 과거 근거").await;
    let request = Uuid::new_v4().to_string();
    let revised = draft("독립 수정 사실");
    store
        .correct_memory(
            &user,
            memory.id.clone(),
            1,
            request.clone(),
            revised.clone(),
        )
        .await
        .unwrap();
    store.delete_source(&user, source.clone(), 1).await.unwrap();
    store.delete_source(&user, source.clone(), 1).await.unwrap();
    assert!(matches!(
        store.source(&user, source.clone(), Some(1)).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.memory(&user, dependent.id, None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.memory(&user, memory.id.clone(), Some(1)).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        store
            .memory_history(&user, memory.id.clone(), None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(
        store
            .correct_memory(&user, memory.id.clone(), 1, request, revised)
            .await
            .unwrap()
            .revision,
        2
    );
    assert_eq!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Memory, "독립수정사실"))
            .await
            .unwrap()
            .hits[0]
            .id,
        memory.id
    );
    assert!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Memory, "과거근거"))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    let mut stale = source_input("one", "합성 과거 근거");
    stale.expected_revision = Some(1);
    assert!(matches!(
        store
            .ingest_text(&user, group.id.clone(), batch(1, vec![stale]))
            .await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store
            .ingest_text(&user, group.id, input)
            .await
            .unwrap()
            .source_ids,
        receipt.source_ids
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.source(&user, source, None).await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn cross_tenant_evidence_is_rejected_before_any_memory_is_written() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "evidence@example.test").await.user;
    let group = group(&store, &b).await;
    let receipt = store
        .ingest_text(
            &b,
            group.id,
            batch(0, vec![source_input("one", "다른 사용자 합성 근거")]),
        )
        .await
        .unwrap();
    let mut value = draft("불허 근거 주장");
    value.evidence.push(EvidenceRef {
        source_id: receipt.source_ids[0].clone(),
        source_revision: 1,
        start_byte: 0,
        end_byte: "다른 사용자 합성 근거".len() as u32,
        quote: "다른 사용자 합성 근거".into(),
    });
    assert!(matches!(
        store
            .create_memory(&a, Uuid::new_v4().to_string(), value)
            .await,
        Err(StoreError::NotFound)
    ));
    assert!(store.memories(&a, None).await.unwrap().items.is_empty());
}

#[tokio::test]
async fn source_update_invalidates_old_temporal_evidence_without_hiding_independent_revisions() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let group = group(&store, &user).await;
    let time = now().unwrap();
    let receipt = store
        .ingest_text(
            &user,
            group.id.clone(),
            batch(0, vec![source_input("one", "원문 선호 녹차")]),
        )
        .await
        .unwrap();
    let source = receipt.source_ids[0].clone();
    let memory = backed(&store, &user, &source, "원문 선호 녹차").await;
    let mut future = draft("독립 미래 커피");
    future.valid_from = Some(time + 100);
    store
        .correct_memory(
            &user,
            memory.id.clone(),
            1,
            Uuid::new_v4().to_string(),
            future,
        )
        .await
        .unwrap();
    let mut next = source_input("one", "원문 수정");
    next.version = "v2".into();
    next.expected_revision = Some(1);
    store
        .ingest_text(&user, group.id, batch(1, vec![next]))
        .await
        .unwrap();
    assert_eq!(
        store
            .memory(&user, memory.id.clone(), Some(1))
            .await
            .unwrap()
            .status,
        MemoryStatus::Invalidated
    );
    assert!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Memory, "녹차"))
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    let mut later = query(KnowledgeKind::Memory, "미래커피");
    later.valid_at = Some(time + 101);
    assert_eq!(
        store.search_knowledge(&user, later).await.unwrap().hits[0].revision,
        2
    );
    // Invalidating a future current assertion must leave an earlier independent one searchable until its start.
    let other = new_memory(&store, &user, "현재 독립 홍차").await;
    let mut future = draft("미래 근거 수정");
    future.valid_from = Some(time + 100);
    future.evidence.push(EvidenceRef {
        source_id: source,
        source_revision: 2,
        start_byte: 0,
        end_byte: "원문 수정".len() as u32,
        quote: "원문 수정".into(),
    });
    store
        .correct_memory(
            &user,
            other.id.clone(),
            1,
            Uuid::new_v4().to_string(),
            future,
        )
        .await
        .unwrap();
    let mut next = source_input("one", "또 원문 수정");
    next.version = "v3".into();
    next.expected_revision = Some(2);
    let group = store.collections(&user).await.unwrap().remove(0);
    store
        .ingest_text(&user, group.id, batch(2, vec![next]))
        .await
        .unwrap();
    assert_eq!(
        store.memory(&user, other.id, None).await.unwrap().status,
        MemoryStatus::Invalidated
    );
    assert_eq!(
        store
            .search_knowledge(&user, query(KnowledgeKind::Memory, "독립홍차"))
            .await
            .unwrap()
            .hits[0]
            .revision,
        1
    );
    let mut later = query(KnowledgeKind::Memory, "독립홍차");
    later.valid_at = Some(time + 101);
    assert!(
        store
            .search_knowledge(&user, later)
            .await
            .unwrap()
            .hits
            .is_empty()
    );
}

#[tokio::test]
async fn collection_journal_replay_keeps_or_deletes_and_corruption_fails_closed() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let group = group(&store, &user).await;
    let receipt = store
        .ingest_text(
            &user,
            group.id.clone(),
            batch(0, vec![source_input("one", "복구 합성 원문")]),
        )
        .await
        .unwrap();
    let tenant = user.tenant.0.clone();
    let object = group.id.clone();
    store
        .call(move |db| {
            db.knowledge_journal.append(&tenant, &object, 4)?;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let kept = store.collections(&user).await.unwrap().remove(0);
    assert!(!kept.connected);
    assert!(!kept.deleted);
    assert!(
        store
            .source(&user, receipt.source_ids[0].clone(), None)
            .await
            .is_ok()
    );
    let tenant = user.tenant.0.clone();
    let object = group.id;
    store
        .call(move |db| {
            db.knowledge_journal.append(&tenant, &object, 3)?;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.collections(&user).await.unwrap()[0].deleted);
    assert!(matches!(
        store
            .source(&user, receipt.source_ids[0].clone(), None)
            .await,
        Err(StoreError::NotFound)
    ));
    drop(store);
    let path = dir.path().join("knowledge-invalidations.journal");
    let valid = std::fs::read(&path).unwrap();
    std::fs::write(&path, &valid[..valid.len() - 1]).unwrap();
    assert!(matches!(Store::open(dir.path()), Err(StoreError::Corrupt)));
    let mut corrupt = valid.clone();
    corrupt[88] ^= 1;
    std::fs::write(&path, &corrupt).unwrap();
    assert!(matches!(Store::open(dir.path()), Err(StoreError::Corrupt)));
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(Store::open(dir.path()), Err(StoreError::Corrupt)));
}

#[tokio::test]
async fn an_existing_knowledge_installation_never_initializes_a_missing_journal() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    new_memory(&store, &user, "지워지지 않은 합성 기억").await;
    drop(store);
    std::fs::remove_file(dir.path().join("knowledge-invalidations.journal")).unwrap();
    assert!(matches!(Store::open(dir.path()), Err(StoreError::Corrupt)));
}

#[tokio::test]
async fn short_search_reports_its_work_bound_and_never_counts_other_tenants() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "bounded@example.test").await.user;
    let group = group(&store, &a).await;
    for start in (0..1001).step_by(32) {
        let entries = (start..(start + 32).min(1001))
            .map(|i| source_input(&format!("row-{i}"), "합성 홍차 자료"))
            .collect();
        store
            .ingest_text(&a, group.id.clone(), batch(start / 32, entries))
            .await
            .unwrap();
    }
    let result = store
        .search_knowledge(&a, query(KnowledgeKind::Source, "차"))
        .await
        .unwrap();
    assert!(result.truncated);
    assert_eq!(result.hits.len(), 20);
    assert!(result.next_cursor.is_some());
    let result = store
        .search_knowledge(&a, query(KnowledgeKind::Source, "홍차자료"))
        .await
        .unwrap();
    assert!(!result.truncated);
    assert_eq!(result.hits.len(), 20);
    let result = store
        .search_knowledge(&b, query(KnowledgeKind::Source, "차"))
        .await
        .unwrap();
    assert!(!result.truncated);
    assert!(result.hits.is_empty());
}

#[tokio::test]
async fn purged_intermediate_revision_still_supersedes_older_fact_until_future_correction() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let group = group(&store, &user).await;
    let time = now().unwrap();
    let receipt = store
        .ingest_text(
            &user,
            group.id,
            batch(0, vec![source_input("one", "합성 삭제 중간 근거")]),
        )
        .await
        .unwrap();
    let source = receipt.source_ids[0].clone();
    let mut first = draft("독립 과거 녹차");
    first.valid_from = Some(time - 300);
    let memory = store
        .create_memory(&user, Uuid::new_v4().to_string(), first)
        .await
        .unwrap();
    let mut middle = draft("합성 삭제 중간 근거");
    middle.valid_from = Some(time - 200);
    middle.evidence.push(EvidenceRef {
        source_id: source.clone(),
        source_revision: 1,
        start_byte: 0,
        end_byte: middle.text.len() as u32,
        quote: middle.text.clone(),
    });
    store
        .correct_memory(
            &user,
            memory.id.clone(),
            1,
            Uuid::new_v4().to_string(),
            middle,
        )
        .await
        .unwrap();
    let mut future = draft("독립 미래 커피");
    future.valid_from = Some(time + 100);
    store
        .correct_memory(
            &user,
            memory.id.clone(),
            2,
            Uuid::new_v4().to_string(),
            future,
        )
        .await
        .unwrap();
    store.delete_source(&user, source, 1).await.unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    for text in ["과거녹차", "중간근거", "미래커피"] {
        assert!(
            store
                .search_knowledge(&user, query(KnowledgeKind::Memory, text))
                .await
                .unwrap()
                .hits
                .is_empty()
        );
    }
    assert!(matches!(
        store.memory(&user, memory.id.clone(), Some(2)).await,
        Err(StoreError::NotFound)
    ));
    let history = store
        .memory_history(&user, memory.id.clone(), None)
        .await
        .unwrap();
    assert_eq!(
        history.items.iter().map(|m| m.revision).collect::<Vec<_>>(),
        vec![3, 1]
    );
    let mut before = query(KnowledgeKind::Memory, "과거녹차");
    before.valid_at = Some(time - 250);
    assert_eq!(
        store.search_knowledge(&user, before).await.unwrap().hits[0].revision,
        1
    );
    let mut after = query(KnowledgeKind::Memory, "미래커피");
    after.valid_at = Some(time + 101);
    assert_eq!(
        store.search_knowledge(&user, after).await.unwrap().hits[0].revision,
        3
    );
    assert_eq!(
        store.memory(&user, memory.id, None).await.unwrap().revision,
        3
    );
}

#[tokio::test]
async fn interrupted_schema_three_upgrade_has_durable_empty_journal_and_can_retry() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let dbpath = dir.path().join("assistant.sqlite");
    let conn = Connection::open(&dbpath).unwrap();
    for migration in [
        include_str!("../../../../migrations/assistant/0001_accounts_threads.sql"),
        include_str!("../../../../migrations/assistant/0002_providers.sql"),
        include_str!("../../../../migrations/assistant/0003_chat.sql"),
    ] {
        conn.execute_batch(migration).unwrap();
    }
    // Fail the migration before its commit, after the startup code must have
    // persisted the prerequisite journal. Dropping the connection models recovery
    // with the same schema3 + prepared empty-journal state as a pre-commit crash.
    conn.execute_batch("CREATE TABLE knowledge_state(interrupted_upgrade_marker INTEGER)")
        .unwrap();
    drop(conn);
    assert!(Store::open(dir.path()).is_err());
    let journal = dir.path().join("knowledge-invalidations.journal");
    assert_eq!(std::fs::metadata(&journal).unwrap().len(), 0);
    let conn = Connection::open(&dbpath).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    conn.execute_batch("DROP TABLE knowledge_state").unwrap();
    drop(conn);
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    new_memory(&store, &user, "교체 후 합성 기억").await;
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.memories(&user, None).await.unwrap().items.len(), 1);
}
