//! Actual SQLite path with explicit clocks. The reader is the typed `PublicRead` hand-off, so these tests
//! exercise the store's grant, claim, verification and settlement; the broker is covered by the app tests.
use super::*;
use crate::personal_browser::tasks::tests::{Member, member};

const URL: &str = "https://status.example.test/service";
const NOTICES: &str = "SELECT count(*) FROM local_notifications";

fn request(thread: &str, id: Uuid) -> PublicWatchRequest {
    PublicWatchRequest {
        request_id: id.to_string(),
        thread_id: thread.into(),
        message: "https://status.example.test/service 를 매시간 확인하고 바뀔 때만 알려줘".into(),
        url: URL.into(),
        every_seconds: 3600,
        days: 7,
    }
}
async fn grant(store: &Store, m: &Scope, request: PublicWatchRequest) -> Result<PublicWatchView> {
    store
        .create_public_watch(m.owner, m.user.clone(), 1, request)
        .await
}
async fn listed(store: &Store, m: &Scope) -> Vec<PublicWatchView> {
    store
        .public_watches(m.owner, m.user.clone(), 1, m.thread.parse().unwrap())
        .await
        .unwrap()
        .items
}
async fn claim_at(store: &Store, m: &Scope, time: i64, epoch: i64) -> Vec<PublicWatchLease> {
    store
        .claim_due_public_watches(m.owner, epoch, time)
        .await
        .unwrap()
}
async fn scalar<T: rusqlite::types::FromSql + Send + 'static>(
    store: &Store,
    sql: impl Into<String>,
) -> T {
    let sql = sql.into();
    store
        .call(move |db| Ok(db.conn.query_row(&sql, [], |r| r.get(0))?))
        .await
        .unwrap()
}
async fn exec(store: &Store, sql: impl Into<String>) {
    let sql = sql.into();
    store
        .call(move |db| {
            db.conn.execute(&sql, [])?;
            Ok(())
        })
        .await
        .unwrap();
}
fn page(url: &str, text: &str, observed_at: i64) -> PublicReadOutcome {
    PublicReadOutcome::Page(PublicPage {
        url: url.into(),
        title: "Status".into(),
        text: text.into(),
        observed_at,
    })
}
async fn settle(
    store: &Store,
    lease: &PublicWatchLease,
    outcome: PublicReadOutcome,
    closed: bool,
    time: i64,
) {
    store
        .settle_public_watch_at(lease.clone(), PublicRead { outcome, closed }, time)
        .await
        .unwrap();
}
/// Claims the one due slot at `time`, settles it one second later, and returns the stored check state.
async fn step(
    store: &Store,
    m: &Scope,
    time: i64,
    make: impl FnOnce(&PublicWatchLease) -> PublicReadOutcome,
) -> String {
    let mut leases = claim_at(store, m, time, 1).await;
    assert_eq!(leases.len(), 1, "exactly one slot is due at {time}");
    let lease = leases.remove(0);
    let outcome = make(&lease);
    settle(store, &lease, outcome, true, time + 1).await;
    scalar(
        store,
        format!(
            "SELECT state FROM member_public_watch_checks WHERE watch_id='{}' AND due_at={}",
            lease.watch_id, lease.due_at
        ),
    )
    .await
}

