use super::*;

struct Member {
    root: tempfile::TempDir,
    store: Store,
    scope: Scope,
}
async fn member() -> Member {
    let root = crate::tests::directory();
    let path = root.path().canonicalize().unwrap();
    let owner = VolumeOwner {
        account: Uuid::new_v4(),
        tenant: Uuid::new_v4(),
        volume: Uuid::new_v4(),
        generation: 1,
    };
    owner
        .initialize(
            &path,
            "Member".into(),
            "tasks@example.test".into(),
            "Synthetic-first-password".into(),
        )
        .await
        .unwrap();
    let store = owner.open_verified(&path).await.unwrap();
    let first = store
        .login(
            "tasks@example.test".into(),
            "Synthetic-first-password".into(),
        )
        .await
        .unwrap();
    let user = store
        .change_password(&first.user, "Synthetic-second-password".into())
        .await
        .unwrap()
        .user;
    let thread = store
        .create_thread(&user, "Schedule checks".into())
        .await
        .unwrap()
        .id;
    Member {
        root,
        store,
        scope: Scope {
            owner,
            user,
            thread,
        },
    }
}
async fn start(store: &Store, scope: &Scope, title: &str) -> (Uuid, Workspace) {
    let request = Uuid::new_v4();
    let Admission::Fresh(job) = store
        .personal_browser_open(
            scope.clone(),
            request,
            "https://example.org/".into(),
            title.into(),
            false,
        )
        .await
        .unwrap()
    else {
        panic!("new task")
    };
    (request, job.view)
}

#[tokio::test]
async fn personal_tasks_page_cursor_survives_newer_jobs_and_preserves_original_ids() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let mut ids = vec![];
    for index in 0..=PAGE_SIZE {
        let (request, mut view) = start(&store, &scope, &format!("Check schedule {index}")).await;
        ids.push(view.run_id.clone());
        view.state = "closed".into();
        store
            .personal_browser_finish(scope.clone(), view, request, None)
            .await
            .unwrap();
    }
    let page = store
        .personal_task_page(scope.owner, scope.user.clone(), None)
        .await
        .unwrap();
    assert_eq!(page.items.len(), PAGE_SIZE);
    assert_eq!(page.items.first().unwrap().id, *ids.last().unwrap());
    start(&store, &scope, "Newly scheduled page").await;
    let older = store
        .personal_task_page(
            scope.owner,
            scope.user.clone(),
            Some(page.next_before.unwrap().parse().unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(older.items.len(), 1);
    assert_eq!(older.items[0].id, ids[0]);
    assert!(older.next_before.is_none());
}

#[tokio::test]
async fn personal_tasks_reopen_completed_result_and_recover_interrupted_job_with_real_history() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member { root, store, scope } = member().await;
    let (request, mut first) = start(&store, &scope, "First schedule").await;
    let first_id = first.run_id.parse().unwrap();
    let job = store
        .personal_browser_publish(scope.clone(), request, first.clone())
        .await
        .unwrap();
    first = job.view;
    first.state = "ready".into();
    first = store
        .personal_browser_finish(
            scope.clone(),
            first,
            request,
            Some(Output::Observation("First page".into())),
        )
        .await
        .unwrap();
    let first_read = store
        .personal_task_detail(scope.owner, scope.user.clone(), first_id)
        .await
        .unwrap();
    assert_eq!(first_read.task.next_actor, "member");
    assert_eq!(first_read.observation.as_deref(), Some("First page"));
    assert!(first_read.result.is_none());
    let follow = Uuid::new_v4();
    let Admission::Fresh(continued) = store
        .personal_browser_reserve(
            scope.clone(),
            Version {
                workspace_id: first.id,
                revision: first.revision,
                request_id: follow,
            },
            "follow",
            None,
        )
        .await
        .unwrap()
    else {
        panic!("new follow")
    };
    let mut view = continued.view;
    view.state = "completed".into();
    store
        .personal_browser_finish(
            scope.clone(),
            view,
            follow,
            Some(Output::Observation("Schedule confirmed".into())),
        )
        .await
        .unwrap();
    let (_, next) = start(&store, &scope, "Check another timetable").await;
    let second_id = next.run_id.parse().unwrap();
    drop(store);
    let reopened = scope
        .owner
        .open_verified(&root.path().canonicalize().unwrap())
        .await
        .unwrap();
    let settled = reopened
        .personal_task_detail(scope.owner, scope.user.clone(), first_id)
        .await
        .unwrap();
    assert_eq!(settled.task.state, "completed");
    assert_eq!(settled.result.as_deref(), Some("Schedule confirmed"));
    assert!(settled.observation.is_none());
    assert_eq!(settled.task.thread_id, scope.thread);
    assert_eq!(
        settled
            .history
            .iter()
            .map(|e| e.state.as_str())
            .collect::<Vec<_>>(),
        [
            "starting",
            "reading",
            "ready",
            "resuming",
            "completed",
            "closed"
        ]
    );
    let interrupted = reopened
        .personal_task_detail(scope.owner, scope.user, second_id)
        .await
        .unwrap();
    assert_eq!(interrupted.task.state, "outcome_unknown");
    assert_eq!(interrupted.task.next_actor, "member");
    assert!(interrupted.result.is_none());
    assert_eq!(interrupted.history.last().unwrap().state, "outcome_unknown");
}

#[tokio::test]
async fn personal_tasks_schema_89_preserves_existing_result_and_marks_unknown_event_time() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member { root, store, scope } = member().await;
    let (request, mut view) = start(&store, &scope, "Historical check").await;
    let id = view.run_id.parse().unwrap();
    view.state = "completed".into();
    store
        .personal_browser_finish(
            scope.clone(),
            view,
            request,
            Some(Output::Observation("Retained answer".into())),
        )
        .await
        .unwrap();
    let owner = scope.owner;
    store
        .call(move |db| {
            let identity: String = db.conn.query_row(
                "SELECT sql FROM sqlite_schema WHERE name='personal_runtime_identity'",
                [],
                |r| r.get(0),
            )?;
            crate::schema::tests::replace_with_predecessor(&mut db.conn, 89);
            db.conn.execute_batch(&identity)?;
            db.conn.execute(
                "INSERT INTO personal_runtime_identity VALUES(1,?1,?2)",
                params![owner.volume.to_string(), owner.generation as i64],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let reopened = scope
        .owner
        .open_verified(&root.path().canonicalize().unwrap())
        .await
        .unwrap();
    let detail = reopened
        .personal_task_detail(scope.owner, scope.user, id)
        .await
        .unwrap();
    assert_eq!(detail.result.as_deref(), Some("Retained answer"));
    assert_eq!(detail.history.len(), 2);
    assert_eq!(detail.history[0].at, Some(detail.task.created_at));
    assert_eq!(detail.history[1].state, "completed");
    assert_eq!(detail.history[1].at, None);
}
