use super::*;
use crate::personal_browser::tasks::tests::{Member, member};
use chrono::{TimeZone, Utc};

macro_rules! remember {
    ($store:expr, $user:expr, $subject:expr, $text:expr) => {
        $store
            .create_memory(
                $user,
                Uuid::new_v4().to_string(),
                moosuri_core::MemoryDraft {
                    subject: $subject.into(),
                    text: $text.into(),
                    speaker: None,
                    evidence: vec![],
                    valid_from: None,
                    valid_until: None,
                },
            )
            .await
            .unwrap()
    };
}

fn watch(id: Uuid) -> MonitorRequest {
    MonitorRequest {
        request_id: id.to_string(),
        thread_id: None,
        message: None,
        kind: MonitorKind::Watch,
        title: "서비스 상태 감시".into(),
        scope: WorkScope::Memory,
        query: "수첩".into(),
        every_seconds: Some(1800),
        at: None,
        timezone: None,
        days: 7,
    }
}
fn brief(id: Uuid, at: &str, zone: &str) -> MonitorRequest {
    MonitorRequest {
        request_id: id.to_string(),
        thread_id: None,
        message: None,
        kind: MonitorKind::Brief,
        title: "아침 브리핑".into(),
        scope: WorkScope::Memory,
        query: "수첩".into(),
        every_seconds: None,
        at: Some(at.into()),
        timezone: Some(zone.into()),
        days: 7,
    }
}
async fn create(store: &Store, scope: &Scope, request: MonitorRequest) -> Result<MonitorView> {
    store
        .create_member_monitor(scope.owner, scope.user.clone(), 1, request)
        .await
}
async fn status(store: &Store, scope: &Scope, id: &str) -> MonitorView {
    store
        .member_monitor(scope.owner, scope.user.clone(), id.parse().unwrap())
        .await
        .unwrap()
}
/// Runs the timer's monitor step at an explicit clock, exactly as the member tick does.
async fn sweep(store: &Store, scope: &Scope, time: i64, epoch: i64) -> usize {
    let owner = scope.owner;
    store
        .call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let done = dispatch(&tx, owner, epoch, time)?;
            tx.commit()?;
            Ok(done)
        })
        .await
        .unwrap()
}
async fn exec(store: &Store, sql: String) {
    store
        .call(move |db| {
            db.conn.execute(&sql, [])?;
            Ok(())
        })
        .await
        .unwrap();
}
async fn count(store: &Store, sql: impl Into<String>) -> i64 {
    let sql = sql.into();
    store
        .call(move |db| Ok(db.conn.query_row(&sql, [], |r| r.get(0))?))
        .await
        .unwrap()
}
async fn text(store: &Store, sql: impl Into<String>) -> String {
    let sql = sql.into();
    store
        .call(move |db| Ok(db.conn.query_row(&sql, [], |r| r.get(0))?))
        .await
        .unwrap()
}
const NOTICES: &str = "SELECT count(*) FROM local_notifications";
const LIVE_PUSH: &str = "SELECT count(*) FROM local_notifications WHERE push_suppressed=0";
const RUNS: &str = "SELECT count(*) FROM member_work_runs";