#[tokio::test]
async fn grant_is_pinned_validated_replayed_bounded_and_scoped_to_its_thread() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let id = Uuid::new_v4();
    let first = grant(&store, &scope, request(&scope.thread, id))
        .await
        .unwrap();
    assert_eq!(
        (
            first.state.as_str(),
            first.revision,
            first.url.as_str(),
            first.every_seconds,
            first.days
        ),
        ("active", 1, URL, 3600, 7)
    );
    assert_eq!(first.thread_id, scope.thread);
    assert!(
        first.checks.is_empty(),
        "the baseline is taken by the timer, never inside the request"
    );
    // Pinned owner / volume / generation / auth epoch / host epoch / thread / host / id / URL, due at once.
    let pinned = format!(
        "SELECT count(*) FROM member_public_watches WHERE id='{id}' AND tenant_id='{}' AND account_id='{}'
         AND volume_id='{}' AND generation=1 AND auth_epoch={} AND host_epoch=1 AND thread_id='{}'
         AND host='status.example.test' AND url='{URL}' AND every_seconds=3600 AND days=7 AND state='active'
         AND next_due=created_at AND expires_at=created_at+7*86400",
        scope.owner.tenant, scope.owner.account, scope.owner.volume, scope.user.epoch, scope.thread
    );
    assert_eq!(scalar::<i64>(&store, pinned).await, 1);
    // The grant is a normal turn in the original conversation.
    let turns: i64 = scalar(
        &store,
        format!(
            "SELECT count(*) FROM chat_runs WHERE thread_id='{}' AND request_id='{id}'",
            scope.thread
        ),
    )
    .await;
    assert_eq!(turns, 1);
    // Replays add nothing; a different body under the same id is a conflict.
    assert_eq!(
        grant(&store, &scope, request(&scope.thread, id))
            .await
            .unwrap(),
        first
    );
    assert_eq!(
        scalar::<i64>(&store, "SELECT count(*) FROM member_public_watches").await,
        1
    );
    assert_eq!(
        scalar::<i64>(
            &store,
            "SELECT count(*) FROM chat_runs WHERE request_id=(SELECT id FROM member_public_watches)"
        )
        .await,
        1
    );
    let mut other = request(&scope.thread, id);
    other.every_seconds = 1800;
    assert!(matches!(
        grant(&store, &scope, other).await,
        Err(StoreError::Conflict)
    ));
    let mut other = request(&scope.thread, id);
    other.url = "https://status.example.test/other".into();
    assert!(matches!(
        grant(&store, &scope, other).await,
        Err(StoreError::Conflict)
    ));
    // Only explicit public https pages: no scheme downgrade, IP literal, private name, credential,
    // port, query or fragment; and only the three cadences, 1..=90 days, a message and canonical ids.
    let mut bad = Vec::new();
    for url in [
        "http://status.example.test/service",
        "ftp://status.example.test/",
        "https://203.0.113.9/service",
        "https://127.1/",
        "https://[2001:db8::1]/",
        "https://user:pw@status.example.test/",
        "https://status.example.test:8443/",
        "https://status.example.test/?token=1",
        "https://status.example.test/#frag",
        "https://localhost/",
        "https://intranet/",
        "https://printer.local/",
        "https://db.internal/",
        "not a url",
        "",
    ] {
        let mut r = request(&scope.thread, Uuid::new_v4());
        r.url = url.into();
        bad.push(r);
    }
    for edit in [
        (|r: &mut PublicWatchRequest| r.every_seconds = 60) as fn(&mut PublicWatchRequest),
        |r| r.every_seconds = 7200,
        |r| r.days = 0,
        |r| r.days = 91,
        |r| r.message = "  ".into(),
        |r| r.request_id = "nope".into(),
        |r| r.thread_id = "nope".into(),
    ] {
        let mut r = request(&scope.thread, Uuid::new_v4());
        edit(&mut r);
        bad.push(r);
    }
    for r in bad {
        let label = r.url.clone();
        assert!(
            matches!(
                grant(&store, &scope, r).await,
                Err(StoreError::InvalidInput)
            ),
            "{label}"
        );
    }
    assert_eq!(
        scalar::<i64>(&store, "SELECT count(*) FROM member_public_watches").await,
        1
    );
    // Listing is per thread; another conversation sees nothing.
    let elsewhere = store
        .create_thread(&scope.user, "Other".into())
        .await
        .unwrap()
        .id;
    let other_thread = store
        .public_watches(
            scope.owner,
            scope.user.clone(),
            1,
            elsewhere.parse().unwrap(),
        )
        .await
        .unwrap();
    assert!(other_thread.items.is_empty());
    assert_eq!(listed(&store, &scope).await, vec![first]);
    // Bounded: four active watches per member.
    for _ in 0..3 {
        grant(&store, &scope, request(&scope.thread, Uuid::new_v4()))
            .await
            .unwrap();
    }
    assert!(matches!(
        grant(&store, &scope, request(&scope.thread, Uuid::new_v4())).await,
        Err(StoreError::BudgetExhausted)
    ));
}

