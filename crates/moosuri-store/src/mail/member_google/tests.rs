use super::*;
use crate::tests::{TEST_LOCK, directory};
use serde_json::{Value, json};
const PASSWORD: &str = "Synthetic-google-password-42";
const CHANGED: &str = "Changed-google-password-42";
const ACCESS: &str = "SYNTHETIC-MEMBER-GOOGLE-ACCESS-CANARY";
const REFRESH: &str = "SYNTHETIC-MEMBER-GOOGLE-REFRESH-CANARY";
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
            "Google member".into(),
            "member@example.test".into(),
            PASSWORD.into(),
        )
        .await
        .unwrap();
    let s = owner.open_verified(dir.path()).await.unwrap();
    let initial = s
        .login("member@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert!(
        s.start_member_google_oauth(&initial.user, owner, Uuid::new_v4())
            .await
            .is_err()
    );
    let u = s
        .change_password(&initial.user, CHANGED.into())
        .await
        .unwrap()
        .user;
    (dir, s, u, owner, Uuid::new_v4())
}
fn wire_owner(o: VolumeOwner) -> Value {
    json!({"account_id":o.account,"tenant_id":o.tenant,"volume_id":o.volume,"generation":o.generation,"peer_uid":unsafe{libc::geteuid()}})
}
fn binding(o: VolumeOwner, app: Uuid, purpose: &str, expires: i64) -> String {
    let payload = json!({"v":1,"owner":wire_owner(o),"app_revision":app,"purpose":purpose,"mailbox":"member@example.test","token_tag":URL_SAFE_NO_PAD.encode([7u8;32]),"expires_at":expires});
    format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap()),
        URL_SAFE_NO_PAD.encode([8u8; 32])
    )
}
fn reply(id: Uuid, o: VolumeOwner, app: Uuid, refresh: Option<&str>) -> Value {
    json!({"v":1,"request_id":id,"owner":wire_owner(o),"app_revision":app,"state":"ok","result":{"access_token":ACCESS,"refresh_token":refresh,"expires_in":3600,"scopes":[READ],"mailbox":"member@example.test","access_binding":binding(o,app,"access",now().unwrap()+3600),"refresh_binding":binding(o,app,"refresh",now().unwrap()+86400)}})
}
fn carrier(id: Uuid, reply: Value) -> MemberGoogleGrant {
    MemberGoogleGrant::from_broker_reply(id, Zeroizing::new(serde_json::to_vec(&reply).unwrap()))
        .unwrap()
}
async fn exchange(s: &Store, u: &UserContext, o: VolumeOwner, app: Uuid) -> MailOAuthExchange {
    let start = s.start_member_google_oauth(u, o, app).await.unwrap();
    let h = s
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    s.claim_member_google_oauth(u, o, app, h).await.unwrap()
}
async fn connect(s: &Store, u: &UserContext, o: VolumeOwner, app: Uuid) -> MemberGoogleStatus {
    let e = exchange(s, u, o, app).await;
    let id = e.member_google_request_id().unwrap();
    s.complete_member_google_oauth(e, carrier(id, reply(id, o, app, Some(REFRESH))))
        .await
        .unwrap()
}
#[tokio::test]
async fn consent_is_exact_session_owner_app_one_use_and_cannot_become_send_authority() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, o, app) = setup().await;
    let start = s.start_member_google_oauth(&u, o, app).await.unwrap();
    let handle = s
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let other = s
        .login("member@example.test".into(), CHANGED.into())
        .await
        .unwrap()
        .user;
    assert!(s.claim_mail_oauth(&u, handle.clone()).await.is_err());
    assert!(
        s.claim_member_google_oauth(&other, o, app, handle.clone())
            .await
            .is_err()
    );
    assert!(
        s.claim_member_google_oauth(
            &u,
            VolumeOwner {
                volume: Uuid::new_v4(),
                ..o
            },
            app,
            handle.clone()
        )
        .await
        .is_err()
    );
    assert!(
        s.claim_member_google_oauth(&u, o, Uuid::new_v4(), handle.clone())
            .await
            .is_err()
    );
    let e = s
        .claim_member_google_oauth(&u, o, app, handle.clone())
        .await
        .unwrap();
    assert!(
        s.claim_member_google_oauth(&u, o, app, handle)
            .await
            .is_err()
    );
    assert!(
        s.complete_mail_oauth(
            e,
            TokenSet {
                access_token: SecretString::new(ACCESS.into()).unwrap(),
                refresh_token: Some(SecretString::new(REFRESH.into()).unwrap()),
                expires_in: 3600,
                scopes: None
            },
            "member@example.test".into()
        )
        .await
        .is_err()
    );
    let first = exchange(&s, &u, o, app).await;
    let second = exchange(&s, &u, o, app).await;
    let id = first.member_google_request_id().unwrap();
    let connected = s
        .complete_member_google_oauth(first, carrier(id, reply(id, o, app, Some(REFRESH))))
        .await
        .unwrap();
    let id = second.member_google_request_id().unwrap();
    assert!(
        s.complete_member_google_oauth(second, carrier(id, reply(id, o, app, Some(REFRESH))))
            .await
            .is_err()
    );
    assert!(connected.connected && connected.read_only && !connected.send_enabled);
    assert!(s.mail_connections(&u).await.unwrap().is_empty());
    assert!(s.mail_actions(&u).await.unwrap().is_empty());
    assert!(s.next_mail_dispatch().await.unwrap().is_none());
}
#[tokio::test]
async fn app_changes_invalidate_pending_flows_credentials_and_cannot_roll_back() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, o, app) = setup().await;
    connect(&s, &u, o, app).await;
    let old = s.member_google_read_binding(&u, o, app).await.unwrap();
    let pending = exchange(&s, &u, o, app).await;
    let next = Uuid::new_v4();
    let _new = s.start_member_google_oauth(&u, o, next).await.unwrap();
    assert!(s.check_member_google_read(&old).await.is_err());
    assert!(s.member_google_read_binding(&u, o, app).await.is_err());
    assert!(s.start_member_google_oauth(&u, o, app).await.is_err());
    let id = pending.member_google_request_id().unwrap();
    assert!(
        s.complete_member_google_oauth(pending, carrier(id, reply(id, o, app, Some(REFRESH))))
            .await
            .is_err()
    );
    let status = s.member_google_status(&u, o, next).await.unwrap();
    assert_eq!(status.status, "needs_reauth");
    assert!(!status.connected);
}
#[tokio::test]
async fn encrypted_readonly_grants_reject_malformed_scope_request_and_owner_receipts() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, o, app) = setup().await;
    for case in 0..8 {
        let e = exchange(&s, &u, o, app).await;
        let id = e.member_google_request_id().unwrap();
        let mut r = reply(id, o, app, Some(REFRESH));
        match case {
            0 => r["request_id"] = json!(Uuid::new_v4()),
            1 => r["result"]["scopes"] = json!([READ, moosuri_connectors::gmail::COMPOSE_SCOPE]),
            2 => r["owner"]["volume_id"] = json!(Uuid::new_v4()),
            3 => r["owner"]["peer_uid"] = json!(unsafe { libc::geteuid() }.saturating_add(1)),
            4 => r["result"]["refresh_token"] = Value::Null,
            5 => {
                r["result"]["access_binding"] =
                    json!(binding(o, app, "refresh", now().unwrap() + 3600))
            }
            6 => {
                r["result"]["access_binding"] =
                    json!(binding(o, Uuid::new_v4(), "access", now().unwrap() + 3600))
            }
            _ => {
                r["result"]["access_binding"] = json!(binding(o, app, "access", now().unwrap() - 1))
            }
        }
        assert!(
            s.complete_member_google_oauth(e, carrier(id, r))
                .await
                .is_err(),
            "case {case}"
        );
        s.call(|db| {
            db.conn.execute("DELETE FROM mail_oauth_sessions", [])?;
            Ok(())
        })
        .await
        .unwrap();
    }
    let st = connect(&s, &u, o, app).await;
    assert!(st.expires_at.is_some() && !st.needs_refresh);
    let safe = serde_json::to_string(&s.member_google_status_local(&u, o).await.unwrap()).unwrap();
    assert!(!safe.contains("CANARY") && !safe.contains("binding") && !safe.contains("token"));
    let b = s.member_google_read_binding(&u, o, app).await.unwrap();
    assert_eq!(b.access_token.expose(), ACCESS);
    s.check_member_google_read(&b).await.unwrap();
    let path = s
        .call(|db| Ok(db.conn.path().unwrap().to_owned()))
        .await
        .unwrap();
    for file in [
        std::path::PathBuf::from(&path),
        std::path::PathBuf::from(format!("{path}-wal")),
    ] {
        let bytes = std::fs::read(file).unwrap_or_default();
        assert!(
            !bytes
                .windows(b"SYNTHETIC-MEMBER-GOOGLE".len())
                .any(|v| v == b"SYNTHETIC-MEMBER-GOOGLE")
        );
    }
    assert!(dir.path().exists());
}
#[tokio::test]
async fn refresh_is_single_dispatch_preserves_or_rotates_refresh_and_fences_old_reads() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, o, app) = setup().await;
    connect(&s, &u, o, app).await;
    for token in [None, Some("SYNTHETIC-ROTATED-REFRESH")] {
        let old = s.member_google_read_binding(&u, o, app).await.unwrap();
        let lease = s.claim_member_google_refresh(&u, o, app).await.unwrap();
        assert!(s.claim_member_google_refresh(&u, o, app).await.is_err());
        let id = lease.request_id;
        let mut r = reply(id, o, app, token);
        r["result"]["refresh_binding"] = json!(binding(o, app, "refresh", now().unwrap() + 172800));
        let receipt = r["result"]["refresh_binding"].as_str().unwrap().to_owned();
        s.settle_member_google_refresh(lease, Some(carrier(id, r)))
            .await
            .unwrap();
        assert!(s.check_member_google_read(&old).await.is_err());
        let new = s.member_google_read_binding(&u, o, app).await.unwrap();
        assert!(new.credential_version > old.credential_version);
        let next = s.claim_member_google_refresh(&u, o, app).await.unwrap();
        assert_eq!(next.refresh_token.expose(), token.unwrap_or(REFRESH));
        assert_eq!(next.refresh_binding, receipt);
        let id = next.request_id;
        s.settle_member_google_refresh(next, Some(carrier(id, reply(id, o, app, token))))
            .await
            .unwrap();
    }
    s.call(|db| {
        db.conn.execute(
            "UPDATE member_google_grants SET expires_at=unixepoch()-1",
            [],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.member_google_status_local(&u, o)
            .await
            .unwrap()
            .needs_refresh
    );
    let expired = s.member_google_read_binding(&u, o, app).await.unwrap();
    assert!(expired.needs_refresh);
    assert!(s.check_member_google_read(&expired).await.is_err());
    let lease = s.claim_member_google_refresh(&u, o, app).await.unwrap();
    assert_eq!(
        s.settle_member_google_refresh(lease, None)
            .await
            .unwrap()
            .status,
        "needs_reauth"
    );
    assert!(s.claim_member_google_refresh(&u, o, app).await.is_err());
    assert!(s.member_google_read_binding(&u, o, app).await.is_err());
}
#[tokio::test]
async fn pinned_disconnect_reconnect_logout_and_password_changes_fence_inflight_work() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, o, app) = setup().await;
    let first = connect(&s, &u, o, app).await;
    let b = s.member_google_read_binding(&u, o, app).await.unwrap();
    let lease = s.claim_member_google_refresh(&u, o, app).await.unwrap();
    s.disconnect_member_google(&u, o, first.connection_id.unwrap(), first.epoch)
        .await
        .unwrap();
    assert!(s.check_member_google_read(&b).await.is_err());
    let second = connect(&s, &u, o, app).await;
    assert!(
        s.disconnect_member_google(&u, o, first.connection_id.unwrap(), first.epoch)
            .await
            .is_err()
    );
    let id = lease.request_id;
    assert!(
        s.settle_member_google_refresh(lease, Some(carrier(id, reply(id, o, app, Some(REFRESH)))))
            .await
            .is_err()
    );
    assert_eq!(
        s.member_google_status_local(&u, o)
            .await
            .unwrap()
            .connection_id,
        second.connection_id
    );
    let b = s.member_google_read_binding(&u, o, app).await.unwrap();
    let lease = s.claim_member_google_refresh(&u, o, app).await.unwrap();
    s.logout(&u).await.unwrap();
    assert!(s.check_member_google_read(&b).await.is_err());
    let id = lease.request_id;
    assert!(
        s.settle_member_google_refresh(lease, Some(carrier(id, reply(id, o, app, Some(REFRESH)))))
            .await
            .is_err()
    );
    let u = s
        .login("member@example.test".into(), CHANGED.into())
        .await
        .unwrap()
        .user;
    assert_eq!(
        s.member_google_status_local(&u, o).await.unwrap().status,
        "needs_reauth"
    );
    connect(&s, &u, o, app).await;
    let b = s.member_google_read_binding(&u, o, app).await.unwrap();
    let lease = s.claim_member_google_refresh(&u, o, app).await.unwrap();
    let new = s
        .change_password(&u, "Second-changed-password-42".into())
        .await
        .unwrap()
        .user;
    assert!(s.check_member_google_read(&b).await.is_err());
    let id = lease.request_id;
    assert!(
        s.settle_member_google_refresh(lease, Some(carrier(id, reply(id, o, app, Some(REFRESH)))))
            .await
            .is_err()
    );
    assert!(
        !s.member_google_status_local(&new, o)
            .await
            .unwrap()
            .connected
    );
}
#[tokio::test]
async fn restart_discards_unknown_refresh_but_keeps_ordinary_encrypted_grants() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, o, app) = setup().await;
    let st = connect(&s, &u, o, app).await;
    drop(s);
    let s = o.open_verified(dir.path()).await.unwrap();
    assert_eq!(
        s.member_google_status_local(&u, o)
            .await
            .unwrap()
            .connection_id,
        st.connection_id
    );
    let b = s.member_google_read_binding(&u, o, app).await.unwrap();
    s.check_member_google_read(&b).await.unwrap();
    let _lease = s.claim_member_google_refresh(&u, o, app).await.unwrap();
    drop(s);
    let s = o.open_verified(dir.path()).await.unwrap();
    let st = s.member_google_status_local(&u, o).await.unwrap();
    assert_eq!(st.status, "needs_reauth");
    assert!(!st.connected && !st.refresh_pending);
    assert!(st.expires_at.is_none());
    assert!(s.claim_member_google_refresh(&u, o, app).await.is_err());
    assert!(s.member_google_read_binding(&u, o, app).await.is_err());
}
