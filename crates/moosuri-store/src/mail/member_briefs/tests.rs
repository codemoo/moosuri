use super::*;
use crate::tests::{TEST_LOCK, directory};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};

const PASSWORD: &str = "Synthetic-brief-password-42";
const CHANGED: &str = "Changed-brief-password-42";
const CANARY: &str = "BRIEF-MAIL-SUBJECT-CANARY";
const TITLE: &str = "BRIEF-EVENT-TITLE-CANARY";

async fn setup() -> (tempfile::TempDir, Store, UserContext, VolumeOwner, Uuid) {
    let dir = directory();
    let owner = VolumeOwner {
        account: Uuid::new_v4(),
        tenant: Uuid::new_v4(),
        volume: Uuid::new_v4(),
        generation: 1,
    };
    owner
        .initialize(
            dir.path(),
            "Brief member".into(),
            "member@example.test".into(),
            PASSWORD.into(),
        )
        .await
        .unwrap();
    let s = owner.open_verified(dir.path()).await.unwrap();
    let first = s
        .login("member@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let u = s
        .change_password(&first.user, CHANGED.into())
        .await
        .unwrap()
        .user;
    (dir, s, u, owner, Uuid::new_v4())
}
fn wire_owner(o: VolumeOwner) -> Value {
    json!({"account_id":o.account,"tenant_id":o.tenant,"volume_id":o.volume,"generation":o.generation,"peer_uid":unsafe{libc::geteuid()}})
}
fn receipt(
    o: VolumeOwner,
    app: Uuid,
    service: MemberGoogleService,
    purpose: &str,
    expires: i64,
) -> String {
    let mut claims = json!({"v":1,"owner":wire_owner(o),"app_revision":app,"purpose":service.binding_purpose(purpose),"mailbox":"member@example.test","token_tag":URL_SAFE_NO_PAD.encode([7u8;32]),"expires_at":expires});
    if service != MemberGoogleService::Gmail {
        claims["subject"] = json!("synthetic-subject");
    }
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap()),
        URL_SAFE_NO_PAD.encode([8u8; 32])
    )
}
/// Synthetic broker exchange; no network, no real credentials.
async fn connect(
    s: &Store,
    u: &UserContext,
    o: VolumeOwner,
    app: Uuid,
    service: MemberGoogleService,
) -> (Uuid, i64) {
    let start = s
        .start_member_service_oauth(u, o, service, app)
        .await
        .unwrap();
    let handle = s
        .capture_member_service_oauth(
            service,
            start.state,
            Some(SecretString::new("synthetic-code".into()).unwrap()),
        )
        .await
        .unwrap();
    let e = s
        .claim_member_service_oauth(u, o, service, app, handle.unwrap())
        .await
        .unwrap();
    let id = e.member_google_request_id().unwrap();
    let t = now().unwrap();
    let reply = json!({"v":1,"request_id":id,"owner":wire_owner(o),"app_revision":app,"state":"ok","result":{
        "access_token":"SYNTHETIC-BRIEF-ACCESS","refresh_token":"SYNTHETIC-BRIEF-REFRESH","expires_in":3600,"scopes":[service.scope()],
        "mailbox":"member@example.test","access_binding":receipt(o,app,service,"access",t+3600),"refresh_binding":receipt(o,app,service,"refresh",t+86400)}});
    let grant = MemberGoogleGrant::from_broker_reply(
        id,
        Zeroizing::new(serde_json::to_vec(&reply).unwrap()),
    )
    .unwrap();
    s.complete_member_google_oauth(e, grant).await.unwrap();
    let st = s.member_service_status(u, o, service, app).await.unwrap();
    (st.connection_id.unwrap(), st.epoch)
}
fn request(thread: &str, g: (Uuid, i64), c: (Uuid, i64)) -> MemberBriefRequest {
    MemberBriefRequest {
        request_id: Uuid::new_v4().to_string(),
        thread_id: thread.into(),
        consent: true,
        message: Some("매일 아침 8시에 오늘 일정과 중요한 메일을 알려줘".into()),
        gmail: Some(MemberBriefPin {
            connection_id: g.0.to_string(),
            epoch: g.1,
        }),
        calendar: Some(MemberBriefPin {
            connection_id: c.0.to_string(),
            epoch: c.1,
        }),
        at: "08:00".into(),
        timezone: "Asia/Seoul".into(),
        interval_days: 1,
        days: 7,
        important_only: false,
    }
}
fn evidence() -> MemberBriefOutcome {
    let message = |i: usize| MemberBriefMessage {
        subject: format!("{CANARY} {i}\u{7}"),
        from: "sender@example.test".into(),
        received_at_ms: 1,
    };
    MemberBriefOutcome::Evidence(MemberBriefEvidence {
        messages: Some((0..7).map(message).collect()),
        events: Some(vec![MemberBriefEvent {
            title: TITLE.into(),
            start: "2026-11-26T10:00:00+09:00".into(),
            end: "2026-11-26T11:00:00+09:00".into(),
            all_day: false,
        }]),
    })
}
async fn count(s: &Store, sql: &'static str, id: String) -> i64 {
    s.call(move |db| Ok(db.conn.query_row(sql, [id], |r| r.get::<_, i64>(0))?))
        .await
        .unwrap()
}
async fn ciphers(s: &Store) -> Vec<Vec<u8>> {
    s.call(|db| {
        let mut q = db
            .conn
            .prepare("SELECT cipher FROM member_connected_brief_runs WHERE cipher IS NOT NULL")?;
        Ok(q.query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<Vec<u8>>, _>>()?)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn grant_is_explicit_pinned_to_current_connections_and_idempotent() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let gmail = connect(&s, &u, o, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&s, &u, o, app, MemberGoogleService::Calendar).await;
    let thread = s.create_thread(&u, "Morning".into()).await.unwrap().id;
    let base = request(&thread, gmail, calendar);

    let mut no_consent = base.clone();
    no_consent.consent = false;
    assert!(matches!(
        s.create_member_brief(o, u.clone(), 1, no_consent).await,
        Err(StoreError::InvalidInput)
    ));
    let mut stale = base.clone();
    stale.calendar = Some(MemberBriefPin {
        connection_id: calendar.0.to_string(),
        epoch: calendar.1 + 1,
    });
    assert!(matches!(
        s.create_member_brief(o, u.clone(), 1, stale).await,
        Err(StoreError::Conflict)
    ));
    let mut elsewhere = base.clone();
    elsewhere.thread_id = Uuid::new_v4().to_string();
    assert!(matches!(
        s.create_member_brief(o, u.clone(), 1, elsewhere).await,
        Err(StoreError::NotFound)
    ));
    // The wire contract is closed: nothing like a send/write flag can be smuggled in.
    let mut wire = serde_json::to_value(&base).unwrap();
    wire["send"] = json!(true);
    assert!(serde_json::from_value::<MemberBriefRequest>(wire).is_err());

    let made = s
        .create_member_brief(o, u.clone(), 1, base.clone())
        .await
        .unwrap();
    assert_eq!(
        (made.state.as_str(), made.sources.clone(), made.read_only),
        ("active", vec!["gmail".to_string(), "calendar".into()], true)
    );
    assert!(made.next_due.unwrap() > now().unwrap() && made.runs.is_empty());
    let replay = s
        .create_member_brief(o, u.clone(), 1, base.clone())
        .await
        .unwrap();
    assert_eq!(
        (replay.id.as_str(), replay.revision),
        (made.id.as_str(), made.revision)
    );
    let task = s
        .personal_task_detail(o, u.clone(), made.id.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(task.task.kind, "connected_brief");
    assert_eq!(task.request, base.message.as_deref().unwrap());
    assert_eq!(task.origin_turn_id.as_deref(), Some(made.id.as_str()));
    assert!(
        s.member_briefs(o, u.clone(), 1, Some(Uuid::new_v4()))
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let mut changed = base;
    changed.at = "09:00".into();
    assert!(matches!(
        s.create_member_brief(o, u, 1, changed).await,
        Err(StoreError::Conflict)
    ));
}

#[tokio::test]
async fn important_only_is_explicit_pinned_in_the_fingerprint_and_reaches_the_lease() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let gmail = connect(&s, &u, o, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&s, &u, o, app, MemberGoogleService::Calendar).await;
    let thread = s.create_thread(&u, "Morning".into()).await.unwrap().id;

    // Omitted on the wire means the ordinary brief; the flag is a typed boolean, never a string.
    let mut wire = serde_json::to_value(request(&thread, gmail, calendar)).unwrap();
    wire.as_object_mut().unwrap().remove("important_only");
    assert!(
        !serde_json::from_value::<MemberBriefRequest>(wire.clone())
            .unwrap()
            .important_only
    );
    wire["important_only"] = json!("yes");
    assert!(serde_json::from_value::<MemberBriefRequest>(wire).is_err());

    // Important mail needs a Gmail source: a calendar-only grant cannot carry it.
    let mut orphan = request(&thread, gmail, calendar);
    (orphan.gmail, orphan.important_only) = (None, true);
    assert!(matches!(
        s.create_member_brief(o, u.clone(), 1, orphan).await,
        Err(StoreError::InvalidInput)
    ));

    let mut important = request(&thread, gmail, calendar);
    important.important_only = true;
    let made = s
        .create_member_brief(o, u.clone(), 1, important.clone())
        .await
        .unwrap();
    assert!(made.important_only);
    // Same body replays; the same request id with the flag flipped is a different fingerprint.
    assert_eq!(
        s.create_member_brief(o, u.clone(), 1, important.clone())
            .await
            .unwrap()
            .revision,
        made.revision
    );
    let mut flipped = important;
    flipped.important_only = false;
    assert!(matches!(
        s.create_member_brief(o, u.clone(), 1, flipped).await,
        Err(StoreError::Conflict)
    ));

    let lease = s
        .claim_due_member_briefs(o, 1, made.next_due.unwrap() + 60)
        .await
        .unwrap()
        .remove(0);
    assert!(lease.important_only && lease.gmail.is_some());
    // Empty result is a valid, honest brief: zero important mail, still completed and counted as 0.
    let empty = MemberBriefOutcome::Evidence(MemberBriefEvidence {
        messages: Some(vec![]),
        events: Some(vec![]),
    });
    s.settle_member_brief(lease, empty).await.unwrap();
    let view = s
        .member_brief(o, u.clone(), 1, made.id.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        (view.runs[0].state.as_str(), view.runs[0].gmail_count),
        ("completed", Some(0))
    );
    assert!(view.important_only);

    // An ordinary grant stays ordinary.
    let plain = s
        .create_member_brief(o, u, 1, request(&thread, gmail, calendar))
        .await
        .unwrap();
    assert!(!plain.important_only);
}

#[tokio::test]
async fn slot_is_claimed_once_across_restart_and_evidence_is_minimal_encrypted_and_alerted_once() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, o, app) = setup().await;
    let gmail = connect(&s, &u, o, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&s, &u, o, app, MemberGoogleService::Calendar).await;
    let thread = s.create_thread(&u, "Morning".into()).await.unwrap().id;
    let made = s
        .create_member_brief(o, u.clone(), 1, request(&thread, gmail, calendar))
        .await
        .unwrap();
    let due = made.next_due.unwrap();
    assert!(
        s.claim_due_member_briefs(o, 1, due - 1)
            .await
            .unwrap()
            .is_empty()
    );
    // A gateway tick from an older host epoch cannot run (or claim) a newer grant's slot.
    assert!(
        s.claim_due_member_briefs(o, 0, due + 60)
            .await
            .unwrap()
            .is_empty()
    );

    let leases = s.claim_due_member_briefs(o, 1, due + 60).await.unwrap();
    assert_eq!(leases.len(), 1);
    assert_eq!(
        (leases[0].due_at, leases[0].thread_id.as_str()),
        (due, thread.as_str())
    );
    assert!(leases[0].gmail.is_some() && leases[0].calendar.is_some());
    // The claim is durable before any provider IO: nothing can claim the slot again.
    assert!(
        s.claim_due_member_briefs(o, 1, due + 61)
            .await
            .unwrap()
            .is_empty()
    );
    s.settle_member_brief(leases.into_iter().next().unwrap(), evidence())
        .await
        .unwrap();

    let view = s
        .member_brief(o, u.clone(), 1, made.id.parse().unwrap())
        .await
        .unwrap();
    let run = &view.runs[0];
    assert_eq!(
        (
            run.state.as_str(),
            run.freshness.as_deref(),
            run.gmail_count,
            run.calendar_count
        ),
        ("completed", Some("live"), Some(5), Some(1))
    );
    let detail = run.detail.as_ref().unwrap();
    assert_eq!(detail.messages.as_ref().unwrap().len(), 5);
    assert!(
        detail.messages.as_ref().unwrap()[0]
            .subject
            .starts_with(CANARY)
            && !detail.messages.as_ref().unwrap()[0]
                .subject
                .contains('\u{7}')
    );
    assert_eq!(detail.events.as_ref().unwrap()[0].title, TITLE);
    for blob in ciphers(&s).await {
        assert!(!blob.windows(CANARY.len()).any(|w| w == CANARY.as_bytes()));
        assert!(!blob.windows(TITLE.len()).any(|w| w == TITLE.as_bytes()));
    }
    let notices = "SELECT count(*) FROM local_notifications WHERE schedule_id=?1";
    assert_eq!(count(&s, notices, made.id.clone()).await, 1);
    let text: String = s
        .call(|db| {
            Ok(db
                .conn
                .query_row("SELECT title||message FROM local_notifications", [], |r| {
                    r.get(0)
                })?)
        })
        .await
        .unwrap();
    assert!(!text.contains(CANARY) && !text.contains(TITLE) && text.contains("메일 5건"));

    // Rewinding the schedule cannot re-run a slot that already has a ledger row, nor alert twice.
    let id = made.id.clone();
    s.call(move |db| {
        Ok(db.conn.execute(
            "UPDATE member_connected_briefs SET next_due=?2 WHERE id=?1",
            params![id, due],
        )?)
    })
    .await
    .unwrap();
    assert!(
        s.claim_due_member_briefs(o, 1, due + 120)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(count(&s, notices, made.id.clone()).await, 1);

    // Next day: claim, then "crash" before settling. Restart marks it unknown; it is never read again.
    let next = due + 86400;
    let leases = s.claim_due_member_briefs(o, 1, next + 60).await.unwrap();
    assert_eq!(leases.len(), 1);
    drop(leases);
    drop(s);
    let s = o.open_verified(dir.path()).await.unwrap();
    let view = s
        .member_brief(o, u.clone(), 1, made.id.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        (view.runs[0].due_at, view.runs[0].state.as_str()),
        (next, "unknown")
    );
    let id = made.id.clone();
    s.call(move |db| {
        Ok(db.conn.execute(
            "UPDATE member_connected_briefs SET next_due=?2 WHERE id=?1",
            params![id, next],
        )?)
    })
    .await
    .unwrap();
    assert!(
        s.claim_due_member_briefs(o, 1, next + 120)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(count(&s, notices, made.id).await, 1);
}

#[tokio::test]
async fn disconnect_withdraws_private_evidence_and_lost_credentials_give_guidance_not_a_brief() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let gmail = connect(&s, &u, o, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&s, &u, o, app, MemberGoogleService::Calendar).await;
    let thread = s.create_thread(&u, "Morning".into()).await.unwrap().id;
    let first = s
        .create_member_brief(o, u.clone(), 1, request(&thread, gmail, calendar))
        .await
        .unwrap();
    let due = first.next_due.unwrap();
    let lease = s
        .claim_due_member_briefs(o, 1, due + 60)
        .await
        .unwrap()
        .remove(0);
    s.settle_member_brief(lease, evidence()).await.unwrap();
    assert_eq!(ciphers(&s).await.len(), 1);

    s.disconnect_member_service(&u, o, MemberGoogleService::Calendar, calendar.0, calendar.1)
        .await
        .unwrap();
    let after = s
        .member_brief(o, u.clone(), 1, first.id.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        (after.state.as_str(), after.end_reason.as_deref()),
        ("revoked", Some("source_changed"))
    );
    assert_eq!(after.runs[0].state, "withdrawn");
    assert!(after.runs[0].detail.is_none() && ciphers(&s).await.is_empty());
    assert!(after.next_due.is_none());

    // Reconnect, grant again, then lose the Gmail credential without a disconnect (refresh failure).
    let calendar = connect(&s, &u, o, app, MemberGoogleService::Calendar).await;
    let second = s
        .create_member_brief(o, u.clone(), 1, request(&thread, gmail, calendar))
        .await
        .unwrap();
    s.call(|db| Ok(db.conn.execute("UPDATE member_google_grants SET status='needs_reauth',cipher=NULL,expires_at=NULL WHERE service='gmail'", [])?)).await.unwrap();
    let due = second.next_due.unwrap();
    assert!(
        s.claim_due_member_briefs(o, 1, due + 60)
            .await
            .unwrap()
            .is_empty()
    );
    let view = s
        .member_brief(o, u, 1, second.id.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        (
            view.runs[0].state.as_str(),
            view.runs[0].guidance.as_deref()
        ),
        ("needs_connection", Some("gmail"))
    );
    assert!(
        view.runs
            .iter()
            .all(|r| r.state != "completed" && r.detail.is_none())
    );
    assert_eq!(
        (view.state.as_str(), view.end_reason.as_deref()),
        ("revoked", Some("needs_connection"))
    );
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM local_notifications WHERE schedule_id=?1",
            second.id
        )
        .await,
        1
    );
}

