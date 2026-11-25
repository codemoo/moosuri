use super::*;

pub(in crate::personal_browser) struct Member {
    pub(in crate::personal_browser) root: tempfile::TempDir,
    pub(in crate::personal_browser) store: Store,
    pub(in crate::personal_browser) scope: Scope,
}
pub(in crate::personal_browser) async fn member() -> Member {
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

pub(in crate::personal_browser) async fn reopen_predecessor(
    member: Member,
    version: i64,
) -> Member {
    let Member { store, root, scope } = member;
    let owner = scope.owner;
    store
        .call(move |db| {
            let identity: String = db.conn.query_row(
                "SELECT sql FROM sqlite_schema WHERE name='personal_runtime_identity'",
                [],
                |r| r.get(0),
            )?;
            crate::schema::tests::replace_with_predecessor(&mut db.conn, version);
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
    Member {
        store: reopened,
        scope,
        root,
    }
}

#[tokio::test]
async fn personal_tasks_releases_expired_direct_control_once_without_reopening_chat() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let (request, mut view) = start(&store, &scope, "Review the timetable").await;
    let id = Uuid::parse_str(&view.run_id).unwrap();
    view.state = "ready".into();
    let view = store
        .personal_browser_finish(scope.clone(), view, request, None)
        .await
        .unwrap();
    store
        .personal_browser_reserve(
            scope.clone(),
            Version {
                workspace_id: view.id,
                revision: view.revision,
                request_id: Uuid::new_v4(),
            },
            "takeover",
            Some("fixture-control".into()),
        )
        .await
        .unwrap();
    let detail = store
        .personal_task_detail(scope.owner, scope.user.clone(), id)
        .await
        .unwrap();
    assert_eq!(detail.task.state, "user_control");
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE personal_browser_jobs SET controller_until=unixepoch()-1 WHERE run_id=?1",
                [id.to_string()],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let page = store
        .personal_task_page(scope.owner, scope.user.clone(), None)
        .await
        .unwrap();
    assert_eq!(page.items[0].state, "ready");
    let after = store
        .personal_task_detail(scope.owner, scope.user, id)
        .await
        .unwrap();
    assert_eq!(after.reason.as_deref(), Some("controller_expired"));
    assert_eq!(after.history.len(), detail.history.len() + 1);
    assert_eq!(after.history.last().unwrap().state, "ready");
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
    assert!(
        first_read
            .history
            .last()
            .unwrap()
            .detail
            .as_ref()
            .unwrap()
            .contains("First page")
    );
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
    assert!(
        settled
            .history
            .iter()
            .find(|e| e.state == "ready")
            .unwrap()
            .detail
            .as_ref()
            .unwrap()
            .contains("갱신")
    );
    assert!(
        settled
            .history
            .iter()
            .find(|e| e.state == "completed")
            .unwrap()
            .detail
            .as_ref()
            .unwrap()
            .contains("Schedule confirmed")
    );
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
    let Member {
        store: reopened,
        scope,
        root: _root,
    } = reopen_predecessor(Member { root, store, scope }, 89).await;
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

fn research(id: Uuid, period: i64) -> moosuri_core::personal_browser::WorkRequest {
    use moosuri_core::personal_browser::{WorkRequest, WorkScope};
    WorkRequest {
        request_id: id.to_string(),
        title: "자료 조사".into(),
        scope: WorkScope::Memory,
        query: "수첩".into(),
        period,
        first_due: None,
        thread_id: None,
        message: None,
    }
}

#[tokio::test]
async fn workflow_research_reads_sources_replays_one_chat_turn_and_withdraws_forgotten_text() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        root: _root,
        store,
        scope,
    } = member().await;
    let user = &scope.user;
    let memory = store
        .create_memory(
            user,
            Uuid::new_v4().to_string(),
            moosuri_core::MemoryDraft {
                subject: "수첩 색상".into(),
                text: "합성 파란 표지".into(),
                speaker: None,
                evidence: vec![],
                valid_from: None,
                valid_until: None,
            },
        )
        .await
        .unwrap();
    let key = Uuid::new_v4();
    let request = || {
        let mut q = research(key, 0);
        q.thread_id = Some(scope.thread.clone());
        q.message = Some("기억에서 수첩 찾아줘".into());
        q
    };
    let first = store
        .create_member_workflow(scope.owner, user.clone(), 1, request())
        .await
        .unwrap();
    let second = store
        .create_member_workflow(scope.owner, user.clone(), 1, request())
        .await
        .unwrap();
    assert_eq!(first.run_id, second.run_id);
    let id = first.run_id.unwrap().parse().unwrap();
    let detail = store
        .personal_task_detail(scope.owner, user.clone(), id)
        .await
        .unwrap();
    assert_eq!(detail.task.thread_id, scope.thread);
    assert_eq!(detail.request, "기억에서 수첩 찾아줘");
    assert_eq!(
        detail
            .history
            .iter()
            .map(|h| h.state.as_str())
            .collect::<Vec<_>>(),
        ["starting", "searched", "source_read", "completed"]
    );
    assert_eq!(detail.history[2].detail.as_deref(), Some("합성 파란 표지"));
    assert_eq!(
        detail.task.preview.as_deref(),
        Some("기억 1건 확인 · 합성 파란 표지")
    );
    assert!(detail.task.scheduled_at.is_none());
    assert!(detail.history.windows(2).all(|w| w[0].at <= w[1].at));
    let mut changed = request();
    changed.query = "바뀜".into();
    assert!(matches!(
        store
            .create_member_workflow(scope.owner, user.clone(), 1, changed)
            .await,
        Err(StoreError::Conflict)
    ));
    let page = store
        .personal_task_page(scope.owner, user.clone(), None)
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].preview, detail.task.preview);
    store
        .forget_memory(user, memory.id, memory.revision)
        .await
        .unwrap();
    let after = store
        .personal_task_detail(scope.owner, user.clone(), id)
        .await
        .unwrap();
    assert!(
        !serde_json::to_string(&after)
            .unwrap()
            .contains("합성 파란 표지")
    );
    assert!(
        after.history[2]
            .detail
            .as_ref()
            .unwrap()
            .contains("더 이상 볼 수 없습니다")
    );
    let page = store
        .personal_task_page(scope.owner, user.clone(), None)
        .await
        .unwrap();
    assert_eq!(page.items[0].preview.as_deref(), Some("기억 1건 확인"));
    assert_eq!(after.task.preview, page.items[0].preview);
}