#[test]
fn daily_slots_follow_the_local_clock_across_dst_and_offsets() {
    use chrono_tz::America::New_York;
    let (zone, name) = parse_zone("America/New_York").unwrap();
    assert_eq!(name, "America/New_York");
    let at = |y, mo, d, h, mi| {
        New_York
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .earliest()
            .unwrap()
            .timestamp()
    };
    // 02:30 does not exist on 2026-03-08: the slot is skipped, never invented in UTC.
    let clock = parse_clock("02:30").unwrap();
    assert_eq!(
        slot_after(&zone, clock, at(2026, 3, 7, 12, 0)).unwrap(),
        at(2026, 3, 9, 2, 30)
    );
    // 01:30 occurs twice on 2026-11-01: the first occurrence is the slot, once.
    let clock = parse_clock("01:30").unwrap();
    let fold = at(2026, 11, 1, 1, 30);
    assert_eq!(
        slot_after(&zone, clock, at(2026, 10, 31, 12, 0)).unwrap(),
        fold
    );
    assert_eq!(
        slot_after(&zone, clock, fold).unwrap(),
        New_York
            .with_ymd_and_hms(2026, 11, 2, 1, 30, 0)
            .unwrap()
            .timestamp()
    );
    // The local date, not the UTC date, decides the day: 23:30Z is already tomorrow in +09:00.
    let (seoul, canonical) = parse_zone("UTC+9").unwrap();
    assert_eq!(canonical, "+09:00");
    let late = Utc
        .with_ymd_and_hms(2026, 6, 1, 23, 30, 0)
        .unwrap()
        .timestamp();
    assert_eq!(
        slot_after(&seoul, parse_clock("09:00").unwrap(), late).unwrap(),
        Utc.with_ymd_and_hms(2026, 6, 2, 0, 0, 0)
            .unwrap()
            .timestamp()
    );
    assert_eq!(parse_zone("GMT-5").unwrap().1, "-05:00");
    assert_eq!(parse_zone("Asia/Seoul").unwrap().1, "Asia/Seoul");
    for bad in [
        "",
        "Mars/Base",
        "+15:00",
        "-13:00",
        "+09:60",
        "UTC+",
        "+9:5x",
    ] {
        assert!(parse_zone(bad).is_err(), "{bad}");
    }
    for bad in ["9:00", "24:00", "09:5", "09:00 ", "0900"] {
        assert!(parse_clock(bad).is_err(), "{bad}");
    }
}

#[tokio::test]
async fn create_validates_replays_conflicts_and_stays_out_of_fixed_schedules() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let id = Uuid::new_v4();
    let first = create(&store, &scope, watch(id)).await.unwrap();
    let again = create(&store, &scope, watch(id)).await.unwrap();
    assert_eq!(first, again);
    assert_eq!(first.state, "active");
    assert_eq!(first.alerts, 0);
    assert_eq!(first.recent.len(), 1);
    assert_eq!(first.recent[0].outcome, "baseline");
    assert!(first.basis.contains("메일, 캘린더는 읽지 않습니다"));
    // Replays never add rows; a different body under the same id is a conflict.
    assert_eq!(
        count(&store, "SELECT count(*) FROM member_work_monitors").await,
        1
    );
    assert_eq!(count(&store, RUNS).await, 1);
    let mut other_query = watch(id);
    other_query.query = "다른 검색어".into();
    assert!(matches!(
        create(&store, &scope, other_query).await,
        Err(StoreError::Conflict)
    ));
    let mut other_days = watch(id);
    other_days.days = 30;
    assert!(matches!(
        create(&store, &scope, other_days).await,
        Err(StoreError::Conflict)
    ));
    // Fixed schedules are unaffected: the monitor is neither listed nor run by the fixed-period timer.
    let page = store
        .personal_task_page(scope.owner, scope.user.clone(), None)
        .await
        .unwrap();
    assert!(page.schedules.is_empty());
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, first.latest.as_ref().unwrap().run_id);
    // Invalid shapes are rejected before anything is written.
    let mut bad = Vec::new();
    for edit in [
        (|r: &mut MonitorRequest| r.every_seconds = Some(60)) as fn(&mut MonitorRequest),
        |r| r.every_seconds = None,
        |r| r.at = Some("09:00".into()),
        |r| r.timezone = Some("UTC".into()),
        |r| r.days = 0,
        |r| r.days = 91,
        |r| r.query = "  ".into(),
        |r| r.request_id = "not-a-uuid".into(),
    ] {
        let mut r = watch(Uuid::new_v4());
        edit(&mut r);
        bad.push(r);
    }
    for edit in [
        (|r: &mut MonitorRequest| r.at = Some("9:00".into())) as fn(&mut MonitorRequest),
        |r| r.timezone = None,
        |r| r.timezone = Some("Mars/Base".into()),
        |r| r.every_seconds = Some(1800),
    ] {
        let mut r = brief(Uuid::new_v4(), "09:00", "Asia/Seoul");
        edit(&mut r);
        bad.push(r);
    }
    for r in bad {
        assert!(matches!(
            create(&store, &scope, r).await,
            Err(StoreError::InvalidInput)
        ));
    }
    assert_eq!(
        count(&store, "SELECT count(*) FROM member_work_monitors").await,
        1
    );
    // Bounded: eight active monitors per member.
    for _ in 0..7 {
        create(&store, &scope, watch(Uuid::new_v4())).await.unwrap();
    }
    assert!(matches!(
        create(&store, &scope, watch(Uuid::new_v4())).await,
        Err(StoreError::BudgetExhausted)
    ));
    let listed = store
        .member_monitors(scope.owner, scope.user.clone())
        .await
        .unwrap();
    assert_eq!((listed.items.len(), listed.active_limit), (8, 8));
}