#[tokio::test]
async fn final_slot_stays_active_so_it_can_be_stopped_and_then_withdraws_silently() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, owner, app) = setup().await;
    let gmail = connect(&store, &user, owner, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&store, &user, owner, app, MemberGoogleService::Calendar).await;
    let thread = store
        .create_thread(&user, "Morning".into())
        .await
        .unwrap()
        .id;
    let mut req = request(&thread, gmail, calendar);
    req.days = 1;
    let grant = store
        .create_member_brief(owner, user.clone(), 1, req)
        .await
        .unwrap();
    let due = grant.next_due.unwrap();
    // Claim the last still-valid slot: no further slot, yet the brief must still be active.
    let lease = store
        .claim_due_member_briefs(owner, 1, due)
        .await
        .unwrap()
        .remove(0);
    let id = grant.id.parse().unwrap();
    let mid = store
        .member_brief(owner, user.clone(), 1, id)
        .await
        .unwrap();
    assert_eq!(
        (mid.state.as_str(), mid.next_due),
        ("active", Some(mid.expires_at))
    );
    let stopped = store
        .stop_member_brief(
            owner,
            user.clone(),
            1,
            id,
            MemberBriefStop {
                request_id: Uuid::new_v4().to_string(),
                revision: mid.revision,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (stopped.state.as_str(), stopped.end_reason.as_deref()),
        ("stopped", Some("member"))
    );
    store.settle_member_brief(lease, evidence()).await.unwrap();
    let view = store.member_brief(owner, user, 1, id).await.unwrap();
    assert_eq!(view.runs[0].state, "withdrawn");
    assert!(view.runs[0].detail.is_none() && ciphers(&store).await.is_empty());
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM local_notifications WHERE schedule_id=?1",
            grant.id
        )
        .await,
        0
    );
}

