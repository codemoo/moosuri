use super::*;
use crate::tests::{TEST_LOCK, admin, member};
use std::os::unix::fs::PermissionsExt;
fn directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("moosuri-calendar-test-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}
#[tokio::test]
async fn calendar_schema26_late_failure_preserves_prior_schema_and_retries_atomically() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let thread = store
        .create_thread(&user, "Synthetic retained conversation".into())
        .await
        .unwrap();
    drop(store);
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch("DROP TABLE calendar_actions; DROP TABLE calendar_oauth_sessions; DROP TABLE calendar_connections; CREATE TABLE calendar_actions(conflict INTEGER); PRAGMA user_version=25;").unwrap();
    drop(conn);
    assert!(Store::open(dir.path()).is_err());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        25
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'calendar_%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('missions','mission_revisions','mission_controls','vault_grants','vault_handoffs','vault_operation_commitments')", [], |r| r.get::<_,i64>(0)).unwrap(),6);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM threads WHERE id=?1",
            [&thread.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    conn.execute_batch("DROP TABLE calendar_actions;").unwrap();
    drop(conn);
    drop(Store::open(dir.path()).unwrap());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        crate::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'calendar_%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM threads WHERE id=?1",
            [&thread.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}
fn app() -> GoogleAppPin {
    GoogleAppPin {
        revision: None,
        client_id: "synthetic-calendar.apps.googleusercontent.com".into(),
        redirect_uri: "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
    }
}
#[tokio::test]
async fn disconnect_retires_pending_and_claimed_oauth_even_after_journal_replay() {
    let _guard = TEST_LOCK.lock().await;
    for replay in [false, true] {
        let (dir, mut store, owner, connected) = fixture().await;
        let other = member(&store, &owner, "other-oauth@example.test")
            .await
            .user;
        let pending = store.start_calendar_oauth(&owner, app()).await.unwrap();
        let pending_handle = store
            .capture_calendar_oauth(pending.state, Some(token("synthetic-pending-code")))
            .await
            .unwrap()
            .unwrap();
        let claimed = store.start_calendar_oauth(&owner, app()).await.unwrap();
        let claimed_handle = store
            .capture_calendar_oauth(claimed.state, Some(token("synthetic-claimed-code")))
            .await
            .unwrap()
            .unwrap();
        let exchange = store
            .claim_calendar_oauth(&owner, claimed_handle, app())
            .await
            .unwrap();
        let other_flow = store.start_calendar_oauth(&other, app()).await.unwrap();
        let other_handle = store
            .capture_calendar_oauth(other_flow.state, Some(token("synthetic-other-code")))
            .await
            .unwrap()
            .unwrap();
        let other_exchange = store
            .claim_calendar_oauth(&other, other_handle, app())
            .await
            .unwrap();
        if replay {
            store.call(|db| {
                db.conn.execute_batch("CREATE TEMP TRIGGER fail_calendar_disconnect BEFORE UPDATE OF status ON calendar_connections WHEN NEW.status='disconnected' BEGIN SELECT RAISE(ABORT,'synthetic disconnect interruption'); END;")?;
                Ok(())
            }).await.unwrap();
            assert!(
                store
                    .disconnect_calendar(&owner, connected.id.clone())
                    .await
                    .is_err()
            );
            assert_eq!(store.initialized().await, Err(StoreError::Unavailable));
            drop(store);
            store = Store::open(dir.path()).unwrap();
        } else {
            store
                .disconnect_calendar(&owner, connected.id.clone())
                .await
                .unwrap();
        }
        assert_eq!(
            store.calendar_connections(&owner).await.unwrap()[0].status,
            "disconnected"
        );
        assert!(
            store
                .claim_calendar_oauth(&owner, pending_handle, app())
                .await
                .is_err()
        );
        assert!(
            store
                .complete_calendar_oauth(exchange, tokens(), primary())
                .await
                .is_err()
        );
        assert_eq!(
            store.calendar_connections(&owner).await.unwrap()[0].status,
            "disconnected"
        );
        let mut other_primary = primary();
        other_primary.id = "other-owner@example.test".into();
        let other_connected = store
            .complete_calendar_oauth(other_exchange, tokens(), other_primary)
            .await
            .unwrap();
        assert_eq!(other_connected.status, "active");
        let reconnected = connect(&store, &owner).await;
        assert_eq!(reconnected.id, connected.id);
        assert!(reconnected.epoch > connected.epoch);
        assert_eq!(reconnected.status, "active");
    }
}
fn token(v: &str) -> SecretString {
    SecretString::new(v.into()).unwrap()
}
fn tokens() -> TokenSet {
    TokenSet {
        access_token: token("synthetic-calendar-access"),
        refresh_token: Some(token("synthetic-calendar-refresh")),
        expires_in: 3600,
        scopes: Some(SCOPES.iter().map(|s| s.to_string()).collect()),
    }
}
fn primary() -> CalendarInfo {
    CalendarInfo {
        id: "owner@example.test".into(),
        title: "Synthetic primary".into(),
        time_zone: "Asia/Seoul".into(),
        primary: true,
        access_role: "owner".into(),
        email_notifications: false,
    }
}
fn fields() -> CalendarFields {
    CalendarFields {
        title: "Synthetic Calendar request".into(),
        description: "Synthetic only".into(),
        location: "Test room".into(),
        start: "2026-11-15T10:00:00+09:00".into(),
        end: "2026-11-15T11:00:00+09:00".into(),
        time_zone: Some("Asia/Seoul".into()),
    }
}
async fn connect(s: &Store, u: &UserContext) -> CalendarConnection {
    let start = s.start_calendar_oauth(u, app()).await.unwrap();
    let handle = s
        .capture_calendar_oauth(start.state, Some(token("synthetic-auth-code")))
        .await
        .unwrap()
        .unwrap();
    let e = s.claim_calendar_oauth(u, handle, app()).await.unwrap();
    s.complete_calendar_oauth(e, tokens(), primary())
        .await
        .unwrap()
}
async fn fixture() -> (tempfile::TempDir, Store, UserContext, CalendarConnection) {
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let c = connect(&s, &u).await;
    (d, s, u, c)
}
fn change(c: &CalendarConnection) -> CalendarChange {
    CalendarChange {
        request_id: Uuid::new_v4().to_string(),
        connection_id: c.id.clone(),
        expected_epoch: c.epoch,
        expected_revision: c.revision,
        mutation: CalendarMutation::Create { fields: fields() },
    }
}
fn event(d: &CalendarDispatch) -> CalendarEvent {
    CalendarEvent {
        id: d.event_id.clone(),
        etag: "\"provider-etag\"".into(),
        status: "confirmed".into(),
        fields: fields(),
        html_link: None,
        writable: true,
        all_day: false,
        updated: Some("2026-11-15T00:00:00Z".into()),
    }
}
async fn dispatch(
    s: &Store,
    u: &UserContext,
    c: &CalendarConnection,
    request: CalendarChange,
) -> CalendarDispatch {
    let b = s.calendar_binding(u, c.id.clone(), app()).await.unwrap();
    match s
        .admit_calendar_change(&b, request, primary(), None)
        .await
        .unwrap()
    {
        CalendarAdmission::Dispatch(d) => d,
        _ => panic!("new request must dispatch"),
    }
}
#[tokio::test]
async fn calendar_consent_is_separate_one_use_and_exact_original_session() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let b = member(&s, &u, "other-calendar@example.test").await.user;
    let flow = s.start_calendar_oauth(&u, app()).await.unwrap();
    assert!(flow.state.starts_with("cal."));
    let handle = s
        .capture_calendar_oauth(flow.state.clone(), Some(token("synthetic-auth-code")))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        s.capture_calendar_oauth(flow.state, Some(token("synthetic-replay")))
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        s.claim_calendar_oauth(&b, handle.clone(), app()).await,
        Err(StoreError::Conflict)
    ));
    let login = s
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(matches!(
        s.claim_calendar_oauth(&login, handle.clone(), app()).await,
        Err(StoreError::Conflict)
    ));
    let e = s
        .claim_calendar_oauth(&u, handle.clone(), app())
        .await
        .unwrap();
    let mut incomplete = tokens();
    incomplete.scopes = Some(vec![SCOPES[0].into()]);
    assert!(matches!(
        s.complete_calendar_oauth(e, incomplete, primary()).await,
        Err(StoreError::Forbidden)
    ));
    assert!(s.calendar_connections(&u).await.unwrap().is_empty());
    assert!(s.mail_connections(&u).await.unwrap().is_empty());
    assert!(matches!(
        s.claim_calendar_oauth(&u, handle, app()).await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn calendar_dispatch_exact_idempotence_and_conflicting_retry_do_not_resend() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = fixture().await;
    let req = change(&c);
    let d = dispatch(&s, &u, &c, req.clone()).await;
    let second = s
        .existing_calendar_change(&u, req.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.state, "dispatch_committed");
    assert_eq!(second.event_id, d.event_id);
    let mut different = req.clone();
    if let CalendarMutation::Create { fields } = &mut different.mutation {
        fields.title.push('!')
    }
    assert!(matches!(
        s.existing_calendar_change(&u, different).await,
        Err(StoreError::Conflict)
    ));
    let e = event(&d);
    let saved = s
        .settle_calendar_change(
            &d,
            "applied".into(),
            Some(e.clone()),
            "google_event_receipt".into(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(saved.provider_etag, e.etag.into());
    assert_eq!(saved.state, "applied");
    let repeat = s.existing_calendar_change(&u, req).await.unwrap().unwrap();
    assert_eq!(repeat.state, "applied");
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM calendar_actions", [], |r| r
                    .get::<_, i64>(0))?,
            1
        );
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn calendar_unknown_restart_only_lookup_and_absence_is_not_causal_delete() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, c) = fixture().await;
    let req = change(&c);
    let d = dispatch(&s, &u, &c, req.clone()).await;
    let id = d.change.request_id.clone();
    drop(d);
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    let history = s.calendar_actions(&u).await.unwrap();
    assert_eq!(history[0].state, "outcome_unknown");
    assert_eq!(
        s.existing_calendar_change(&u, req)
            .await
            .unwrap()
            .unwrap()
            .state,
        "outcome_unknown"
    );
    let d = s.calendar_reconcile(&u, id, app()).await.unwrap();
    assert!(d.event_id.starts_with('m'));
    let e = event(&d);
    s.settle_calendar_change(
        &d,
        "applied".into(),
        Some(e),
        "matching_operation_observed".into(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(s.calendar_actions(&u).await.unwrap()[0].state, "applied");
}
#[tokio::test]
async fn calendar_primary_role_etag_and_email_notification_rejections_precede_commit() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = fixture().await;
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    for p in [
        CalendarInfo {
            primary: false,
            ..primary()
        },
        CalendarInfo {
            access_role: "writer".into(),
            ..primary()
        },
        CalendarInfo {
            id: "foreign@example.test".into(),
            ..primary()
        },
        CalendarInfo {
            email_notifications: true,
            ..primary()
        },
    ] {
        assert!(matches!(
            s.admit_calendar_change(&b, change(&c), p, None).await,
            Err(StoreError::Conflict)
        ))
    }
    let mut req = change(&c);
    let prior = CalendarEvent {
        id: "synthetic1".into(),
        etag: "\"new\"".into(),
        status: "confirmed".into(),
        fields: fields(),
        html_link: None,
        writable: true,
        all_day: false,
        updated: None,
    };
    req.mutation = CalendarMutation::Delete {
        event_id: prior.id.clone(),
        etag: "\"old\"".into(),
    };
    assert!(matches!(
        s.admit_calendar_change(&b, req, primary(), Some(prior))
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(s.calendar_actions(&u).await.unwrap().is_empty());
}
#[tokio::test]
async fn calendar_account_epoch_disconnect_and_late_refresh_cannot_reactivate() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, c) = fixture().await;
    let other = member(&s, &u, "calendar-other@example.test").await.user;
    assert!(s.calendar_connections(&other).await.unwrap().is_empty());
    assert!(matches!(
        s.calendar_binding(&other, c.id.clone(), app()).await,
        Err(StoreError::NotFound)
    ));
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    let d = dispatch(&s, &u, &c, change(&c)).await;
    s.disconnect_calendar(&u, c.id.clone()).await.unwrap();
    assert!(s.refresh_calendar(&b, tokens()).await.is_err());
    assert!(s.check_calendar_binding(&b).await.is_err());
    assert!(
        s.settle_calendar_change(
            &d,
            "applied".into(),
            Some(event(&d)),
            "late_provider_receipt".into(),
            None
        )
        .await
        .is_err()
    );
    let history = s.calendar_actions(&u).await.unwrap();
    assert_eq!(history[0].state, "applied");
    assert!(history[0].event.is_none());
    assert_eq!(history[0].receipt.as_deref(), Some("late_provider_receipt"));
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    assert_eq!(
        s.calendar_connections(&u).await.unwrap()[0].status,
        "disconnected"
    );
    assert!(s.calendar_binding(&u, c.id, app()).await.is_err());
}
#[tokio::test]
async fn calendar_logout_before_commit_and_late_results_respect_real_session() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = fixture().await;
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    s.logout(&u).await.unwrap();
    assert!(matches!(
        s.admit_calendar_change(&b, change(&c), primary(), None)
            .await,
        Err(StoreError::Unauthorized)
    ));
    let u = s
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    let d = dispatch(&s, &u, &c, change(&c)).await;
    s.logout(&u).await.unwrap();
    assert!(
        s.settle_calendar_change(
            &d,
            "applied".into(),
            Some(event(&d)),
            "late_session_receipt".into(),
            None
        )
        .await
        .is_err()
    );
    let current = s
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert_eq!(
        s.calendar_actions(&current).await.unwrap()[0].state,
        "applied"
    );
    assert!(
        s.calendar_actions(&current).await.unwrap()[0]
            .event
            .is_none()
    );
}
#[tokio::test]
async fn calendar_refresh_cas_missing_refresh_retains_and_scope_union_is_rejected() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = fixture().await;
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    let mut refreshed = tokens();
    refreshed.refresh_token = None;
    refreshed.scopes = None;
    s.refresh_calendar(&b, refreshed).await.unwrap();
    assert!(s.refresh_calendar(&b, tokens()).await.is_err());
    let fresh = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    assert_eq!(fresh.refresh.expose(), "synthetic-calendar-refresh");
    let mut union = tokens();
    union
        .scopes
        .as_mut()
        .unwrap()
        .push("https://www.googleapis.com/auth/gmail.send".into());
    assert!(matches!(
        s.refresh_calendar(&fresh, union).await,
        Err(StoreError::Forbidden)
    ));
}
#[tokio::test]
async fn calendar_operator_app_change_oauth_and_epoch_fences_are_enforced() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, c) = fixture().await;
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    let flow = s.start_calendar_oauth(&u, app()).await.unwrap();
    let client = moosuri_connectors::gmail::OAuthClient {
        client_id: app().client_id,
        client_secret: token("synthetic-operator-secret"),
        redirect_uri: app().redirect_uri,
    };
    assert!(matches!(
        s.save_google_setup(&u, 0, client).await,
        Err(StoreError::GoogleAuthorizationInProgress)
    ));
    s.capture_calendar_oauth(flow.state, None).await.unwrap();
    let other = moosuri_connectors::gmail::OAuthClient {
        client_id: "different.apps.googleusercontent.com".into(),
        client_secret: token("synthetic-operator-secret"),
        redirect_uri: app().redirect_uri,
    };
    assert!(matches!(
        s.save_google_setup(&u, 0, other).await,
        Err(StoreError::GoogleConnectionsInUse)
    ));
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE accounts SET auth_epoch=auth_epoch+1 WHERE tenant_id=?1",
            [tenant],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.check_calendar_binding(&b).await.is_err());
    let u = s
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert_eq!(
        s.calendar_connections(&u).await.unwrap()[0].status,
        "needs_reconnect"
    );
    assert!(s.calendar_binding(&u, c.id, app()).await.is_err());
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    assert_eq!(
        s.calendar_connections(&u).await.unwrap()[0].status,
        "needs_reconnect"
    );
}
#[tokio::test]
async fn calendar_actual_encrypted_restore_disables_credentials_and_never_replays_unknown_write() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, c) = fixture().await;
    let d = dispatch(&s, &u, &c, change(&c)).await;
    let id = d.change.request_id.clone();
    let account = u.account.id.clone();
    drop(d);
    drop(s);
    let kit = directory();
    let heads = directory();
    let destination = directory();
    let identity = kit.path().join("identity");
    let recipient = crate::backup::keygen(&identity).unwrap();
    crate::backup::setup(dir.path(), &recipient, &identity).unwrap();
    let cipher = kit.path().join("calendar.age");
    let head = heads.path().join("head.json");
    crate::cutover::checkpoint(dir.path(), &cipher, &head).unwrap();
    let restored = destination.path().join("restored");
    crate::cutover::restore(&cipher, &head, &identity, &restored).unwrap();
    let s = Store::open(&restored).unwrap();
    s.call(|db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM calendar_connections WHERE credential IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM calendar_oauth_sessions", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM calendar_actions", [], |r| r
                    .get::<_, String>(0))?,
            "quarantined"
        );
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    crate::recover_account(
        &restored,
        account,
        Role::Admin,
        "Synthetic-restored-calendar-temporary".into(),
    )
    .await
    .unwrap();
    let s = Store::open(&restored).unwrap();
    let login = s
        .login(
            "admin@example.test".into(),
            "Synthetic-restored-calendar-temporary".into(),
        )
        .await
        .unwrap();
    let u = s
        .change_password(&login.user, "Synthetic-restored-calendar-final".into())
        .await
        .unwrap()
        .user;
    assert_eq!(
        s.calendar_actions(&u).await.unwrap()[0].state,
        "quarantined"
    );
    assert!(s.calendar_reconcile(&u, id, app()).await.is_err());
    assert!(s.calendar_binding(&u, c.id, app()).await.is_err());
}
#[tokio::test]
async fn calendar_reconcile_after_legitimate_refresh_uses_current_read_authority_not_write_replay()
{
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = fixture().await;
    let d = dispatch(&s, &u, &c, change(&c)).await;
    s.settle_calendar_change(
        &d,
        "outcome_unknown".into(),
        None,
        "synthetic_ack_loss".into(),
        None,
    )
    .await
    .unwrap();
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    s.refresh_calendar(&b, tokens()).await.unwrap();
    let reconciled = s
        .calendar_reconcile(&u, d.change.request_id.clone(), app())
        .await
        .unwrap();
    assert_eq!(reconciled.change.expected_revision, 1);
    assert_eq!(reconciled.settlement_revision, 2);
    let outcome = s
        .settle_calendar_change(
            &reconciled,
            "applied".into(),
            Some(event(&reconciled)),
            "exact_event_lookup".into(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(outcome.state, "applied");
    assert!(outcome.event.is_some());
}
#[tokio::test]
async fn calendar_missing_http_callback_expires_to_lookup_without_restarting_or_resend() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, c) = fixture().await;
    let request = change(&c);
    let d = dispatch(&s, &u, &c, request.clone()).await;
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE calendar_actions SET created_at=?2 WHERE tenant_id=?1",
            params![tenant, now()? - 100],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let actions = s.calendar_actions(&u).await.unwrap();
    assert_eq!(actions[0].state, "outcome_unknown");
    assert_eq!(
        actions[0].error.as_deref(),
        Some("dispatch_receipt_missing")
    );
    let lookup = s
        .calendar_reconcile(&u, d.change.request_id, app())
        .await
        .unwrap();
    assert_eq!(lookup.event_id, actions[0].event_id);
    assert!(
        matches!(s.existing_calendar_change(&u,request).await.unwrap(),Some(v)if v.state=="outcome_unknown")
    );
}
#[tokio::test]
async fn calendar_stale_pre_admission_revision_can_renew_without_duplicate_dispatch() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, c) = fixture().await;
    let original = change(&c);
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    s.refresh_calendar(&b, tokens()).await.unwrap();
    let fresh = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    assert!(matches!(
        s.admit_calendar_change(&fresh, original.clone(), primary(), None)
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(s.calendar_actions(&u).await.unwrap().is_empty());
    let mut renewed = original;
    renewed.request_id = Uuid::new_v4().to_string();
    renewed.expected_revision = fresh.connection.revision;
    assert!(matches!(
        s.admit_calendar_change(&fresh, renewed.clone(), primary(), None)
            .await
            .unwrap(),
        CalendarAdmission::Dispatch(_)
    ));
    assert!(
        matches!(s.existing_calendar_change(&u,renewed).await.unwrap(),Some(v)if v.state=="dispatch_committed")
    );
    assert_eq!(s.calendar_actions(&u).await.unwrap().len(), 1);
}

#[tokio::test]
async fn independent_pending_oauth_cannot_undo_disconnect() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, c) = fixture().await;
    let flow = s.start_calendar_oauth(&u, app()).await.unwrap();
    let handle = s
        .capture_calendar_oauth(flow.state, Some(token("synthetic-pending-code")))
        .await
        .unwrap()
        .unwrap();
    let e = s.claim_calendar_oauth(&u, handle, app()).await.unwrap();
    s.disconnect_calendar(&u, c.id.clone()).await.unwrap();
    assert_eq!(
        s.calendar_connections(&u).await.unwrap()[0].status,
        "disconnected"
    );
    let late = s.complete_calendar_oauth(e, tokens(), primary()).await;
    assert!(
        late.is_err(),
        "late OAuth completed after disconnect: {}",
        late.as_ref()
            .map(|c| c.status.as_str())
            .unwrap_or("refused")
    );
    assert_eq!(
        s.calendar_connections(&u).await.unwrap()[0].status,
        "disconnected"
    );
}
