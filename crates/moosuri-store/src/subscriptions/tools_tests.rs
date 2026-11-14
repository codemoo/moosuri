use super::*;
use crate::tests::{TEST_LOCK, admin, member};

async fn seeded(store: &Store, u: &UserContext) -> SubscriptionRef {
    let login = store
        .begin_subscription_login(u, None, "Synthetic capability metadata".into())
        .await
        .unwrap();
    let b = login.binding;
    let seed = b.clone();
    // Fake ciphertext is never opened. These cases test Store capability admission only.
    store.call(move|db|{
        db.conn.execute("UPDATE chatgpt_bindings SET status='active' WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![seed.tenant,seed.account,seed.id])?;
        db.conn.execute("INSERT INTO chatgpt_sessions(tenant_id,account_id,binding_id,revision,generation,cipher,expires_at,plan_enabled) VALUES(?1,?2,?3,?4,1,x'00',9999999999,1)",params![seed.tenant,seed.account,seed.id,seed.revision])?;
        Ok(())
    }).await.unwrap();
    SubscriptionRef {
        model: "fixture-model".into(),
        effort: Some("high".into()),
        ..b
    }
}
#[tokio::test]
async fn exact_owner_tool_proof_needs_text_and_current_revision_and_survives_text_reprobe() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let other = member(&s, &a.user, "other@example.test").await;
    let b = seeded(&s, &a.user).await;
    assert!(
        s.subscription_tool_probe_binding(
            &a.user,
            b.id.clone(),
            b.revision,
            b.model.clone(),
            b.effort.clone()
        )
        .await
        .is_err()
    );
    let text = s
        .record_subscription_probe(&a.user, b.clone())
        .await
        .unwrap();
    assert!(!text.function_tools);
    assert!(
        s.subscription_tool_probe_binding(
            &other.user,
            b.id.clone(),
            b.revision,
            b.model.clone(),
            b.effort.clone()
        )
        .await
        .is_err()
    );
    assert!(
        s.record_subscription_tool_probe(&other.user, b.clone())
            .await
            .is_err()
    );
    assert!(
        s.subscription_tool_probe_binding(&a.user, b.id.clone(), b.revision, b.model.clone(), None)
            .await
            .is_err()
    );
    let (_, challenge) = s
        .subscription_tool_probe_binding(
            &a.user,
            b.id.clone(),
            b.revision,
            b.model.clone(),
            b.effort.clone(),
        )
        .await
        .unwrap();
    assert!(challenge.starts_with("proof-"));
    let proven = s
        .record_subscription_tool_probe(&a.user, b.clone())
        .await
        .unwrap();
    assert!(proven.function_tools);
    assert!(
        s.record_subscription_probe(&a.user, b.clone())
            .await
            .unwrap()
            .function_tools
    );
    let u = a.user.clone();
    let frozen = text.clone();
    let proven2 = proven.clone();
    s.call(move |db| {
        // A new proof does not make old frozen text choice invalid or grant it tools.
        assert!(binding(&db.conn, &u.tenant.0, &u.account.id, &frozen).is_ok());
        assert!(!frozen.function_tools);
        assert!(binding(&db.conn, &u.tenant.0, &u.account.id, &proven2).is_ok());
        db.conn
            .execute("UPDATE chatgpt_model_probes SET function_tools=0", [])?;
        assert!(binding(&db.conn, &u.tenant.0, &u.account.id, &proven2).is_err());
        assert!(binding(&db.conn, &u.tenant.0, &u.account.id, &frozen).is_ok());
        Ok(())
    })
    .await
    .unwrap();
    let late = b.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE chatgpt_bindings SET status='disconnected' WHERE id=?1",
            [b.id],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    // Disconnection fences metadata proof before attempting to open the fake carrier.
    assert!(
        s.record_subscription_tool_probe(&a.user, late)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn schema35_personal_probes_migrate_with_disabled_tool_capability() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let b = seeded(&s, &a.user).await;
    s.record_subscription_probe(&a.user, b).await.unwrap();
    s.call(|db| {
        db.conn.execute_batch(
            "DROP INDEX local_notifications_source;DROP TRIGGER notification_suppression_monotonic;DROP TABLE notification_source_preferences;ALTER TABLE local_notifications DROP COLUMN source_suppressed;ALTER TABLE local_notifications DROP COLUMN push_suppressed;DROP TABLE learned_skill_receipts;DROP TABLE learned_skill_observations;DROP TABLE learned_skill_uses;DROP TABLE learned_skill_proofs;DROP TABLE learned_skills;DROP TABLE credential_broker_auth;DROP TABLE credential_broker_bindings;DROP TABLE credential_broker_devices;DROP TABLE credential_broker_pairs;DROP TABLE credential_broker_invocations;DROP TABLE credential_broker_commitments;ALTER TABLE chatgpt_model_probes DROP COLUMN function_tools;PRAGMA user_version=35;",
        )?;
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    let u = s.authenticate(a.token).await.unwrap();
    assert!(!s.model_settings(&u).await.unwrap().choices[0].function_tools);
    s.call(|db| {
        assert_eq!(
            db.conn
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?,
            crate::CURRENT_SCHEMA_VERSION
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn member_owner_can_verify_own_tools_and_logout_fences_late_proof() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let m = member(&s, &a.user, "own-tools@example.test").await;
    let b = seeded(&s, &m.user).await;
    s.record_subscription_probe(&m.user, b.clone())
        .await
        .unwrap();
    assert!(
        s.subscription_tool_probe_binding(
            &m.user,
            b.id.clone(),
            b.revision,
            b.model.clone(),
            b.effort.clone()
        )
        .await
        .is_ok()
    );
    assert!(
        s.record_subscription_tool_probe(&m.user, b.clone())
            .await
            .unwrap()
            .function_tools
    );
    let helper = s
        .login_chatgpt_helper(
            "own-tools@example.test".into(),
            crate::tests::PASSWORD.into(),
        )
        .await
        .unwrap();
    assert!(
        s.subscription_tool_probe_binding(
            &helper.user,
            b.id.clone(),
            b.revision,
            b.model.clone(),
            b.effort.clone()
        )
        .await
        .is_err()
    );
    s.logout(&m.user).await.unwrap();
    assert!(s.record_subscription_tool_probe(&m.user, b).await.is_err());
}