#[tokio::test]
async fn epoch_fence_revokes_only_earlier_grants_and_blocks_settlement() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, owner, app) = setup().await;
    let gmail = connect(&store, &user, owner, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&store, &user, owner, app, MemberGoogleService::Calendar).await;
    let thread = store
        .create_thread(&user, "Morning".into())
        .await
        .unwrap()
        .id;
    let grant = store
        .create_member_brief(owner, user.clone(), 1, request(&thread, gmail, calendar))
        .await
        .unwrap();
    let lease = store
        .claim_due_member_briefs(owner, 1, grant.next_due.unwrap())
        .await
        .unwrap()
        .remove(0);
    let id = grant.id.parse().unwrap();
    // A late tick from an older epoch cannot revoke a newer grant.
    store.fence_member_brief_epoch(owner, 0).await.unwrap();
    store.fence_member_brief_epoch(owner, 1).await.unwrap();
    assert_eq!(
        store
            .member_brief(owner, user.clone(), 1, id)
            .await
            .unwrap()
            .state,
        "active"
    );
    // A newer authenticated tick revokes before the in-flight result is settled.
    store.fence_member_brief_epoch(owner, 2).await.unwrap();
    store.settle_member_brief(lease, evidence()).await.unwrap();
    let view = store.member_brief(owner, user, 1, id).await.unwrap();
    assert_eq!(
        (view.state.as_str(), view.end_reason.as_deref()),
        ("revoked", Some("session_changed"))
    );
    assert_eq!(view.runs[0].state, "withdrawn");
    assert!(ciphers(&store).await.is_empty());
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM local_notifications WHERE schedule_id=?1",
            grant.id
        )
        .await,
        0
    );
}