#[tokio::test]
async fn baseline_then_one_alert_only_for_new_visible_content_and_each_slot_once() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    grant(&store, &scope, request(&scope.thread, Uuid::new_v4()))
        .await
        .unwrap();
    let t0: i64 = scalar(&store, "SELECT created_at FROM member_public_watches").await;
    // Slot 0: claimed once with a fresh disposable workspace; the same instant claims nothing more.
    let mut leases = claim_at(&store, &scope, t0, 1).await;
    assert_eq!(leases.len(), 1);
    let lease = leases.remove(0);
    assert_eq!(
        (lease.url.as_str(), lease.host.as_str(), lease.due_at),
        (URL, "status.example.test", t0)
    );
    assert!(claim_at(&store, &scope, t0, 1).await.is_empty());
    assert_eq!(
        scalar::<String>(&store, "SELECT state FROM member_public_watch_checks").await,
        "claimed"
    );
    assert_eq!(
        scalar::<i64>(&store, "SELECT next_due FROM member_public_watches").await,
        t0 + 3600
    );
    settle(
        &store,
        &lease,
        page(URL, "All systems operational", t0),
        true,
        t0 + 5,
    )
    .await;
    let shown = listed(&store, &scope).await.remove(0);
    assert_eq!(
        shown.checks,
        vec![PublicWatchCheck {
            due_at: t0,
            state: "baseline".into(),
            excerpt: Some("All systems operational".into())
        }]
    );
    assert_eq!(
        scalar::<i64>(&store, NOTICES).await,
        0,
        "the baseline never alerts"
    );
    // Same visible content (different whitespace, another URL path after the redirect): unchanged.
    let state = step(&store, &scope, t0 + 3600, |_| {
        page(
            "https://status.example.test/service/",
            "  All systems\n   operational ",
            t0 + 3600,
        )
    })
    .await;
    assert_eq!(state, "unchanged");
    assert_eq!(scalar::<i64>(&store, NOTICES).await, 0);
    assert_eq!(listed(&store, &scope).await[0].checks[0].excerpt, None);
    // New visible content: exactly one alert, carrying the host and no page text.
    let mut leases = claim_at(&store, &scope, t0 + 7230, 1).await;
    assert_eq!(leases.len(), 1);
    let lease = leases.remove(0);
    assert_eq!(lease.due_at, t0 + 7200);
    assert!(
        claim_at(&store, &scope, t0 + 7231, 1).await.is_empty(),
        "claimed slots are never re-claimed"
    );
    settle(
        &store,
        &lease,
        page(URL, "Major outage: database unavailable", t0 + 7230),
        true,
        t0 + 7232,
    )
    .await;
    let shown = listed(&store, &scope).await.remove(0);
    assert_eq!(shown.checks[0].state, "changed");
    assert_eq!(
        shown.checks[0].excerpt.as_deref(),
        Some("Major outage: database unavailable")
    );
    assert_eq!(scalar::<i64>(&store, NOTICES).await, 1);
    assert_eq!(
        scalar::<i64>(&store, "SELECT alerts FROM member_public_watches").await,
        1
    );
    let message: String = scalar(&store, "SELECT message FROM local_notifications").await;
    assert!(message.contains("status.example.test") && !message.contains("database"));
    // A duplicate settlement of the same slot (retry, lost reply) changes nothing and alerts nothing.
    settle(
        &store,
        &lease,
        page(URL, "Something else entirely", t0 + 7233),
        true,
        t0 + 7234,
    )
    .await;
    assert_eq!(scalar::<i64>(&store, NOTICES).await, 1);
    assert_eq!(listed(&store, &scope).await[0].checks[0].state, "changed");
    // The new content is now the reference: seeing it again is not a second change.
    let state = step(&store, &scope, t0 + 10800, |_| {
        page(URL, "Major outage: database unavailable", t0 + 10800)
    })
    .await;
    assert_eq!(state, "unchanged");
    assert_eq!(scalar::<i64>(&store, NOTICES).await, 1);
    // Only a hash is kept on the watch; no page text outside the encrypted excerpt.
    assert_eq!(
        scalar::<i64>(
            &store,
            "SELECT length(fingerprint) FROM member_public_watches"
        )
        .await,
        64
    );
    assert_eq!(
        scalar::<i64>(
            &store,
            "SELECT count(*) FROM member_public_watch_checks WHERE closed=0"
        )
        .await,
        0
    );
    assert!(
        store
            .public_watch_orphans(scope.owner, t0 + 10801)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_failed_or_unverified_read_is_never_a_change_and_never_resets_the_reference() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    grant(&store, &scope, request(&scope.thread, Uuid::new_v4()))
        .await
        .unwrap();
    let t0: i64 = scalar(&store, "SELECT created_at FROM member_public_watches").await;
    assert_eq!(
        step(&store, &scope, t0, |_| page(
            URL,
            "All systems operational",
            t0
        ))
        .await,
        "baseline"
    );
    let reference: String = scalar(&store, "SELECT fingerprint FROM member_public_watches").await;
    // Each case builds its outcome from the slot time `t`.
    let failures: [(&str, fn(i64) -> PublicReadOutcome); 4] = [
        ("browser_unavailable", |_| {
            PublicReadOutcome::Failed("browser_unavailable")
        }),
        // Redirected to another host: not the pinned page, even if its text is new.
        ("redirected_off_host", |t| {
            page("https://evil.example.test/service", "Major outage", t)
        }),
        // Observed long before the slot was claimed: not a fresh observation.
        ("stale_observation", |t| page(URL, "Major outage", t - 3600)),
        ("empty_page", |t| page(URL, " \n\t ", t)),
    ];
    for (i, (reason, make)) in failures.into_iter().enumerate() {
        let time = t0 + 3600 * (i as i64 + 1);
        let state = step(&store, &scope, time, |_| make(time)).await;
        assert_eq!(state, "failed", "{reason}");
        let stored: String = scalar(
            &store,
            format!("SELECT reason FROM member_public_watch_checks WHERE due_at={time}"),
        )
        .await;
        assert_eq!(stored, reason);
        assert_eq!(scalar::<i64>(&store, NOTICES).await, 0, "{reason}");
        assert_eq!(
            scalar::<String>(&store, "SELECT fingerprint FROM member_public_watches").await,
            reference
        );
    }
    // The same content as before the outage is unchanged, not a new baseline and not a change.
    let t = t0 + 3600 * 5;
    assert_eq!(
        step(&store, &scope, t, |_| page(
            URL,
            "All systems operational",
            t
        ))
        .await,
        "unchanged"
    );
    assert_eq!(
        scalar::<i64>(&store, "SELECT alerts FROM member_public_watches").await,
        0
    );
    // Genuinely new content after all that is one change.
    let t = t0 + 3600 * 6;
    assert_eq!(
        step(&store, &scope, t, |_| page(URL, "Degraded performance", t)).await,
        "changed"
    );
    assert_eq!(scalar::<i64>(&store, NOTICES).await, 1);
    // Failed checks carry no excerpt: nothing is invented.
    let shown = listed(&store, &scope).await.remove(0);
    assert!(
        shown
            .checks
            .iter()
            .filter(|c| c.state == "failed")
            .all(|c| c.excerpt.is_none())
    );
}