#[tokio::test]
async fn workflow_preview_uses_the_next_visible_source_after_one_is_forgotten() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let user = &scope.user;
    let mut memories = Vec::new();
    for color in ["초록", "파랑"] {
        let memory = store
            .create_memory(
                user,
                Uuid::new_v4().to_string(),
                moosuri_core::MemoryDraft {
                    subject: format!("{color} 수첩"),
                    text: format!("{color} 표지"),
                    speaker: None,
                    evidence: vec![],
                    valid_from: None,
                    valid_until: None,
                },
            )
            .await
            .unwrap();
        memories.push((color, memory));
    }
    let run = store
        .create_member_workflow(scope.owner, user.clone(), 1, research(Uuid::new_v4(), 0))
        .await
        .unwrap()
        .run_id
        .unwrap()
        .parse()
        .unwrap();
    let before = store
        .personal_task_detail(scope.owner, user.clone(), run)
        .await
        .unwrap();
    let first = before.task.preview.unwrap();
    let position = memories
        .iter()
        .position(|(color, _)| first.contains(*color))
        .unwrap();
    let (_, forgotten) = memories.remove(position);
    store
        .forget_memory(user, forgotten.id, forgotten.revision)
        .await
        .unwrap();
    let expected = format!("기억 2건 확인 · {} 표지", memories[0].0);
    let page = store
        .personal_task_page(scope.owner, user.clone(), None)
        .await
        .unwrap();
    let detail = store
        .personal_task_detail(scope.owner, user.clone(), run)
        .await
        .unwrap();
    assert_eq!(page.items[0].preview.as_deref(), Some(expected.as_str()));
    assert_eq!(page.items[0].preview, detail.task.preview);
}

