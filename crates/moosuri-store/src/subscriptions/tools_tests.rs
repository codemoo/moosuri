use super::*;
use crate::tests::{TEST_LOCK, additional_admin, admin, member};

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
async fn owner_subscription_is_private_and_yields_to_the_installation_default() {
    use crate::{CredentialChange, ProviderChange};
    use moosuri_core::ProviderConfig;

    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path()).unwrap();
    let admin = admin(&store).await.user;
    let member = member(&store, &admin, "model-member@example.test")
        .await
        .user;
    let other = additional_admin(&store, "other-model-admin@example.test")
        .await
        .user;
    let subscription = seeded(&store, &admin).await;
    store
        .record_subscription_probe(&admin, subscription.clone())
        .await
        .unwrap();
    let personal = ModelSelection {
        source: ModelSource::PersonalChatGpt,
        provider_id: subscription.id.clone(),
        provider_revision: subscription.revision,
        model: subscription.model.clone(),
        effort: subscription.effort.clone(),
    };
    store
        .set_model_preference(&admin, Some(personal.clone()))
        .await
        .unwrap();
    assert!(matches!(
        store
            .set_default_model(&admin, Some(personal.clone()))
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .set_model_preference(&other, Some(personal.clone()))
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store.resolve_model(&member).await,
        Err(StoreError::Unavailable)
    ));

    let private_thread = store
        .create_thread(&admin, "Private model".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &admin,
            private_thread.id,
            Uuid::new_v4().to_string(),
            "Fixture prompt".into(),
        )
        .await
        .unwrap();
    let run = store.claim_agent_run().await.unwrap().unwrap();
    let binding = run.binding.subscription.unwrap();
    assert_eq!(binding.tenant, admin.tenant.0);
    assert_eq!(binding.account, admin.account.id);
    assert_eq!(binding.id, subscription.id);
    assert_eq!(binding.effort, subscription.effort);
    assert!(run.binding.credential.is_none());

    let shared = store
        .create_provider(
            &admin,
            ProviderChange {
                name: "Fixture API".into(),
                config: ProviderConfig {
                    kind: ProviderKind::OpenAi,
                    base_url: None,
                    allowed_ips: vec![],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Replace(Zeroizing::new("fixture-api-key".into())),
            },
        )
        .await
        .unwrap();
    let global = ModelSelection {
        source: ModelSource::Shared,
        provider_id: shared.id,
        provider_revision: shared.revision,
        model: "api-fixture-model".into(),
        effort: Some("medium".into()),
    };
    store
        .record_model_probe(
            &admin,
            global.provider_id.clone(),
            global.provider_revision,
            global.model.clone(),
            global.effort.clone(),
        )
        .await
        .unwrap();
    store
        .set_default_model(&admin, Some(global.clone()))
        .await
        .unwrap();
    assert!(matches!(
        store
            .begin_agent_step(&run.lease, run.context.checkpoint_seq)
            .await,
        Err(StoreError::Conflict)
    ));
    for user in [&admin, &member, &other] {
        let binding = store.resolve_model(user).await.unwrap();
        assert_eq!(binding.id, global.provider_id);
        assert_eq!(binding.model, global.model);
        assert_eq!(binding.effort, global.effort);
        assert!(binding.subscription.is_none());
    }

    let settings = store.model_settings(&admin).await.unwrap();
    assert_eq!(settings.selection, Some(personal));
    assert_eq!(settings.installation_default, Some(global));
    store.set_default_model(&admin, None).await.unwrap();
    assert!(
        store
            .resolve_model(&admin)
            .await
            .unwrap()
            .subscription
            .is_some()
    );
    assert!(matches!(
        store.resolve_model(&member).await,
        Err(StoreError::Unavailable)
    ));
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
        crate::schema::tests::replace_with_predecessor(&mut db.conn, 35);
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
async fn admin_owner_can_verify_own_tools_and_logout_fences_late_proof() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    admin(&s).await;
    let m = additional_admin(&s, "own-tools@example.test").await;
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