#[tokio::test]
async fn restart_and_crash_dedup_never_rereads_and_stops_only_its_own_disposable_workspaces() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    // A human's live workspace on the same thread, which the watch must never address.
    let Admission::Fresh(human) = store
        .personal_browser_open(
            scope.clone(),
            Uuid::new_v4(),
            "https://example.org/".into(),
            "Read it".into(),
            false,
        )
        .await
        .unwrap()
    else {
        panic!("fresh human workspace")
    };
    grant(&store, &scope, request(&scope.thread, Uuid::new_v4()))
        .await
        .unwrap();
    let t0: i64 = scalar(&store, "SELECT created_at FROM member_public_watches").await;
    let first = claim_at(&store, &scope, t0, 1).await.remove(0);
    assert_ne!(first.workspace_id.to_string(), human.view.id);
    // Process restart while the read was in flight: the claim is `unknown`, never re-claimed.
    store.call(|db| restart(&db.conn)).await.unwrap();
    assert_eq!(
        scalar::<String>(&store, "SELECT state FROM member_public_watch_checks").await,
        "unknown"
    );
    assert!(claim_at(&store, &scope, t0 + 1, 1).await.is_empty());
    // The lost reader's late result is discarded: no baseline, no alert. Its workspace was not
    // confirmed closed, so the timer stops exactly that disposable workspace and nothing else.
    settle(
        &store,
        &first,
        page(URL, "Late result", t0 + 2),
        false,
        t0 + 3,
    )
    .await;
    assert_eq!(
        scalar::<String>(&store, "SELECT state FROM member_public_watch_checks").await,
        "unknown"
    );
    assert_eq!(
        scalar::<Option<String>>(&store, "SELECT fingerprint FROM member_public_watches").await,
        None
    );
    assert_eq!(scalar::<i64>(&store, NOTICES).await, 0);
    let orphans = store
        .public_watch_orphans(scope.owner, t0 + 4)
        .await
        .unwrap();
    assert_eq!(orphans, vec![first.workspace_id.to_string()]);
    store
        .ack_public_watch_workspace(scope.owner, orphans[0].clone())
        .await
        .unwrap();
    assert!(
        store
            .public_watch_orphans(scope.owner, t0 + 5)
            .await
            .unwrap()
            .is_empty()
    );
    // A claim that never settles (lost task) is reaped by age instead of by restart.
    let second = claim_at(&store, &scope, t0 + 3600, 1).await.remove(0);
    assert_ne!(second.workspace_id, first.workspace_id);
    assert!(
        store
            .public_watch_orphans(scope.owner, t0 + 3600 + STALE_CLAIM - 1)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        scalar::<String>(
            &store,
            format!(
                "SELECT state FROM member_public_watch_checks WHERE due_at={}",
                t0 + 3600
            )
        )
        .await,
        "claimed"
    );
    let orphans = store
        .public_watch_orphans(scope.owner, t0 + 3600 + STALE_CLAIM + 1)
        .await
        .unwrap();
    assert_eq!(orphans, vec![second.workspace_id.to_string()]);
    assert_eq!(
        scalar::<String>(
            &store,
            format!(
                "SELECT state FROM member_public_watch_checks WHERE due_at={}",
                t0 + 3600
            )
        )
        .await,
        "unknown"
    );
    // Two timers racing for the same slot: exactly one claim.
    let (a, b) = tokio::join!(
        store.claim_due_public_watches(scope.owner, 1, t0 + 7200),
        store.claim_due_public_watches(scope.owner, 1, t0 + 7200)
    );
    assert_eq!(a.unwrap().len() + b.unwrap().len(), 1);
    // The human workspace was never touched: still registered, unchanged, never stopping or offered for stop.
    let current = store
        .personal_browser_current(scope.clone())
        .await
        .unwrap()
        .unwrap()
        .view;
    assert_eq!(
        (
            current.id.as_str(),
            current.state.as_str(),
            current.revision
        ),
        (human.view.id.as_str(), "starting", human.view.revision)
    );
    assert_eq!(
        scalar::<i64>(&store, format!("SELECT count(*) FROM personal_browser_workers WHERE workspace_id='{}' AND stopping=0", human.view.id)).await,
        1
    );
    assert_eq!(
        scalar::<i64>(
            &store,
            format!(
                "SELECT count(*) FROM member_public_watch_checks WHERE workspace_id='{}'",
                human.view.id
            )
        )
        .await,
        0
    );
}