async fn due(store: &Store, id: Uuid) {
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE member_workflows SET next_due=unixepoch()-7201 WHERE id=?1",
                [id.to_string()],
            )?;
            Ok(())
        })
        .await
        .unwrap();
}
#[tokio::test]
async fn workflow_due_coalesces_once_across_concurrent_ticks_and_reopen() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member { root, store, scope } = member().await;
    let key = Uuid::new_v4();
    store
        .create_member_workflow(scope.owner, scope.user.clone(), 2, research(key, 1800))
        .await
        .unwrap();
    due(&store, key).await;
    let (one, two) = tokio::join!(
        store.tick_member_workflows(scope.owner, 2),
        store.tick_member_workflows(scope.owner, 2)
    );
    assert_eq!(one.unwrap() + two.unwrap(), 1);
    let first = store
        .personal_task_page(scope.owner, scope.user.clone(), None)
        .await
        .unwrap();
    assert_eq!(first.items.len(), 1);
    assert!(first.items[0].scheduled_at.is_some());
    assert_eq!(first.items[0].preview.as_deref(), Some("기억 0건 확인"));
    assert!(first.schedules[0].next_due.unwrap() > now().unwrap());
    drop(store);
    let store = scope
        .owner
        .open_verified(&root.path().canonicalize().unwrap())
        .await
        .unwrap();
    assert_eq!(
        store.tick_member_workflows(scope.owner, 2).await.unwrap(),
        0
    );
    let saved = store
        .personal_task_detail(scope.owner, scope.user, first.items[0].id.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(saved.history[0].title.as_deref(), Some("예약 실행 시작"));
    assert_eq!(saved.request, "내 기억에서 ‘수첩’ 검색");
    assert!(saved.result.unwrap().contains("0건"));
}

#[tokio::test]
async fn schedule_history_pages_only_its_own_runs_after_retirement() {
    use moosuri_core::personal_browser::WorkAction;
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        root: _root,
        store,
        scope,
    } = member().await;
    let owner = scope.owner;
    let user = &scope.user;
    let schedule = Uuid::new_v4();
    let other = Uuid::new_v4();
    for id in [schedule, other] {
        store
            .create_member_workflow(owner, user.clone(), 1, research(id, 1800))
            .await
            .unwrap();
    }
    let mut expected = Vec::new();
    for index in 0..=PAGE_SIZE + 1 {
        let assignment = if index == PAGE_SIZE { other } else { schedule };
        let receipt = store
            .change_member_workflow(
                owner,
                user.clone(),
                1,
                assignment,
                WorkAction::Run {
                    request_id: Uuid::new_v4().to_string(),
                },
            )
            .await
            .unwrap();
        if assignment == schedule {
            expected.push(receipt.run_id.unwrap());
        }
    }
    store
        .change_member_workflow(
            owner,
            user.clone(),
            1,
            schedule,
            WorkAction::Remove { revision: 1 },
        )
        .await
        .unwrap();
    let first = store
        .member_task_history_page(owner, user.clone(), Some(schedule), None)
        .await
        .unwrap();
    assert_eq!(first.items.len(), PAGE_SIZE);
    assert!(first.schedules.is_empty());
    let cursor = first.next_before.expect("an older run");
    let last = store
        .member_task_history_page(owner, user.clone(), Some(schedule), Some(cursor))
        .await
        .unwrap();
    assert!(last.next_before.is_none());
    expected.reverse();
    let id = schedule.to_string();
    let actual: Vec<_> = first
        .items
        .into_iter()
        .chain(last.items)
        .map(|row| {
            assert_eq!(row.schedule_id.as_deref(), Some(id.as_str()));
            row.id
        })
        .collect();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn workflow_schedule_pause_resume_manual_retry_and_retirement_keep_prior_receipts() {
    use moosuri_core::personal_browser::WorkAction;
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        root: _root,
        store,
        scope,
    } = member().await;
    let key = Uuid::new_v4();
    let o = scope.owner;
    let u = &scope.user;
    store
        .create_member_workflow(o, u.clone(), 1, research(key, 3600))
        .await
        .unwrap();
    store
        .change_member_workflow(o, u.clone(), 1, key, WorkAction::Pause { revision: 1 })
        .await
        .unwrap();
    assert!(matches!(
        store
            .change_member_workflow(o, u.clone(), 1, key, WorkAction::Resume { revision: 1 })
            .await,
        Err(StoreError::Conflict)
    ));
    due(&store, key).await;
    assert_eq!(store.tick_member_workflows(o, 1).await.unwrap(), 0);
    let request_id = Uuid::new_v4().to_string();
    let run = || WorkAction::Run {
        request_id: request_id.clone(),
    };
    let receipt = store
        .change_member_workflow(o, u.clone(), 1, key, run())
        .await
        .unwrap();
    assert_eq!(
        receipt.run_id,
        store
            .change_member_workflow(o, u.clone(), 1, key, run())
            .await
            .unwrap()
            .run_id
    );
    store
        .change_member_workflow(o, u.clone(), 1, key, WorkAction::Resume { revision: 2 })
        .await
        .unwrap();
    due(&store, key).await;
    assert_eq!(store.tick_member_workflows(o, 1).await.unwrap(), 1);
    store
        .change_member_workflow(o, u.clone(), 1, key, WorkAction::Remove { revision: 3 })
        .await
        .unwrap();
    let page = store.personal_task_page(o, u.clone(), None).await.unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(
        page.items
            .iter()
            .filter(|r| r.scheduled_at.is_some())
            .count(),
        1
    );
    assert!(
        page.items
            .iter()
            .any(|r| Some(&r.id) == receipt.run_id.as_ref() && r.scheduled_at.is_none())
    );
    assert!(page.schedules.is_empty());
    assert!(matches!(
        store
            .change_member_workflow(o, u.clone(), 1, key, run())
            .await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn workflow_registry_epoch_generation_and_restored_grants_fence_due_research() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        root: _root,
        store,
        scope,
    } = member().await;
    let key = Uuid::new_v4();
    let o = scope.owner;
    let u = &scope.user;
    store
        .create_member_workflow(o, u.clone(), 3, research(key, 1800))
        .await
        .unwrap();
    due(&store, key).await;
    let mut other = o;
    other.generation += 1;
    assert!(store.tick_member_workflows(other, 3).await.is_err());
    assert_eq!(store.tick_member_workflows(o, 4).await.unwrap(), 0);
    let page = store.personal_task_page(o, u.clone(), None).await.unwrap();
    assert!(!page.schedules[0].enabled);
    assert!(page.items.is_empty());
    store
        .change_member_workflow(
            o,
            u.clone(),
            4,
            key,
            moosuri_core::personal_browser::WorkAction::Resume { revision: 2 },
        )
        .await
        .unwrap();
    assert_eq!(store.tick_member_workflows(o, 3).await.unwrap(), 0);
    let reenabled = store.personal_task_page(o, u.clone(), None).await.unwrap();
    assert!(reenabled.schedules[0].enabled);
    store
        .call(|db| crate::schedules::quarantine_for_restore(&db.conn))
        .await
        .unwrap();
    due(&store, key).await;
    assert_eq!(store.tick_member_workflows(o, 4).await.unwrap(), 0);
}

