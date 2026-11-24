use super::*;
use crate::tests::{TEST_LOCK, directory};
use serde_json::{Value, json};

const PASSWORD: &str = "Synthetic-calendar-password-42";
const CHANGED: &str = "Changed-calendar-password-42";
const CANARY: &str = "CALENDAR-TITLE-CANARY";
const ACCESS: &str = "SYNTHETIC-MEMBER-CALENDAR-ACCESS-CANARY";
const REFRESH: &str = "SYNTHETIC-MEMBER-CALENDAR-REFRESH-CANARY";

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
            "Calendar member".into(),
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
fn receipt(o: VolumeOwner, app: Uuid, purpose: &str, expires: i64) -> String {
    let payload = json!({"v":1,"owner":wire_owner(o),"app_revision":app,"purpose":purpose,"subject":"synthetic-subject","mailbox":"member@example.test","token_tag":URL_SAFE_NO_PAD.encode([7u8;32]),"expires_at":expires});
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap()),
        URL_SAFE_NO_PAD.encode([8u8; 32])
    )
}
async fn connect(s: &Store, u: &UserContext, o: VolumeOwner, app: Uuid) -> MemberGoogleBinding {
    let start = s
        .start_member_service_oauth(u, o, Service::Calendar, app)
        .await
        .unwrap();
    let handle = s
        .capture_member_service_oauth(
            Service::Calendar,
            start.state,
            Some(SecretString::new("synthetic-code".into()).unwrap()),
        )
        .await
        .unwrap();
    let e = s
        .claim_member_service_oauth(u, o, Service::Calendar, app, handle.unwrap())
        .await
        .unwrap();
    let id = e.member_google_request_id().unwrap();
    let t = now().unwrap();
    let reply = json!({"v":1,"request_id":id,"owner":wire_owner(o),"app_revision":app,"state":"ok","result":{"access_token":ACCESS,"refresh_token":REFRESH,"expires_in":3600,"scopes":[Service::Calendar.scope()],"mailbox":"member@example.test","access_binding":receipt(o,app,"calendar:access",t+3600),"refresh_binding":receipt(o,app,"calendar:refresh",t+86400)}});
    let grant = MemberGoogleGrant::from_broker_reply(
        id,
        Zeroizing::new(serde_json::to_vec(&reply).unwrap()),
    )
    .unwrap();
    s.complete_member_google_oauth(e, grant).await.unwrap();
    s.member_service_read_binding(u, o, Service::Calendar, app)
        .await
        .unwrap()
}
fn fields(title: &str, start: &str, end: &str) -> CalendarFields {
    CalendarFields {
        title: title.into(),
        description: String::new(),
        location: String::new(),
        start: start.into(),
        end: end.into(),
        time_zone: Some("Asia/Seoul".into()),
    }
}
fn create(request: Uuid) -> MemberCalendarIntent {
    MemberCalendarIntent {
        request_id: request,
        kind: Kind::Create,
        event_id: stable_event_id(&request.to_string()),
        before_etag: None,
        fields: fields(
            CANARY,
            "2026-11-26T15:00:00+09:00",
            "2026-11-26T16:00:00+09:00",
        ),
    }
}
fn mv(request: Uuid, etag: &str) -> MemberCalendarIntent {
    MemberCalendarIntent {
        request_id: request,
        kind: Kind::Move,
        event_id: "evt1aaaaaa".into(),
        before_etag: Some(etag.into()),
        fields: fields(
            CANARY,
            "2026-11-27T15:00:00+09:00",
            "2026-11-27T16:00:00+09:00",
        ),
    }
}
fn observed(intent: &MemberCalendarIntent) -> CalendarEvent {
    CalendarEvent {
        id: intent.event_id.clone(),
        etag: "\"v2\"".into(),
        status: "confirmed".into(),
        fields: intent.fields.clone(),
        html_link: None,
        writable: true,
        all_day: false,
        updated: None,
    }
}
fn fresh(a: MemberCalendarAdmission) -> MemberCalendarLease {
    match a {
        MemberCalendarAdmission::Fresh(l) => l,
        MemberCalendarAdmission::Existing(_) => panic!("expected a fresh dispatch"),
    }
}