#[tokio::test]
async fn stop_revoke_and_expiry_end_the_watch_discard_in_flight_results_and_cancel_alerts() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let id = Uuid::new_v4();
    grant(&store, &scope, request(&scope.thread, id))
        .await
        .unwrap();
    let t0: i64 = scalar(&store, "SELECT created_at FROM member_public_watches").await;
    assert_eq!(
        step(&store, &scope, t0, |_| page(
            URL,
            "All systems operational",
            t0
        ))
        .await,
        "baseline"
    );
    // Stop while a read is in flight: stale revision conflicts, the right one ends the watch, and the
    // in-flight result is withdrawn with no alert and no new reference.
    let inflight = claim_at(&store, &scope, t0 + 3600, 1).await.remove(0);
    let stop = |revision| PublicWatchStop {
        request_id: Uuid::new_v4().to_string(),
        revision,
    };
    assert!(matches!(
        store
            .stop_public_watch(scope.owner, scope.user.clone(), 1, id, stop(9))
            .await,
        Err(StoreError::Conflict)
    ));
    let stopped = store
        .stop_public_watch(scope.owner, scope.user.clone(), 1, id, stop(1))
        .await
        .unwrap();
    assert_eq!((stopped.state.as_str(), stopped.revision), ("stopped", 2));
    let again = store
        .stop_public_watch(scope.owner, scope.user.clone(), 1, id, stop(1))
        .await
        .unwrap();
    assert_eq!(again, stopped, "a replayed stop returns the ended watch");
    let reference: String = scalar(&store, "SELECT fingerprint FROM member_public_watches").await;
    settle(
        &store,
        &inflight,
        page(URL, "Major outage", t0 + 3601),
        true,
        t0 + 3602,
    )
    .await;
    assert_eq!(listed(&store, &scope).await[0].checks[0].state, "withdrawn");
    assert_eq!(scalar::<i64>(&store, NOTICES).await, 0);
    assert_eq!(
        scalar::<String>(&store, "SELECT fingerprint FROM member_public_watches").await,
        reference
    );
    assert!(claim_at(&store, &scope, t0 + 99999, 1).await.is_empty());
    // A newer gateway/host epoch revokes the grant: private excerpts are dropped and its alert is no
    // longer current, and its undelivered push is suppressed.
    let id = Uuid::new_v4();
    grant(&store, &scope, request(&scope.thread, id))
        .await
        .unwrap();
    let t1: i64 = scalar(
        &store,
        format!("SELECT created_at FROM member_public_watches WHERE id='{id}'"),
    )
    .await;
    assert_eq!(
        step(&store, &scope, t1, |_| page(URL, "A", t1)).await,
        "baseline"
    );
    assert_eq!(
        step(&store, &scope, t1 + 3600, |_| page(URL, "B", t1 + 3600)).await,
        "changed"
    );
    let notice: String = scalar(&store, "SELECT id FROM local_notifications").await;
    let tenant = scope.owner.tenant.to_string();
    let current = |notice: String, tenant: String| {
        let store = store.clone();
        async move {
            store
                .call(move |db| notice_current(&db.conn, &tenant, &notice))
                .await
                .unwrap()
        }
    };
    assert_eq!(current(notice.clone(), tenant.clone()).await, Some(true));
    assert!(claim_at(&store, &scope, t1 + 3601, 2).await.is_empty());
    let revoked = listed(&store, &scope)
        .await
        .into_iter()
        .find(|w| w.id == id.to_string())
        .unwrap();
    assert_eq!(revoked.state, "revoked");
    assert!(revoked.checks.iter().all(|c| c.excerpt.is_none()));
    assert_eq!(current(notice, tenant).await, Some(false));
    assert_eq!(
        scalar::<i64>(
            &store,
            "SELECT count(*) FROM local_notifications WHERE push_suppressed=0"
        )
        .await,
        0
    );
    // The end date expires a watch on its own, even if nothing was ever read.
    let id = Uuid::new_v4();
    let mut short = request(&scope.thread, id);
    short.days = 1;
    grant(&store, &scope, short).await.unwrap();
    let t3: i64 = scalar(
        &store,
        format!("SELECT created_at FROM member_public_watches WHERE id='{id}'"),
    )
    .await;
    assert!(claim_at(&store, &scope, t3 + 86400 + 5, 1).await.is_empty());
    assert_eq!(
        scalar::<String>(
            &store,
            format!("SELECT state FROM member_public_watches WHERE id='{id}'")
        )
        .await,
        "expired"
    );
    // A session/auth epoch change (password reset, sign-out everywhere) revokes the same way.
    let id = Uuid::new_v4();
    grant(&store, &scope, request(&scope.thread, id))
        .await
        .unwrap();
    exec(&store, "UPDATE accounts SET auth_epoch=auth_epoch+1").await;
    let t4: i64 = scalar(
        &store,
        format!("SELECT created_at FROM member_public_watches WHERE id='{id}'"),
    )
    .await;
    assert!(claim_at(&store, &scope, t4, 1).await.is_empty());
    assert_eq!(
        scalar::<String>(
            &store,
            format!("SELECT state FROM member_public_watches WHERE id='{id}'")
        )
        .await,
        "revoked"
    );
}

