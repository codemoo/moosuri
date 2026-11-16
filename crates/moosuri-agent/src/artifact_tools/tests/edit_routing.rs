//! Named document edits keep one pinned local target through the actual runner.
use super::*;

async fn document(store: &Store, user: &UserContext, title: &str) -> ArtifactReceipt {
    store
        .write_artifact(
            user,
            ArtifactWrite {
                request_id: uuid::Uuid::new_v4().to_string(),
                id: None,
                expected_revision: None,
                draft: ArtifactDraft {
                    title: title.into(),
                    body: "Synthetic original body".into(),
                    payload: ArtifactPayload::Document,
                    references: vec![],
                },
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn named_read_edit_save_updates_same_document_without_composer_or_mission() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let s = server().await;
    s.state.mode.store(10, Ordering::SeqCst);
    let (_dir, store, user, thread) = setup(&s.base).await;
    let original = document(&store, &user, "주간 보고서").await;
    let runtime = Runtime::start(store.clone());
    store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "'주간 보고서' 문서를 읽고 'Synthetic requested addition'을 추가해서 수정하고 저장해 줘.".into(),
        )
        .await
        .unwrap();
    runtime.wake();
    let done = wait(&store, &user, &thread).await;
    assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
    let receipts = done.run.artifact_receipts.unwrap_or_default();
    assert_eq!(
        receipts.len(),
        1,
        "named edit must have a stored write receipt"
    );
    assert_eq!(receipts[0].id, original.id);
    let saved = store.artifact(&user, original.id, None).await.unwrap();
    assert_eq!(saved.artifact.revision, 2);
    assert_eq!(
        saved.draft.unwrap().body,
        "Synthetic original body\nSynthetic requested addition"
    );
    assert_eq!(
        store
            .artifacts(&user, None, None)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(store.missions(&user).await.unwrap().is_empty());
    assert!(done.run.mission_receipt.is_none());
    assert_eq!(s.state.calls.load(Ordering::SeqCst), 3);
    runtime.shutdown().await;
}

#[tokio::test]
async fn composer_pin_and_current_named_target_must_agree_before_any_write() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let s = server().await;
    s.state.mode.store(10, Ordering::SeqCst);
    let (_dir, store, user, thread) = setup(&s.base).await;
    let selected = document(&store, &user, "선택 문서").await;
    let named = document(&store, &user, "지정 문서").await;
    let runtime = Runtime::start(store.clone());
    store
        .submit_turn_with_artifact(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "'지정 문서' 문서를 읽고 'Synthetic requested addition'을 추가해서 수정하고 저장해 줘."
                .into(),
            Some(ArtifactTarget {
                id: selected.id.clone(),
                revision: 1,
            }),
        )
        .await
        .unwrap();
    runtime.wake();
    let done = wait(&store, &user, &thread).await;
    assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
    assert!(done.run.artifact_receipts.is_none());
    for id in [selected.id, named.id] {
        let saved = store.artifact(&user, id, None).await.unwrap();
        assert_eq!(saved.artifact.revision, 1);
        assert_eq!(saved.draft.unwrap().body, "Synthetic original body");
    }
    assert_eq!(s.state.calls.load(Ordering::SeqCst), 1);
    assert!(store.missions(&user).await.unwrap().is_empty());
    runtime.shutdown().await;
}

#[tokio::test]
async fn matching_named_deictic_and_null_quotes_preserve_selected_document_edits() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    for text in [
        "'선택 문서' 문서를 읽고 'Synthetic requested addition'을 추가해서 수정하고 저장해 줘.",
        "그 문서를 읽고 'Synthetic requested addition'을 추가해서 수정하고 저장해 줘.",
        "선택한 문서를 읽고 'Synthetic requested addition'을 추가해서 수정하고 저장해 줘.",
    ] {
        let s = server().await;
        s.state.mode.store(10, Ordering::SeqCst);
        let (_dir, store, user, thread) = setup(&s.base).await;
        let selected = document(&store, &user, "선택 문서").await;
        let runtime = Runtime::start(store.clone());
        store
            .submit_turn_with_artifact(
                &user,
                thread.clone(),
                uuid::Uuid::new_v4().to_string(),
                text.into(),
                Some(ArtifactTarget {
                    id: selected.id.clone(),
                    revision: 1,
                }),
            )
            .await
            .unwrap();
        runtime.wake();
        let done = wait(&store, &user, &thread).await;
        assert_eq!(
            done.run.state,
            RunState::Completed,
            "{text}: {:?}",
            done.run.error
        );
        assert_eq!(done.run.artifact_receipts.unwrap()[0].id, selected.id);
        let saved = store.artifact(&user, selected.id, None).await.unwrap();
        assert_eq!(saved.artifact.revision, 2, "{text}");
        assert_eq!(
            saved.draft.unwrap().body,
            "Synthetic original body\nSynthetic requested addition"
        );
        assert_eq!(
            store
                .artifacts(&user, None, None)
                .await
                .unwrap()
                .items
                .len(),
            1
        );
        assert!(store.missions(&user).await.unwrap().is_empty());
        runtime.shutdown().await;
    }
}

#[tokio::test]
async fn repeated_quoted_deictic_title_cannot_override_the_selected_document() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let s = server().await;
    s.state.mode.store(10, Ordering::SeqCst);
    let (_dir, store, user, thread) = setup(&s.base).await;
    let selected = document(&store, &user, "선택 문서").await;
    let named = document(&store, &user, "그 문서").await;
    let runtime = Runtime::start(store.clone());
    store
        .submit_turn_with_artifact(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "'그 문서' 문서를 읽고 본문에 '그 문서'라는 문장을 추가해서 수정하고 저장해 줘.".into(),
            Some(ArtifactTarget {
                id: selected.id.clone(),
                revision: 1,
            }),
        )
        .await
        .unwrap();
    runtime.wake();
    let done = wait(&store, &user, &thread).await;
    assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
    assert!(done.run.artifact_receipts.is_none());
    for id in [selected.id, named.id] {
        let saved = store.artifact(&user, id, None).await.unwrap();
        assert_eq!(saved.artifact.revision, 1);
        assert_eq!(saved.draft.unwrap().body, "Synthetic original body");
    }
    assert_eq!(s.state.calls.load(Ordering::SeqCst), 1);
    runtime.shutdown().await;
}