#[tokio::test]
async fn lost_refresh_settles_as_needs_reauth_and_never_reports_a_completed_brief() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, owner, app) = setup().await;
    let gmail = connect(&store, &user, owner, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&store, &user, owner, app, MemberGoogleService::Calendar).await;
    let thread = store
        .create_thread(&user, "Morning".into())
        .await
        .unwrap()
        .id;
    let grant = store
        .create_member_brief(owner, user.clone(), 1, request(&thread, gmail, calendar))
        .await
        .unwrap();
    let lease = store
        .claim_due_member_briefs(owner, 1, grant.next_due.unwrap())
        .await
        .unwrap()
        .remove(0);
    let source = lease.gmail.unwrap();
    let refresh = store
        .claim_member_brief_refresh(&lease, source)
        .await
        .unwrap();
    // While dispatched, nothing else can start another refresh.
    assert!(
        store
            .claim_member_brief_refresh(&lease, source)
            .await
            .is_err()
    );
    // The reply was lost / timed out: the bounded task settles `None`.
    assert!(
        store
            .settle_member_brief_refresh(refresh, None)
            .await
            .is_err()
    );
    let status: String = store
        .call(|db| {
            Ok(db.conn.query_row(
                "SELECT status FROM member_google_grants WHERE service='gmail'",
                [],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(status, "needs_reauth");
    assert!(store.member_brief_binding(&lease, source).await.is_err());
    store.settle_member_brief(lease, evidence()).await.unwrap();
    let view = store
        .member_brief(owner, user, 1, grant.id.parse().unwrap())
        .await
        .unwrap();
    assert!(
        view.runs
            .iter()
            .all(|r| r.state != "completed" && r.detail.is_none())
    );
}

#[tokio::test]
async fn reader_reauth_outcome_is_explained_in_results_not_completed() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, owner, app) = setup().await;
    let gmail = connect(&store, &user, owner, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&store, &user, owner, app, MemberGoogleService::Calendar).await;
    let thread = store
        .create_thread(&user, "Morning".into())
        .await
        .unwrap()
        .id;
    let grant = store
        .create_member_brief(owner, user.clone(), 1, request(&thread, gmail, calendar))
        .await
        .unwrap();
    let lease = store
        .claim_due_member_briefs(owner, 1, grant.next_due.unwrap())
        .await
        .unwrap()
        .remove(0);
    store
        .settle_member_brief(lease, MemberBriefOutcome::Unavailable("needs_reauth"))
        .await
        .unwrap();
    let view = store
        .member_brief(owner, user, 1, grant.id.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(
        (
            view.runs[0].state.as_str(),
            view.runs[0].guidance.as_deref()
        ),
        ("unavailable", Some("needs_reauth"))
    );
    let text: String = store
        .call(|db| {
            Ok(db
                .conn
                .query_row("SELECT title||message FROM local_notifications", [], |r| {
                    r.get(0)
                })?)
        })
        .await
        .unwrap();
    assert!(text.contains("다시 연결"));
}

#[tokio::test]
async fn stopping_a_claimed_slot_closes_the_credential_and_leaves_no_private_result() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, owner, app) = setup().await;
    let gmail = connect(&store, &user, owner, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&store, &user, owner, app, MemberGoogleService::Calendar).await;
    let thread = store
        .create_thread(&user, "Morning".into())
        .await
        .unwrap()
        .id;
    let grant = store
        .create_member_brief(owner, user.clone(), 1, request(&thread, gmail, calendar))
        .await
        .unwrap();
    let lease = store
        .claim_due_member_briefs(owner, 1, grant.next_due.unwrap())
        .await
        .unwrap()
        .remove(0);
    let source = lease.gmail.unwrap();
    let binding = store.member_brief_binding(&lease, source).await.unwrap();
    let id = grant.id.parse().unwrap();
    store
        .stop_member_brief(
            owner,
            user.clone(),
            1,
            id,
            MemberBriefStop {
                request_id: Uuid::new_v4().to_string(),
                revision: grant.revision,
            },
        )
        .await
        .unwrap();
    assert!(store.check_member_brief_binding(&binding).await.is_err());
    assert!(store.member_brief_binding(&lease, source).await.is_err());
    assert!(
        store
            .claim_member_brief_refresh(&lease, source)
            .await
            .is_err()
    );
    store.settle_member_brief(lease, evidence()).await.unwrap();
    let view = store.member_brief(owner, user, 1, id).await.unwrap();
    assert_eq!(view.runs[0].state, "withdrawn");
    assert!(view.runs[0].detail.is_none());
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM local_notifications WHERE schedule_id=?1",
            grant.id
        )
        .await,
        0
    );
}