#[tokio::test]
async fn watch_notifies_only_for_real_changes_and_never_carries_source_text() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let user = &scope.user;
    remember!(store, user, "초록 수첩", "비밀 표지 문구");
    let id = Uuid::new_v4();
    let created = create(&store, &scope, watch(id)).await.unwrap();
    let id = id.to_string();
    let due = created.next_due.unwrap();
    let evidence = created.latest.unwrap();
    assert_eq!(evidence.sources.len(), 1);
    assert!(evidence.sources[0].available);
    assert_eq!(evidence.sources[0].title.as_deref(), Some("초록 수첩"));
    // Unchanged evidence: the check is recorded, but there is no run and no alert.
    assert_eq!(sweep(&store, &scope, due + 5, 1).await, 1);
    assert_eq!(count(&store, NOTICES).await, 0);
    assert_eq!(count(&store, RUNS).await, 1);
    let seen = status(&store, &scope, &id).await;
    assert_eq!(seen.recent[0].outcome, "unchanged");
    assert_eq!(seen.next_due, Some(due + 1800));
    // A repeated tick at the same time finds nothing due.
    assert_eq!(sweep(&store, &scope, due + 5, 1).await, 0);
    // A genuinely new matching record produces exactly one alert and one run with references.
    let second = remember!(store, user, "파랑 수첩", "또 다른 비밀 문구");
    assert_eq!(sweep(&store, &scope, due + 1805, 1).await, 1);
    assert_eq!(count(&store, NOTICES).await, 1);
    assert_eq!(count(&store, RUNS).await, 2);
    assert_eq!(count(&store, LIVE_PUSH).await, 1);
    let message = text(
        &store,
        "SELECT title||'|'||message FROM local_notifications",
    )
    .await;
    for secret in ["초록", "파랑", "비밀", "문구", "표지"] {
        assert!(!message.contains(secret), "{message}");
    }
    let changed = status(&store, &scope, &id).await;
    assert_eq!(
        (changed.alerts, changed.recent[0].outcome.as_str()),
        (1, "changed")
    );
    assert!(changed.recent[0].notified && changed.last_changed_at.is_some());
    assert_eq!(changed.latest.as_ref().unwrap().sources.len(), 2);
    // The same evidence on later checks stays quiet.
    assert_eq!(sweep(&store, &scope, due + 3605, 1).await, 1);
    assert_eq!(sweep(&store, &scope, due + 5405, 1).await, 1);
    assert_eq!(count(&store, NOTICES).await, 1);
    assert_eq!(count(&store, RUNS).await, 2);
    // Correcting a record creates a new revision, which is a change of evidence.
    store
        .correct_memory(
            user,
            second.id.clone(),
            second.revision,
            Uuid::new_v4().to_string(),
            moosuri_core::MemoryDraft {
                subject: "파랑 수첩".into(),
                text: "고친 문구".into(),
                speaker: None,
                evidence: vec![],
                valid_from: None,
                valid_until: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(sweep(&store, &scope, due + 7205, 1).await, 1);
    assert_eq!(count(&store, NOTICES).await, 2);
}

#[tokio::test]
async fn missing_or_withdrawn_sources_return_no_text_and_still_alert_once() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let user = &scope.user;
    let a = remember!(store, user, "초록 수첩", "초록 비밀");
    let b = remember!(store, user, "파랑 수첩", "파랑 비밀");
    let id = Uuid::new_v4();
    let created = create(&store, &scope, watch(id)).await.unwrap();
    let id = id.to_string();
    let due = created.next_due.unwrap();
    // Withdraw one source after the baseline: the reference stays but its title is gone.
    store.forget_memory(user, a.id, a.revision).await.unwrap();
    let view = status(&store, &scope, &id).await;
    let sources = &view.latest.as_ref().unwrap().sources;
    assert_eq!(sources.len(), 2);
    assert_eq!(sources.iter().filter(|s| s.available).count(), 1);
    assert!(
        sources
            .iter()
            .filter(|s| !s.available)
            .all(|s| s.title.is_none())
    );
    let json = serde_json::to_string(&view).unwrap();
    assert!(!json.contains("초록") && !json.contains("비밀"));
    // The worklog of the baseline run shows the placeholder, not the withdrawn text.
    let run = view.latest.as_ref().unwrap().run_id.parse().unwrap();
    let detail = store
        .personal_task_detail(scope.owner, user.clone(), run)
        .await
        .unwrap();
    let detail = serde_json::to_string(&detail).unwrap();
    assert!(detail.contains("더 이상 볼 수 없습니다") && !detail.contains("초록"));
    // The withdrawal is a change of evidence: one generic alert.
    assert_eq!(sweep(&store, &scope, due + 5, 1).await, 1);
    assert_eq!(count(&store, NOTICES).await, 1);
    // Everything gone: one more alert, an empty reference list, and no text anywhere.
    store.forget_memory(user, b.id, b.revision).await.unwrap();
    assert_eq!(sweep(&store, &scope, due + 1805, 1).await, 1);
    assert_eq!(count(&store, NOTICES).await, 2);
    let empty = status(&store, &scope, &id).await;
    assert!(empty.latest.as_ref().unwrap().sources.is_empty());
    assert_eq!(empty.recent[0].source_count, 0);
    assert_eq!(sweep(&store, &scope, due + 3605, 1).await, 1);
    assert_eq!(count(&store, NOTICES).await, 2);
    let all = text(
        &store,
        "SELECT group_concat(title||message,'|') FROM local_notifications",
    )
    .await;
    assert!(!all.contains("초록") && !all.contains("파랑") && !all.contains("비밀"));
}