#[tokio::test]
async fn last_slot_stays_stoppable_and_a_new_host_epoch_blocks_a_claimed_read_before_dispatch() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    let first = Uuid::new_v4();
    grant(&store, &scope, request(&scope.thread, first))
        .await
        .unwrap();
    let t0: i64 = scalar(&store, "SELECT created_at FROM member_public_watches").await;
    exec(
        &store,
        format!(
            "UPDATE member_public_watches SET expires_at={} WHERE id='{first}'",
            t0 + 60
        ),
    )
    .await;
    let lease = claim_at(&store, &scope, t0, 1).await.pop().unwrap();
    assert!(store.public_watch_can_read(lease.clone()).await.unwrap());
    let stopped = store
        .stop_public_watch(
            scope.owner,
            scope.user.clone(),
            1,
            first,
            PublicWatchStop {
                request_id: Uuid::new_v4().to_string(),
                revision: 1,
            },
        )
        .await
        .unwrap();
    assert_eq!(stopped.state, "stopped");
    assert!(!store.public_watch_can_read(lease.clone()).await.unwrap());
    settle(&store, &lease, page(URL, "late", t0 + 1), true, t0 + 1).await;
    assert_eq!(listed(&store, &scope).await[0].checks[0].state, "withdrawn");
    let second = Uuid::new_v4();
    grant(&store, &scope, request(&scope.thread, second))
        .await
        .unwrap();
    let lease = claim_at(&store, &scope, t0 + 2, 1).await.pop().unwrap();
    store
        .fence_public_watch_epoch(scope.owner, 2)
        .await
        .unwrap();
    assert!(!store.public_watch_can_read(lease.clone()).await.unwrap());
    settle(
        &store,
        &lease,
        page(URL, "old authority", t0 + 3),
        true,
        t0 + 3,
    )
    .await;
    let page = store
        .public_watches(
            scope.owner,
            scope.user.clone(),
            2,
            scope.thread.parse().unwrap(),
        )
        .await
        .unwrap();
    let record = page
        .items
        .iter()
        .find(|w| w.id == second.to_string())
        .unwrap();
    assert_eq!(record.state, "revoked");
    assert!(record.checks.iter().all(|c| c.excerpt.is_none()));
    assert_eq!(scalar::<i64>(&store, NOTICES).await, 0);
}