#[tokio::test]
async fn reopening_after_host_change_or_restore_withdraws_the_previous_brief() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, owner, app) = setup().await;
    let gmail = connect(&store, &user, owner, app, MemberGoogleService::Gmail).await;
    let calendar = connect(&store, &user, owner, app, MemberGoogleService::Calendar).await;
    let thread = store
        .create_thread(&user, "Morning".into())
        .await
        .unwrap()
        .id;
    for (epoch, restore) in [(1, false), (2, true)] {
        let grant = store
            .create_member_brief(
                owner,
                user.clone(),
                epoch,
                request(&thread, gmail, calendar),
            )
            .await
            .unwrap();
        let lease = store
            .claim_due_member_briefs(owner, epoch, grant.next_due.unwrap())
            .await
            .unwrap()
            .remove(0);
        store.settle_member_brief(lease, evidence()).await.unwrap();
        assert!(!ciphers(&store).await.is_empty());
        if restore {
            store
                .call(|db| crate::schedules::quarantine_for_restore(&db.conn))
                .await
                .unwrap();
        }
        let view = store
            .member_brief(owner, user.clone(), epoch + 1, grant.id.parse().unwrap())
            .await
            .unwrap();
        assert_eq!(view.state, "revoked");
        assert!(view.runs[0].detail.is_none() && ciphers(&store).await.is_empty());
    }
}