#[tokio::test]
async fn brief_runs_at_the_local_time_and_folds_or_misses_closed_periods() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let user = &scope.user;
    remember!(store, user, "초록 수첩", "오늘 할 일 메모");
    let id = Uuid::new_v4();
    let created = create(&store, &scope, brief(id, "09:00", "+09:00"))
        .await
        .unwrap();
    let id = id.to_string();
    let slot = created.next_due.unwrap();
    assert_eq!(
        (slot + 9 * 3600) % 86400,
        9 * 3600,
        "09:00 at UTC+9 is 00:00Z"
    );
    assert!(slot > now().unwrap() && slot <= now().unwrap() + 86400 + 5);
    assert_eq!(created.timezone.as_deref(), Some("+09:00"));
    assert!(created.recent.is_empty() && created.latest.is_none());
    // Nothing is delivered before its local time.
    assert_eq!(sweep(&store, &scope, slot - 1, 1).await, 0);
    assert_eq!(count(&store, NOTICES).await, 0);
    // On time: one run with references and one notice, even though nothing changed.
    assert_eq!(sweep(&store, &scope, slot + 120, 1).await, 1);
    assert_eq!(sweep(&store, &scope, slot + 120, 1).await, 0);
    assert_eq!(
        (count(&store, RUNS).await, count(&store, NOTICES).await),
        (1, 1)
    );
    let title = text(&store, "SELECT title FROM local_notifications").await;
    assert_eq!(title, "오늘의 브리핑이 준비되었어요");
    let message = text(&store, "SELECT message FROM local_notifications").await;
    assert!(message.contains("메일과 캘린더는 읽지 않았어요") && !message.contains("메모"));
    let day = status(&store, &scope, &id).await;
    assert_eq!(day.next_due, Some(slot + 86400));
    assert_eq!(day.latest.as_ref().unwrap().sources.len(), 1);
    // The app was closed for two days: one brief for the latest slot, earlier ones counted.
    assert_eq!(sweep(&store, &scope, slot + 2 * 86400 + 600, 1).await, 1);
    let folded = status(&store, &scope, &id).await;
    assert_eq!(
        (folded.recent[0].outcome.as_str(), folded.recent[0].skipped),
        ("briefed", 1)
    );
    assert_eq!(count(&store, NOTICES).await, 2);
    // Opened far too late for that day's brief: recorded as missed, nothing delivered.
    assert_eq!(
        sweep(&store, &scope, slot + 3 * 86400 + 7 * 3600, 1).await,
        1
    );
    assert_eq!(count(&store, NOTICES).await, 2);
    let missed = status(&store, &scope, &id).await;
    assert_eq!(missed.recent[0].outcome, "missed");
    assert_eq!(missed.next_due, Some(slot + 4 * 86400));
}