#[tokio::test]
async fn restored_watch_keeps_the_worklog_and_retires_evidence_and_pending_reads() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let Member {
        store,
        scope,
        root: _root,
    } = member().await;
    grant(&store, &scope, request(&scope.thread, Uuid::new_v4()))
        .await
        .unwrap();
    let t: i64 = scalar(&store, "SELECT created_at FROM member_public_watches").await;
    assert_eq!(
        step(&store, &scope, t, |_| page(URL, "Baseline", t)).await,
        "baseline"
    );
    assert_eq!(
        step(&store, &scope, t + 3600, |_| page(URL, "Changed", t + 3600)).await,
        "changed"
    );
    let pending = claim_at(&store, &scope, t + 7200, 1).await.remove(0);
    store
        .call(|db| crate::schedules::quarantine_for_restore(&db.conn))
        .await
        .unwrap();
    assert!(!store.public_watch_can_read(pending.clone()).await.unwrap());
    settle(
        &store,
        &pending,
        page(URL, "After restoration", t + 7201),
        true,
        t + 7201,
    )
    .await;
    assert!(claim_at(&store, &scope, t + 10800, 1).await.is_empty());
    assert_eq!(scalar::<i64>(&store,"SELECT count(*) FROM member_public_watch_checks WHERE state='withdrawn' AND cipher IS NULL").await,3);
    assert_eq!(
        scalar::<i64>(
            &store,
            "SELECT count(*) FROM local_notifications WHERE dismissed=1 AND push_suppressed=1"
        )
        .await,
        1
    );
}
