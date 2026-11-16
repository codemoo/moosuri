use super::*;

#[test]
fn relative_imperatives_keep_offsets_literal_and_reject_ambiguous_shells() {
    for (text, seconds) in [
        ("방금 그 알림을 30분 늦춰줘.", 1800),
        ("그 알림은 한 시간 30분만큼 미뤄 주세요", 5400),
        ("그 알림을 두 시간 앞당겨 줘", -7200),
        ("delay that reminder by 30 minutes", 1800),
        ("advance the previous reminder by 1 hour", -3600),
    ] {
        let command = parsed(text, Some("Asia/Seoul"));
        assert!(command.action == parser::Action::Edit, "{text}");
        assert_eq!(command.shift_seconds, Some(seconds), "{text}");
        assert!(command.rule.is_none());
        assert!(matches!(command.target, Some(parser::Target::Previous)));
    }
    let named = parsed("delay Synthetic Dentist reminder by 30 minutes", None);
    assert!(
        matches!(named.target, Some(parser::Target::Title(title)) if title == "Synthetic Dentist")
    );
    for text in [
        "그 알림을 0분 늦춰줘",
        "그 알림을 30분 20분 늦춰줘",
        "그 알림을 1.5시간 늦춰줘",
        "그 알림을 999999999999999일 늦춰줘",
        "그 알림을 367일 늦춰줘",
        "그 알림을 30분 늦춰줘?",
        "그 알림을 30분 늦추지 마",
        "만약 바쁘면 그 알림을 30분 늦춰줘",
        "그 알림을 30분 또는 60분 늦춰줘",
        "번역: 그 알림을 30분 늦춰줘",
        "‘그 알림을 30분 늦춰줘’",
        "delay reminder by 30 minutes or 1 hour",
    ] {
        assert!(
            !matches!(
                parser::parse(text, Some("Asia/Seoul"), ts("2026-05-01T00:00:00Z"), false),
                parser::Decision::Command(_)
            ),
            "{text}"
        );
    }
}

#[tokio::test]
async fn relative_edit_replay_reopen_and_cancel_preserve_target_and_decoy() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let decoy_thread = store
        .create_thread(&user, "Other reminder".into())
        .await
        .unwrap()
        .id;
    send(
        &store,
        &user,
        &decoy_thread,
        "remind me in 2 hours to stretch",
        None,
    )
    .await;
    let decoy = store.schedules(&user).await.unwrap().remove(0);
    let thread = store
        .create_thread(&user, "Appointment".into())
        .await
        .unwrap()
        .id;
    let first = send(
        &store,
        &user,
        &thread,
        "내일 오전 10시에 치과 예약 확인하라고 알려줘.",
        None,
    )
    .await;
    let receipt = &first.run.schedule_receipts.as_ref().unwrap()[0];
    let original = store
        .schedules(&user)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.id == receipt.id)
        .unwrap();
    let request = Uuid::new_v4().to_string();
    let text = "방금 그 알림을 30분 늦춰줘.";
    let submit = || {
        store.submit_turn_with_schedule_context(
            &user,
            thread.clone(),
            request.clone(),
            text.into(),
            None,
            context(None),
        )
    };
    let edited = submit().await.unwrap();
    assert_eq!(edited.run.state, RunState::Completed);
    let changed = &edited.run.schedule_receipts.as_ref().unwrap()[0];
    assert_eq!(changed.id, original.id);
    assert_eq!(changed.revision, 2);
    let replay = submit().await.unwrap();
    assert_eq!(replay.run.id, edited.run.id);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let replay = store
        .submit_turn_with_schedule_context(
            &user,
            thread.clone(),
            request,
            text.into(),
            None,
            context(None),
        )
        .await
        .unwrap();
    assert_eq!(replay.run.id, edited.run.id);
    let current = store.schedules(&user).await.unwrap();
    let target = current.iter().find(|s| s.id == original.id).unwrap();
    assert_eq!(
        target.next_fire_at,
        original.next_fire_at.map(|at| at + 1800)
    );
    assert_eq!(target.revision, 2);
    let cancelled = send(&store, &user, &thread, "방금 그 알림은 취소해 줘.", None).await;
    let cancelled = &cancelled.run.schedule_receipts.as_ref().unwrap()[0];
    assert_eq!(cancelled.id, original.id);
    assert_eq!(cancelled.revision, 3);
    assert_eq!(cancelled.state, "revoked");
    let current = store.schedules(&user).await.unwrap();
    let untouched = current.iter().find(|s| s.id == decoy.id).unwrap();
    assert_eq!(
        serde_json::to_value(untouched).unwrap(),
        serde_json::to_value(decoy).unwrap()
    );
    assert!(store.claim_run().await.unwrap().is_none());
}

#[tokio::test]
async fn relative_edits_do_not_guess_recurring_or_past_times() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    for (creation, edit) in [
        (
            "매일 오전 10시에 물 마시라고 알려줘",
            "그 알림을 30분 늦춰줘",
        ),
        (
            "remind me in 10 minutes to stretch",
            "그 알림을 한 시간 앞당겨줘",
        ),
    ] {
        let thread = store
            .create_thread(&user, "Invalid offset".into())
            .await
            .unwrap()
            .id;
        let created = send(&store, &user, &thread, creation, None).await;
        let receipt = &created.run.schedule_receipts.as_ref().unwrap()[0];
        let before = store.schedules(&user).await.unwrap();
        let turn = send(&store, &user, &thread, edit, None).await;
        assert_eq!(turn.run.state, RunState::Completed);
        assert!(turn.run.schedule_receipts.is_none());
        assert_eq!(
            serde_json::to_value(before).unwrap(),
            serde_json::to_value(store.schedules(&user).await.unwrap()).unwrap()
        );
        // A stale explicit revision must not bypass the ordinary edit fence.
        assert!(matches!(
            store
                .submit_turn_with_schedule_context(
                    &user,
                    thread.clone(),
                    Uuid::new_v4().to_string(),
                    edit.into(),
                    None,
                    context(Some(ScheduleTarget {
                        id: receipt.id.clone(),
                        revision: 0
                    }))
                )
                .await,
            Err(StoreError::Conflict)
        ));
    }
}