#[tokio::test]
async fn stop_cancels_pending_pushes_and_replays_while_expiry_ends_checks() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let user = &scope.user;
    let id = Uuid::new_v4();
    let created = create(&store, &scope, watch(id)).await.unwrap();
    let id = id.to_string();
    let due = created.next_due.unwrap();
    remember!(store, user, "초록 수첩", "문구");
    assert_eq!(sweep(&store, &scope, due + 5, 1).await, 1);
    assert_eq!(count(&store, LIVE_PUSH).await, 1);
    let stop = |request: Uuid, revision| MonitorStop {
        request_id: request.to_string(),
        revision,
    };
    // A stale revision conflicts while the monitor is live.
    assert!(matches!(
        store
            .stop_member_monitor(
                scope.owner,
                user.clone(),
                id.parse().unwrap(),
                stop(Uuid::new_v4(), 99)
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let request = Uuid::new_v4();
    let stopped = store
        .stop_member_monitor(
            scope.owner,
            user.clone(),
            id.parse().unwrap(),
            stop(request, created.revision),
        )
        .await
        .unwrap();
    assert_eq!(
        (stopped.state.as_str(), stopped.next_due),
        ("stopped", None)
    );
    // Undelivered pushes are cancelled; the inbox record stays.
    assert_eq!(count(&store, LIVE_PUSH).await, 0);
    assert_eq!(count(&store, NOTICES).await, 1);
    // A duplicate stop returns the same stopped state without another change.
    let replay = store
        .stop_member_monitor(
            scope.owner,
            user.clone(),
            id.parse().unwrap(),
            stop(request, created.revision),
        )
        .await
        .unwrap();
    assert_eq!(replay, stopped);
    // A stopped monitor never checks or alerts again, even with changed evidence.
    remember!(store, user, "파랑 수첩", "또 문구");
    assert_eq!(sweep(&store, &scope, due + 3 * 86400, 1).await, 0);
    assert_eq!(count(&store, NOTICES).await, 1);
    // Expiry: the lapsed monitor is reported as expired and is not checked.
    let second = Uuid::new_v4();
    let two = create(&store, &scope, watch(second)).await.unwrap();
    exec(
        &store,
        format!("UPDATE member_work_monitors SET expires_at=unixepoch()-10 WHERE id='{second}'"),
    )
    .await;
    assert_eq!(
        status(&store, &scope, &second.to_string()).await.state,
        "expired"
    );
    assert_eq!(sweep(&store, &scope, two.next_due.unwrap() + 5, 1).await, 0);
    assert_eq!(
        text(
            &store,
            format!("SELECT state FROM member_work_monitors WHERE id='{second}'")
        )
        .await,
        "expired"
    );
    assert_eq!(count(&store, NOTICES).await, 1);
    let checks = status(&store, &scope, &second.to_string()).await.recent;
    assert_eq!(checks.len(), 1, "only the baseline");
}

#[tokio::test]
async fn revoked_authority_stops_checks_without_alerts() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let user = &scope.user;
    let (old, newer) = (Uuid::new_v4(), Uuid::new_v4());
    let first = create(&store, &scope, watch(old)).await.unwrap();
    create(&store, &scope, watch(newer)).await.unwrap();
    remember!(store, user, "초록 수첩", "문구");
    let due = first.next_due.unwrap() + 5;
    // A tick pinned to a newer gateway epoch revokes grants made under the older one.
    assert_eq!(sweep(&store, &scope, due, 2).await, 0);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM member_work_monitors WHERE state='revoked'"
        )
        .await,
        2
    );
    assert_eq!(count(&store, NOTICES).await, 0);
    // An auth epoch change (password reset, sign-out everywhere) revokes the same way.
    create(&store, &scope, watch(Uuid::new_v4())).await.unwrap();
    exec(&store, "UPDATE accounts SET auth_epoch=auth_epoch+1".into()).await;
    assert_eq!(sweep(&store, &scope, due, 1).await, 0);
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM member_work_monitors WHERE state='active'"
        )
        .await,
        0
    );
    assert_eq!(count(&store, NOTICES).await, 0);
    // The notification authority hook agrees: a revoked monitor's notice is not current.
    let notice = Uuid::new_v4().to_string();
    exec(
        &store,
        format!(
            "INSERT INTO local_notifications(tenant_id,id,account_id,schedule_id,occurrence_id,title,message,scheduled_for,delivered_at,delayed,quiet,quiet_revision)
             SELECT tenant_id,'{notice}',account_id,'{old}',NULL,'t','m',1,1,0,0,0 FROM member_work_monitors WHERE id='{old}'"
        ),
    )
    .await;
    let tenant = scope.owner.tenant.to_string();
    let current = store
        .call(move |db| notice_current(&db.conn, &tenant, &notice))
        .await
        .unwrap();
    assert_eq!(current, Some(false));
}