#[tokio::test]
async fn schema_92_member_reopens_with_its_original_thread_and_empty_new_grants() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member { store, scope, root } = member().await;
    drop(store);
    let root = root.path().canonicalize().unwrap();
    let directory = scope.owner.verified_member_directory(&root).unwrap();
    let mut db = rusqlite::Connection::open(directory.join("assistant.sqlite")).unwrap();
    // Volume identity is provisioned by the installer outside the shared assistant migrations.
    let identity: String = db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='personal_runtime_identity'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let pin: (String, i64) = db
        .query_row(
            "SELECT volume,generation FROM personal_runtime_identity WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    crate::schema::tests::replace_with_predecessor(&mut db, 92);
    db.execute_batch(&identity).unwrap();
    db.execute(
        "INSERT INTO personal_runtime_identity(singleton,volume,generation) VALUES(1,?1,?2)",
        params![pin.0, pin.1],
    )
    .unwrap();
    drop(db);
    let store = scope.owner.open_verified(&root).await.unwrap();
    let threads = store.list_threads(&scope.user).await.unwrap();
    assert!(threads.iter().any(|t| t.id == scope.thread));
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
                crate::schema::CURRENT_VERSION
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                        .get::<_, i64>(
                        0
                    ))?,
                0
            );
            for table in [
                "member_connected_briefs",
                "member_public_watches",
                "member_mail_work",
                "member_calendar_actions",
            ] {
                assert_eq!(
                    db.conn
                        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                            .get::<_, i64>(0))?,
                    0
                );
            }
            Ok(())
        })
        .await
        .unwrap();
}
