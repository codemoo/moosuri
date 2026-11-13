use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_connectors::gmail::TokenSet;
fn client(id: &str) -> OAuthClient {
    OAuthClient {
        client_id: id.into(),
        client_secret: SecretString::new("SYNTHETIC-APP-SECRET-CANARY".into()).unwrap(),
        redirect_uri: "http://localhost:3000/api/mail/oauth/callback".into(),
    }
}
fn tokens() -> TokenSet {
    TokenSet {
        access_token: SecretString::new("synthetic-access".into()).unwrap(),
        refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
        expires_in: 3600,
        scopes: None,
    }
}
async fn app(store: &Store) -> GoogleApp {
    store
        .google_app(client("a").redirect_uri)
        .await
        .unwrap()
        .unwrap()
}
async fn connect(
    store: &Store,
    user: &UserContext,
    pin: GoogleAppPin,
) -> moosuri_core::MailConnection {
    let start = store
        .start_mail_oauth_with_app(user, pin.clone(), true)
        .await
        .unwrap();
    let (handle, setup) = store
        .capture_mail_oauth_with_return(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    assert!(setup);
    let exchange = store
        .claim_mail_oauth_with_app(user, handle, pin)
        .await
        .unwrap();
    store
        .complete_mail_oauth(exchange, tokens(), "sender@example.test".into())
        .await
        .unwrap()
}
#[tokio::test]
async fn admin_cas_secret_restart_tamper_and_origin() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "member@example.test").await;
    assert!(matches!(
        store.google_setup(&b.user).await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store.save_google_setup(&b.user, 0, client("a")).await,
        Err(StoreError::Forbidden)
    ));
    assert_eq!(
        store
            .save_google_setup(&a.user, 0, client("a"))
            .await
            .unwrap()
            .revision,
        1
    );
    assert!(matches!(
        store.save_google_setup(&a.user, 0, client("a")).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .google_app("https://different.example/callback".into())
            .await,
        Err(StoreError::Conflict)
    ));
    store
        .call(|db| {
            let cipher: Vec<u8> =
                db.conn
                    .query_row("SELECT secret FROM google_setup", [], |r| r.get(0))?;
            assert!(!String::from_utf8_lossy(&cipher).contains("SYNTHETIC-APP-SECRET-CANARY"));
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(
        app(&store).await.client.client_secret.expose(),
        "SYNTHETIC-APP-SECRET-CANARY"
    );
    store
        .call(|db| {
            db.conn
                .execute("UPDATE google_setup SET revision=revision+1", [])?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store.google_app(client("a").redirect_uri).await,
        Err(StoreError::Corrupt)
    ));
}
#[tokio::test]
async fn all_pending_stages_fence_and_failure_cleanup_allows_rotation() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    store
        .save_google_setup(&a.user, 0, client("a"))
        .await
        .unwrap();
    let pin = app(&store).await.pin;
    let start = store
        .start_mail_oauth_with_app(&a.user, pin.clone(), true)
        .await
        .unwrap();
    assert!(matches!(
        store.save_google_setup(&a.user, 1, client("a")).await,
        Err(StoreError::GoogleAuthorizationInProgress)
    ));
    let handle = store
        .capture_mail_oauth(start.state, SecretString::new("code".into()).unwrap())
        .await
        .unwrap();
    assert!(matches!(
        store.save_google_setup(&a.user, 1, client("a")).await,
        Err(StoreError::GoogleAuthorizationInProgress)
    ));
    let exchange = store
        .claim_mail_oauth_with_app(&a.user, handle, pin.clone())
        .await
        .unwrap();
    assert!(matches!(
        store.save_google_setup(&a.user, 1, client("a")).await,
        Err(StoreError::GoogleAuthorizationInProgress)
    ));
    store.cancel_mail_oauth_exchange(&exchange).await.unwrap();
    store
        .save_google_setup(&a.user, 1, client("a"))
        .await
        .unwrap();
    assert!(matches!(
        store.start_mail_oauth_with_app(&a.user, pin, false).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .complete_mail_oauth(exchange, tokens(), "a@example.test".into())
            .await,
        Err(StoreError::Conflict)
    ));
    let start = store
        .start_mail_oauth_with_app(&a.user, app(&store).await.pin, true)
        .await
        .unwrap();
    assert!(store.cancel_mail_oauth_state(start.state).await.unwrap());
    store
        .save_google_setup(&a.user, 2, client("a"))
        .await
        .unwrap();
}
#[tokio::test]
async fn client_switch_and_stale_refresh_or_error_are_fenced() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    store
        .save_google_setup(&a.user, 0, client("a"))
        .await
        .unwrap();
    let pin = app(&store).await.pin;
    let c = connect(&store, &a.user, pin.clone()).await;
    let binding = store
        .pin_mail_binding(
            store.mail_binding(&a.user, c.id.clone()).await.unwrap(),
            pin,
        )
        .await
        .unwrap();
    assert!(matches!(
        store.save_google_setup(&a.user, 1, client("b")).await,
        Err(StoreError::GoogleConnectionsInUse)
    ));
    let mut changed_origin = client("a");
    changed_origin.redirect_uri = "https://new.example/api/mail/oauth/callback".into();
    assert!(matches!(
        store.save_google_setup(&a.user, 1, changed_origin).await,
        Err(StoreError::GoogleConnectionsInUse)
    ));
    store
        .save_google_setup(&a.user, 1, client("a"))
        .await
        .unwrap();
    assert!(matches!(
        store.save_mail_refresh(&binding, tokens()).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store.mark_mail_needs_reconnect(&binding).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store.mail_connections(&a.user).await.unwrap()[0].status,
        "active"
    );
    store.disconnect_mail(&a.user, c.id).await.unwrap();
    store
        .save_google_setup(&a.user, 2, client("b"))
        .await
        .unwrap();
}
#[tokio::test]
async fn background_rotation_fences_pages_refresh_and_delayed_failure() {
    use moosuri_core::*;
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    store
        .save_google_setup(&a.user, 0, client("a"))
        .await
        .unwrap();
    let pin = app(&store).await.pin;
    let mail = connect(&store, &a.user, pin.clone()).await;
    let c = store
        .create_source_connection(
            &a.user,
            CreateSourceConnection {
                name: "Synthetic Gmail".into(),
                kind: SourceConnectionKind::Gmail,
                scope: "{\"label_ids\":[\"INBOX\"]}".into(),
                remote_account: mail.email,
                mail_connection_id: Some(mail.id),
                sync_enabled: true,
                poll_interval_seconds: 60,
            },
        )
        .await
        .unwrap();
    let cap = store.next_source_connection().await.unwrap().unwrap();
    let credentials = store
        .source_mail_credentials_with_app(&cap, pin.clone())
        .await
        .unwrap();
    let lease = store.begin_source_run(&cap).await.unwrap();
    let page = SourceBatch {
        protocol_version: 1,
        request_id: Uuid::new_v4().to_string(),
        config_revision: c.config_revision,
        run_id: lease.run_id().into(),
        generation: lease.generation(),
        expected_sequence: 0,
        checkpoint_before: None,
        checkpoint_after: Some("synthetic-cursor".into()),
        entries: vec![],
        metadata: vec![],
        tombstones: vec![],
        complete_snapshot: false,
    };
    let receipt = store
        .commit_source_batch(&lease, page.clone())
        .await
        .unwrap();
    store
        .save_google_setup(&a.user, 1, client("a"))
        .await
        .unwrap();
    assert!(matches!(
        store
            .save_source_mail_refresh_with_app(&cap, credentials.revision, tokens(), pin.clone())
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store.source_mail_credentials_with_app(&cap, pin).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .finish_source_run(
                &lease,
                SourceRunOutcome::NeedsReconnect,
                Some("authentication_required".into()),
                None
            )
            .await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store
            .commit_source_batch(&lease, page.clone())
            .await
            .unwrap()
            .sequence,
        receipt.sequence
    );
    let mut fresh = page;
    fresh.request_id = Uuid::new_v4().to_string();
    fresh.expected_sequence = 1;
    fresh.checkpoint_before = fresh.checkpoint_after.clone();
    assert!(matches!(
        store.commit_source_batch(&lease, fresh).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store
            .source_connection(&a.user, c.id.clone())
            .await
            .unwrap()
            .state,
        "active"
    );
    let run = store
        .source_sync_runs(&a.user, c.id)
        .await
        .unwrap()
        .remove(0);
    assert_eq!(run.state, "unknown");
    assert_eq!(run.error.as_deref(), Some("lease_invalidated"));
}
#[tokio::test]
async fn global_legacy_connections_pending_and_expiry_are_respected() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "second@example.test").await;
    let start = store.start_mail_oauth(&b.user).await.unwrap();
    let handle = store
        .capture_mail_oauth(start.state, SecretString::new("code".into()).unwrap())
        .await
        .unwrap();
    let ex = store.claim_mail_oauth(&b.user, handle).await.unwrap();
    store
        .complete_mail_oauth(ex, tokens(), "second@example.test".into())
        .await
        .unwrap();
    assert!(matches!(
        store.save_google_setup(&a.user, 0, client("a")).await,
        Err(StoreError::GoogleConnectionsInUse)
    ));
    let c = store.mail_connections(&b.user).await.unwrap().remove(0);
    store.disconnect_mail(&b.user, c.id).await.unwrap();
    let start = store.start_mail_oauth(&b.user).await.unwrap();
    assert!(matches!(
        store.save_google_setup(&a.user, 0, client("a")).await,
        Err(StoreError::GoogleAuthorizationInProgress)
    ));
    store
        .call(|db| {
            db.conn
                .execute("UPDATE mail_oauth_sessions SET expires_at=0", [])?;
            Ok(())
        })
        .await
        .unwrap();
    store
        .save_google_setup(&a.user, 0, client("a"))
        .await
        .unwrap();
    assert!(
        store
            .capture_mail_oauth(start.state, SecretString::new("late".into()).unwrap())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn restricted_admin_cannot_read_or_write_setup() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Temporary admin".into(),
            "temporary@example.test".into(),
            crate::tests::PASSWORD.into(),
            true,
        )
        .await
        .unwrap();
    let user = store
        .login(
            "temporary@example.test".into(),
            crate::tests::PASSWORD.into(),
        )
        .await
        .unwrap()
        .user;
    assert!(matches!(
        store.google_setup(&user).await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store.save_google_setup(&user, 0, client("a")).await,
        Err(StoreError::Forbidden)
    ));
}
#[tokio::test]
async fn explicit_external_pin_can_use_legacy_but_not_other_known_client() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let legacy = store.start_mail_oauth(&a.user).await.unwrap();
    let h = store
        .capture_mail_oauth(legacy.state, SecretString::new("code".into()).unwrap())
        .await
        .unwrap();
    let ex = store.claim_mail_oauth(&a.user, h).await.unwrap();
    let legacy = store
        .complete_mail_oauth(ex, tokens(), "sender@example.test".into())
        .await
        .unwrap();
    let pin = GoogleAppPin {
        revision: None,
        client_id: "a".into(),
        redirect_uri: client("a").redirect_uri,
    };
    store
        .pin_mail_binding(
            store
                .mail_binding(&a.user, legacy.id.clone())
                .await
                .unwrap(),
            pin.clone(),
        )
        .await
        .unwrap();
    store.disconnect_mail(&a.user, legacy.id).await.unwrap();
    let c = connect(&store, &a.user, pin.clone()).await;
    let mut wrong = pin;
    wrong.client_id = "b".into();
    assert!(matches!(
        store
            .pin_mail_binding(store.mail_binding(&a.user, c.id).await.unwrap(), wrong)
            .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn claimed_pin_must_match_and_expired_exchange_cannot_complete_after_rotation() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    store
        .save_google_setup(&a.user, 0, client("a"))
        .await
        .unwrap();
    let pin = app(&store).await.pin;
    let start = store
        .start_mail_oauth_with_app(&a.user, pin.clone(), true)
        .await
        .unwrap();
    let handle = store
        .capture_mail_oauth(start.state, SecretString::new("code".into()).unwrap())
        .await
        .unwrap();
    let mut wrong = pin.clone();
    wrong.client_id = "b".into();
    assert!(matches!(
        store
            .claim_mail_oauth_with_app(&a.user, handle.clone(), wrong)
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store.claim_mail_oauth(&a.user, handle.clone()).await,
        Err(StoreError::Conflict)
    ));
    let exchange = store
        .claim_mail_oauth_with_app(&a.user, handle, pin)
        .await
        .unwrap();
    store
        .call(|db| {
            db.conn
                .execute("UPDATE mail_oauth_sessions SET expires_at=0", [])?;
            Ok(())
        })
        .await
        .unwrap();
    store
        .save_google_setup(&a.user, 1, client("a"))
        .await
        .unwrap();
    assert!(matches!(
        store
            .complete_mail_oauth(exchange, tokens(), "sender@example.test".into())
            .await,
        Err(StoreError::Conflict)
    ));
}