#[tokio::test]
async fn concurrent_ticks_and_restart_produce_one_alert() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member { store, scope, root } = member().await;
    let user = &scope.user;
    let id = Uuid::new_v4();
    let created = create(&store, &scope, watch(id)).await.unwrap();
    remember!(store, user, "초록 수첩", "문구");
    let due = created.next_due.unwrap() + 5;
    let (a, b) = tokio::join!(sweep(&store, &scope, due, 1), sweep(&store, &scope, due, 1));
    assert_eq!(a + b, 1);
    assert_eq!(count(&store, NOTICES).await, 1);
    // Closing and reopening the member database keeps the occurrence ledger and next due time.
    drop(store);
    let store = scope
        .owner
        .open_verified(&root.path().canonicalize().unwrap())
        .await
        .unwrap();
    assert_eq!(sweep(&store, &scope, due, 1).await, 0);
    assert_eq!(count(&store, NOTICES).await, 1);
    let saved = status(&store, &scope, &id.to_string()).await;
    assert_eq!((saved.state.as_str(), saved.alerts), ("active", 1));
    assert_eq!(saved.next_due, Some(created.next_due.unwrap() + 1800));
    // The next period resumes from the stored state; unchanged evidence stays quiet after restart.
    assert_eq!(sweep(&store, &scope, due + 1800, 1).await, 1);
    assert_eq!(count(&store, NOTICES).await, 1);
    // The same occurrence can never be recorded twice.
    let rejected = store
        .call(move |db| {
            Ok(db
                .conn
                .execute(
                    "INSERT INTO member_work_monitor_checks(monitor_id,due_at,checked_at,outcome)
                     SELECT monitor_id,due_at,1,'unchanged' FROM member_work_monitor_checks LIMIT 1",
                    [],
                )
                .is_err())
        })
        .await
        .unwrap();
    assert!(rejected);
}

/// Needs the hook in `tick_member_workflows` (see the hand-off notes). Until that is wired this
/// fails on purpose: the timer is what runs monitors while the app is closed.
#[tokio::test]
async fn tick_member_workflows_runs_monitors() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let user = &scope.user;
    let id = Uuid::new_v4();
    create(&store, &scope, watch(id)).await.unwrap();
    remember!(store, user, "초록 수첩", "문구");
    exec(
        &store,
        format!("UPDATE member_work_monitors SET next_due=unixepoch()-5 WHERE id='{id}'"),
    )
    .await;
    let (a, b) = tokio::join!(
        store.tick_member_workflows(scope.owner, 1),
        store.tick_member_workflows(scope.owner, 1)
    );
    assert_eq!(a.unwrap() + b.unwrap(), 1);
    assert_eq!(count(&store, NOTICES).await, 1);
    assert!(
        status(&store, &scope, &id.to_string())
            .await
            .next_due
            .unwrap()
            > now().unwrap()
    );
}