#[tokio::test]
async fn create_commits_dispatch_first_then_provider_receipt_and_content_is_encrypted() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let b = connect(&s, &u, o, app).await;
    let request = Uuid::new_v4();
    let intent = create(request);
    let lease = fresh(
        s.begin_member_calendar_action(&b, intent.clone(), None)
            .await
            .unwrap(),
    );
    // Dispatch is durable before any provider IO could have happened.
    let before = s
        .member_calendar_record(&u, o, request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.state, "dispatch_committed");
    assert!(before.confirmed_by.is_none() && before.event.is_none());
    let done = s
        .settle_member_calendar_action(
            lease,
            MemberCalendarSettle::Applied {
                event: Box::new(observed(&intent)),
                by: "provider_response",
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (
            done.state.as_str(),
            done.confirmed_by.as_deref(),
            done.receipt.as_deref()
        ),
        ("applied", Some("provider_response"), Some("created"))
    );
    assert_eq!(done.event.unwrap().fields.title, CANARY);
    let raw: Vec<(Vec<u8>, Option<Vec<u8>>)> = s
        .call(|db| {
            let mut q = db
                .conn
                .prepare("SELECT ifnull(payload,x''),event FROM member_calendar_actions")?;
            let rows = q
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
        .unwrap();
    for (payload, event) in raw {
        assert!(
            !payload
                .windows(CANARY.len())
                .any(|w| w == CANARY.as_bytes())
        );
        assert!(
            !event
                .unwrap()
                .windows(CANARY.len())
                .any(|w| w == CANARY.as_bytes())
        );
    }
}

#[tokio::test]
async fn duplicate_request_never_redispatches_and_changed_content_is_a_conflict() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let b = connect(&s, &u, o, app).await;
    let request = Uuid::new_v4();
    let intent = create(request);
    let lease = fresh(
        s.begin_member_calendar_action(&b, intent.clone(), None)
            .await
            .unwrap(),
    );
    // Retry while the first is in flight (or its reply was lost): Existing, never Fresh.
    match s
        .begin_member_calendar_action(&b, intent.clone(), None)
        .await
        .unwrap()
    {
        MemberCalendarAdmission::Existing(r) => assert_eq!(r.state, "dispatch_committed"),
        MemberCalendarAdmission::Fresh(_) => panic!("second dispatch"),
    }
    s.settle_member_calendar_action(
        lease,
        MemberCalendarSettle::Applied {
            event: Box::new(observed(&intent)),
            by: "provider_response",
        },
    )
    .await
    .unwrap();
    match s
        .begin_member_calendar_action(&b, intent.clone(), None)
        .await
        .unwrap()
    {
        MemberCalendarAdmission::Existing(r) => assert_eq!(r.state, "applied"),
        MemberCalendarAdmission::Fresh(_) => panic!("second dispatch"),
    }
    let mut changed = intent;
    changed.fields.start = "2026-11-26T17:00:00+09:00".into();
    changed.fields.end = "2026-11-26T18:00:00+09:00".into();
    assert!(matches!(
        s.begin_member_calendar_action(&b, changed, None).await,
        Err(StoreError::Conflict)
    ));
    // A create must use the stable provider id derived from its own request id.
    let mut forged = create(Uuid::new_v4());
    forged.event_id = stable_event_id("00000000-0000-4000-8000-000000000000");
    assert!(matches!(
        s.begin_member_calendar_action(&b, forged, None).await,
        Err(StoreError::InvalidInput)
    ));
}

#[tokio::test]
async fn lost_acknowledgement_reconciles_by_readback_without_a_second_dispatch() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let b = connect(&s, &u, o, app).await;
    let request = Uuid::new_v4();
    let intent = create(request);
    let lease = fresh(
        s.begin_member_calendar_action(&b, intent.clone(), None)
            .await
            .unwrap(),
    );
    let unknown = s
        .settle_member_calendar_action(lease, MemberCalendarSettle::Unknown("ack_lost"))
        .await
        .unwrap();
    assert_eq!(unknown.state, "outcome_unknown");
    assert!(unknown.confirmed_by.is_none());
    // Unknown is not "applied": a mere retry is Existing and re-sends nothing.
    assert!(matches!(
        s.begin_member_calendar_action(&b, intent.clone(), None)
            .await
            .unwrap(),
        MemberCalendarAdmission::Existing(_)
    ));
    let lease = s
        .claim_member_calendar_reconcile(&b, request)
        .await
        .unwrap();
    assert_eq!(lease.intent.event_id, intent.event_id);
    // First readback finds nothing: stays unknown (no causal-delete claim, no rewrite).
    let still = s
        .settle_member_calendar_action(lease, MemberCalendarSettle::Unknown("absent_on_readback"))
        .await
        .unwrap();
    assert_eq!(still.state, "outcome_unknown");
    let lease = s
        .claim_member_calendar_reconcile(&b, request)
        .await
        .unwrap();
    let done = s
        .settle_member_calendar_action(
            lease,
            MemberCalendarSettle::Applied {
                event: Box::new(observed(&intent)),
                by: "readback",
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (done.state.as_str(), done.confirmed_by.as_deref()),
        ("applied", Some("readback"))
    );
    // Applied rows are not reconcilable again.
    assert!(matches!(
        s.claim_member_calendar_reconcile(&b, request).await,
        Err(StoreError::Conflict)
    ));
}

#[tokio::test]
async fn stale_version_is_a_definite_conflict_and_the_handle_is_single_use() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let b = connect(&s, &u, o, app).await;
    let refs = s
        .member_calendar_put_candidates(
            &b,
            vec![
                ("evt1aaaaaa".into(), "\"e1\"".into()),
                ("evt2bbbbbb".into(), "\"e2\"".into()),
            ],
        )
        .await
        .unwrap();
    assert_eq!(refs.len(), 2);
    let first = Uuid::new_v4();
    assert_eq!(
        s.member_calendar_candidate(&b, refs[0].clone(), first)
            .await
            .unwrap(),
        ("evt1aaaaaa".into(), "\"e1\"".into())
    );
    // The ETag seen at listing time must be the one in the approved intent.
    assert!(matches!(
        s.begin_member_calendar_action(&b, mv(first, "\"other\""), Some(refs[0].clone()))
            .await,
        Err(StoreError::Conflict)
    ));
    let lease = fresh(
        s.begin_member_calendar_action(&b, mv(first, "\"e1\""), Some(refs[0].clone()))
            .await
            .unwrap(),
    );
    let conflict = s
        .settle_member_calendar_action(lease, MemberCalendarSettle::Conflict)
        .await
        .unwrap();
    assert_eq!(
        (conflict.state.as_str(), conflict.error.as_deref()),
        ("conflict", Some("stale_etag"))
    );
    // A different request cannot reuse a consumed handle; the same request can retry.
    assert!(
        s.member_calendar_candidate(&b, refs[0].clone(), Uuid::new_v4())
            .await
            .is_err()
    );
    assert!(
        s.member_calendar_candidate(&b, refs[0].clone(), first)
            .await
            .is_ok()
    );
    // Expiry.
    s.call(|db| {
        db.conn
            .execute("UPDATE member_calendar_candidates SET expires_at=1", [])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.member_calendar_candidate(&b, refs[1].clone(), Uuid::new_v4())
            .await
            .is_err()
    );
    // Titles are not stored with handles.
    let cols: i64 = s
        .call(|db| Ok(db.conn.query_row("SELECT count(*) FROM pragma_table_info('member_calendar_candidates') WHERE name IN('title','summary','start','end')", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(cols, 0);
}

#[tokio::test]
async fn disconnect_or_reconnect_withdraws_handles_and_a_late_result_is_not_stored() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let b = connect(&s, &u, o, app).await;
    let refs = s
        .member_calendar_put_candidates(&b, vec![("evt1aaaaaa".into(), "\"e1\"".into())])
        .await
        .unwrap();
    let request = Uuid::new_v4();
    let intent = create(request);
    let lease = fresh(
        s.begin_member_calendar_action(&b, intent.clone(), None)
            .await
            .unwrap(),
    );
    s.disconnect_member_service(&u, o, Service::Calendar, b.connection_id, b.epoch)
        .await
        .unwrap();
    let late = s
        .settle_member_calendar_action(
            lease,
            MemberCalendarSettle::Applied {
                event: Box::new(observed(&intent)),
                by: "provider_response",
            },
        )
        .await
        .unwrap();
    assert_eq!(late.state, "withdrawn");
    assert!(late.event.is_none() && late.confirmed_by.is_none());
    assert!(
        s.member_calendar_record(&u, o, request)
            .await
            .unwrap()
            .unwrap()
            .event
            .is_none()
    );
    // The old binding can no longer admit anything or resolve a handle.
    assert!(
        s.member_calendar_candidate(&b, refs[0].clone(), Uuid::new_v4())
            .await
            .is_err()
    );
    assert!(
        s.begin_member_calendar_action(&b, create(Uuid::new_v4()), None)
            .await
            .is_err()
    );
    // Reconnect creates a new generation: the old binding stays dead.
    let again = connect(&s, &u, o, app).await;
    assert!(again.epoch > b.epoch);
    assert!(
        s.begin_member_calendar_action(&b, create(Uuid::new_v4()), None)
            .await
            .is_err()
    );
    assert!(
        s.begin_member_calendar_action(&again, create(Uuid::new_v4()), None)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn other_member_cannot_see_or_claim_a_request_and_restart_makes_dispatch_unknown() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let b = connect(&s, &u, o, app).await;
    let request = Uuid::new_v4();
    let _lease = fresh(
        s.begin_member_calendar_action(&b, create(request), None)
            .await
            .unwrap(),
    );
    // A different owner identity is rejected by the volume authority.
    let stranger = VolumeOwner {
        account: Uuid::new_v4(),
        ..o
    };
    assert!(
        s.member_calendar_record(&u, stranger, request)
            .await
            .is_err()
    );
    // A new session (same member) is not the original session: no cross-session replay.
    let other = s
        .login("member@example.test".into(), CHANGED.into())
        .await
        .unwrap()
        .user;
    assert!(s.member_calendar_record(&other, o, request).await.is_err());
    s.call(|db| {
        restart(&db.conn)?;
        Ok(())
    })
    .await
    .unwrap();
    let r = s
        .member_calendar_record(&u, o, request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (r.state.as_str(), r.error.as_deref()),
        ("outcome_unknown", Some("restart"))
    );
}

async fn age(s: &Store, request: Uuid) {
    s.call(move |db| {
        db.conn.execute(
            "UPDATE member_calendar_actions SET updated_at=unixepoch()-1000 WHERE request_id=?1",
            [request.to_string()],
        )?;
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn aged_unsettled_dispatch_becomes_readback_only_without_a_restart() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let b = connect(&s, &u, o, app).await;
    let (old, young) = (Uuid::new_v4(), Uuid::new_v4());
    let (old_intent, young_intent) = (create(old), create(young));
    let stale = fresh(
        s.begin_member_calendar_action(&b, old_intent.clone(), None)
            .await
            .unwrap(),
    );
    let _young = fresh(
        s.begin_member_calendar_action(&b, young_intent.clone(), None)
            .await
            .unwrap(),
    );
    // Inside the grace window the original lease still owns the request.
    assert!(matches!(
        s.claim_member_calendar_reconcile(&b, old).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        s.member_calendar_record(&u, o, old)
            .await
            .unwrap()
            .unwrap()
            .state,
        "dispatch_committed"
    );
    age(&s, old).await;
    // Replaying the admitted request after the grace window never dispatches again.
    match s
        .begin_member_calendar_action(&b, old_intent.clone(), None)
        .await
        .unwrap()
    {
        MemberCalendarAdmission::Existing(r) => assert_eq!(
            (r.state.as_str(), r.error.as_deref()),
            ("outcome_unknown", Some("dispatch_aged"))
        ),
        MemberCalendarAdmission::Fresh(_) => panic!("aged dispatch must not become a new write"),
    }
    let lease = s.claim_member_calendar_reconcile(&b, old).await.unwrap();
    assert_eq!(lease.intent.event_id, old_intent.event_id);
    // The serial version moved on: the original attempt cannot settle over the readback.
    assert!(matches!(
        s.settle_member_calendar_action(
            stale,
            MemberCalendarSettle::Applied {
                event: Box::new(observed(&old_intent)),
                by: "provider_response"
            }
        )
        .await,
        Err(StoreError::Conflict)
    ));
    // A young request in the same account is untouched (no collateral).
    assert_eq!(
        s.member_calendar_record(&u, o, young)
            .await
            .unwrap()
            .unwrap()
            .state,
        "dispatch_committed"
    );
    assert!(matches!(
        s.claim_member_calendar_reconcile(&b, young).await,
        Err(StoreError::Conflict)
    ));
    let done = s
        .settle_member_calendar_action(
            lease,
            MemberCalendarSettle::Applied {
                event: Box::new(observed(&old_intent)),
                by: "readback",
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (done.state.as_str(), done.confirmed_by.as_deref()),
        ("applied", Some("readback"))
    );
    let rows: i64 = s
        .call(|db| {
            Ok(db
                .conn
                .query_row("SELECT count(*) FROM member_calendar_actions", [], |r| {
                    r.get(0)
                })?)
        })
        .await
        .unwrap();
    assert_eq!(rows, 2);
}

#[tokio::test]
async fn aged_dispatch_is_not_readable_after_the_grant_generation_is_lost() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, o, app) = setup().await;
    let b = connect(&s, &u, o, app).await;
    let request = Uuid::new_v4();
    let intent = create(request);
    let _lease = fresh(
        s.begin_member_calendar_action(&b, intent.clone(), None)
            .await
            .unwrap(),
    );
    age(&s, request).await;
    s.disconnect_member_service(&u, o, Service::Calendar, b.connection_id, b.epoch)
        .await
        .unwrap();
    // Reconnect makes a new epoch; the aged request must not be adopted by it.
    let again = connect(&s, &u, o, app).await;
    assert!(
        s.claim_member_calendar_reconcile(&again, request)
            .await
            .is_err()
    );
    assert!(
        s.claim_member_calendar_reconcile(&b, request)
            .await
            .is_err()
    );
    let r = s
        .member_calendar_record(&u, o, request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(r.state, "withdrawn");
    let payload: Option<Vec<u8>> = s
        .call(|db| {
            Ok(db
                .conn
                .query_row("SELECT payload FROM member_calendar_actions", [], |r| {
                    r.get(0)
                })?)
        })
        .await
        .unwrap();
    assert!(payload.is_none());
}

#[test]
fn windows_use_the_requested_zone_and_refuse_dst_gaps_and_folds() {
    let (a, b) = member_calendar_window("2026-11-26", "15:00", "Asia/Seoul", 90).unwrap();
    assert_eq!(
        (a.as_str(), b.as_str()),
        ("2026-11-26T15:00:00+09:00", "2026-11-26T16:30:00+09:00")
    );
    let (a, _) = member_calendar_window("2026-11-26", "15:00", "America/New_York", 30).unwrap();
    assert_eq!(a, "2026-11-26T15:00:00-05:00");
    assert!(
        member_calendar_window("2026-03-08", "02:30", "America/New_York", 30).is_err(),
        "gap"
    );
    assert!(
        member_calendar_window("2026-11-01", "01:30", "America/New_York", 30).is_err(),
        "fold"
    );
    assert!(member_calendar_window("2026-11-26", "15:00", "Not/AZone", 30).is_err());
    assert!(member_calendar_window("2026-11-26", "15:00", "Asia/Seoul", 0).is_err());
    let (a, b) = member_calendar_move_window(Some("2026-11-26"), "Asia/Seoul", 0).unwrap();
    assert_eq!(
        (a.as_str(), b.as_str()),
        ("2026-11-26T00:00:00+09:00", "2026-11-27T00:00:00+09:00")
    );
    assert_eq!(
        member_calendar_minutes("2026-11-26T10:00:00+09:00", "2026-11-26T11:30:00+09:00").unwrap(),
        90
    );
    assert!(
        member_calendar_title_matches(" 팀 회의 ", "팀 회의")
            && !member_calendar_title_matches("팀 회의", "팀 회의 2")
    );
}
