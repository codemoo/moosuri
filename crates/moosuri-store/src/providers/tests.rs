use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, additional_admin, admin, directory, member};
fn config() -> ProviderConfig {
    ProviderConfig {
        kind: ProviderKind::OpenAi,
        base_url: None,
        allowed_ips: vec![],
        router_providers: vec![],
        zero_data_retention: false,
    }
}
fn change(secret: &str) -> ProviderChange {
    ProviderChange {
        name: "Synthetic provider".into(),
        config: config(),
        enabled: true,
        credential: CredentialChange::Replace(Zeroizing::new(secret.into())),
    }
}
fn selection(id: &str, model: &str) -> ModelSelection {
    ModelSelection {
        source: Default::default(),
        provider_id: id.into(),
        provider_revision: 1,
        model: model.into(),
        effort: None,
    }
}
#[tokio::test]
async fn persistence_encrypted_secret_revision_probe_and_no_fallback() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await;
    let row = store
        .create_provider(&admin.user, change("SYNTHETIC-PRIVATE-KEY"))
        .await
        .unwrap();
    assert!(row.has_credential);
    assert!(row.choices.is_empty());
    assert!(matches!(
        store.resolve_model(&admin.user).await,
        Err(StoreError::Unavailable)
    ));
    assert!(matches!(
        store
            .set_default_model(&admin.user, Some(selection(&row.id, "synthetic-model")))
            .await,
        Err(StoreError::Conflict)
    ));
    store
        .record_model_probe(
            &admin.user,
            row.id.clone(),
            1,
            "synthetic-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(&admin.user, Some(selection(&row.id, "synthetic-model")))
        .await
        .unwrap();
    let binding = store.resolve_model(&admin.user).await.unwrap();
    assert_eq!(
        binding.credential.as_deref().map(String::as_str),
        Some("SYNTHETIC-PRIVATE-KEY")
    );
    let token = admin.token;
    drop(store);
    // Raw SQLite bytes/WAL never contain the API key.
    for name in ["assistant.sqlite", "assistant.sqlite-wal"] {
        if let Ok(bytes) = fs::read(dir.path().join(name)) {
            assert!(!bytes.windows(21).any(|w| w == b"SYNTHETIC-PRIVATE-KEY"));
        }
    }
    let store = Store::open(dir.path()).unwrap();
    let admin = store.authenticate(token).await.unwrap();
    assert_eq!(store.resolve_model(&admin).await.unwrap().revision, 1);
    let next = store
        .update_provider(
            &admin,
            row.id.clone(),
            1,
            ProviderChange {
                name: "Renamed".into(),
                config: config(),
                enabled: true,
                credential: CredentialChange::Keep,
            },
        )
        .await
        .unwrap();
    let id = row.id.clone();
    store.call(move|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM provider_revisions WHERE provider_id=?1 AND credential IS NOT NULL",[&id],|r|r.get::<_,i64>(0))?,1);assert!(db.conn.query_row("SELECT credential IS NULL FROM provider_revisions WHERE provider_id=?1 AND revision=1",[id],|r|r.get::<_,bool>(0))?);Ok(())}).await.unwrap();
    assert_eq!(next.revision, 2);
    assert!(next.choices.is_empty());
    assert!(matches!(
        store.resolve_model(&admin).await,
        Err(StoreError::Conflict)
    ));
    assert!(
        store
            .model_settings(&admin)
            .await
            .unwrap()
            .effective
            .is_none()
    );
    assert!(matches!(
        store
            .record_model_probe(&admin, row.id.clone(), 1, "synthetic-model".into(), None)
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .validate_provider_binding(&admin, row.id.clone(), 1)
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .update_provider(&admin, row.id.clone(), 1, change("another-key"))
            .await,
        Err(StoreError::Conflict)
    ));
    store
        .record_model_probe(
            &admin,
            row.id.clone(),
            2,
            "synthetic-model".into(),
            Some("ultra".into()),
        )
        .await
        .unwrap();
    store
        .record_model_probe(&admin, row.id.clone(), 2, "synthetic-model".into(), None)
        .await
        .unwrap();
    assert!(matches!(
        store
            .set_default_model(&admin, Some(selection(&row.id, "synthetic-model")))
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .set_model_preference(&admin, Some(selection(&row.id, "synthetic-model")))
            .await,
        Err(StoreError::Conflict)
    ));
    let selected = ModelSelection {
        source: Default::default(),
        provider_id: row.id.clone(),
        provider_revision: 2,
        model: "synthetic-model".into(),
        effort: Some("ultra".into()),
    };
    store
        .set_default_model(&admin, Some(selected))
        .await
        .unwrap();
    assert_eq!(
        store.resolve_model(&admin).await.unwrap().effort.as_deref(),
        Some("ultra")
    );
    store
        .update_provider(
            &admin,
            row.id.clone(),
            2,
            ProviderChange {
                name: "Disabled".into(),
                config: config(),
                enabled: false,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    let id = row.id.clone();
    store.call(move|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM provider_revisions WHERE provider_id=?1 AND credential IS NOT NULL",[id],|r|r.get::<_,i64>(0))?,0);Ok(())}).await.unwrap();
    store
        .update_provider(&admin, row.id.clone(), 3, change("replacement-one"))
        .await
        .unwrap();
    store
        .update_provider(&admin, row.id.clone(), 4, change("replacement-two"))
        .await
        .unwrap();
    let id = row.id.clone();
    store.call(move|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM provider_revisions WHERE provider_id=?1 AND revision<5 AND credential IS NOT NULL",[&id],|r|r.get::<_,i64>(0))?,0);assert_eq!(db.conn.query_row("SELECT count(*) FROM provider_revisions WHERE provider_id=?1 AND revision=5 AND credential IS NOT NULL",[id],|r|r.get::<_,i64>(0))?,1);Ok(())}).await.unwrap();
    assert!(matches!(
        store
            .probe_binding(&admin, row.id, 3, "synthetic-model".into(), None)
            .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn admin_assignment_replaces_stale_member_preference_and_revokes_old_dispatch() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await;
    let a = member(&store, &admin.user, "member@example.test").await;
    let invite = store
        .create_user(
            &admin.user,
            "Restricted".into(),
            "restricted@example.test".into(),
        )
        .await
        .unwrap();
    let restricted = store
        .login("restricted@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    assert!(matches!(
        store.create_provider(&a.user, change("key")).await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store.list_providers(&a.user).await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store.model_settings(&restricted.user).await,
        Err(StoreError::Forbidden)
    ));
    let p = store
        .create_provider(&admin.user, change("key-one"))
        .await
        .unwrap();
    let q = store
        .create_provider(&admin.user, change("key-two"))
        .await
        .unwrap();
    let legacy = store
        .record_model_probe(&admin.user, p.id.clone(), 1, "legacy-model".into(), None)
        .await
        .unwrap();
    let default = store
        .record_model_probe(&admin.user, p.id.clone(), 1, "shared-default".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(&admin.user, Some(selection(&p.id, "shared-default")))
        .await
        .unwrap();
    let member = a.user.clone();
    let stale_provider = p.id.clone();
    store
        .call(move |db| {
            // Simulate the schema-85 member preference preserved by migration.
            db.conn.execute(
                "INSERT INTO personal_model_selections VALUES(?1,?2,?3,1,'legacy-model','')",
                params![member.tenant.0, member.account.id, stale_provider],
            )?;
            assert!(
                binding_for_owner_choice(
                    &db.conn,
                    &db.provider_key,
                    &member.tenant.0,
                    &member.account.id,
                    &legacy
                )
                .is_err()
            );
            assert!(
                binding_for_owner_choice(
                    &db.conn,
                    &db.provider_key,
                    &member.tenant.0,
                    &member.account.id,
                    &default
                )
                .is_ok()
            );
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        store.resolve_model(&a.user).await.unwrap().model,
        "shared-default"
    );
    let mut assigned = selection(&q.id, "assigned-model");
    assigned.effort = Some("high".into());
    let pinned = store
        .record_model_probe(
            &admin.user,
            q.id.clone(),
            1,
            assigned.model.clone(),
            assigned.effort.clone(),
        )
        .await
        .unwrap();
    let account = a.user.account.id.clone();
    assert!(matches!(
        store
            .set_model_preference(&a.user, Some(assigned.clone()))
            .await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store
            .set_account_model_assignment(&a.user, account.clone(), Some(assigned.clone()))
            .await,
        Err(StoreError::Forbidden)
    ));
    let wrong_effort = ModelSelection {
        effort: Some("medium".into()),
        ..assigned.clone()
    };
    assert!(matches!(
        store
            .set_account_model_assignment(&admin.user, account.clone(), Some(wrong_effort))
            .await,
        Err(StoreError::Conflict)
    ));
    let result = store
        .set_account_model_assignment(&admin.user, account.clone(), Some(assigned.clone()))
        .await
        .unwrap();
    assert_eq!(result.selection, Some(assigned.clone()));
    let effective = store.resolve_model(&a.user).await.unwrap();
    assert_eq!(effective.id, q.id);
    assert_eq!(effective.model, assigned.model);
    assert_eq!(effective.effort, assigned.effort);
    assert_eq!(store.resolve_model(&admin.user).await.unwrap().id, p.id);
    let status = store.model_settings(&a.user).await.unwrap();
    assert!(status.selection.is_none() && status.choices.is_empty());
    assert_eq!(status.effective.unwrap().effort, assigned.effort);

    let member = a.user.clone();
    let prior_pin = pinned.clone();
    store
        .call(move |db| {
            let current = selected_mission_model(&db.conn, &member)?;
            assert_eq!(current.effort, prior_pin.effort);
            assert!(
                binding_for_owner_choice(
                    &db.conn,
                    &db.provider_key,
                    &member.tenant.0,
                    &member.account.id,
                    &prior_pin
                )
                .is_ok()
            );
            Ok(())
        })
        .await
        .unwrap();

    let thread = store
        .create_thread(&a.user, "Assignment fixture".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &a.user,
            thread.id,
            Uuid::new_v4().to_string(),
            "Synthetic prompt".into(),
        )
        .await
        .unwrap();
    let claimed = store.claim_run().await.unwrap().unwrap();
    assert_eq!(claimed.binding.model, assigned.model);
    assert_eq!(claimed.binding.effort, assigned.effort);
    store
        .set_account_model_assignment(&admin.user, account.clone(), None)
        .await
        .unwrap();
    assert!(matches!(
        store.check_run(&claimed.lease).await,
        Err(StoreError::Conflict)
    ));
    store
        .set_account_model_assignment(&admin.user, account.clone(), Some(assigned.clone()))
        .await
        .unwrap();

    store
        .update_provider(
            &admin.user,
            q.id.clone(),
            1,
            ProviderChange {
                name: "disabled".into(),
                config: config(),
                enabled: false,
                credential: CredentialChange::Keep,
            },
        )
        .await
        .unwrap();
    assert!(store.resolve_model(&a.user).await.is_err());
    assert!(
        store
            .model_settings(&a.user)
            .await
            .unwrap()
            .effective
            .is_none()
    );

    store
        .set_account_model_assignment(&admin.user, account, None)
        .await
        .unwrap();
    assert_eq!(
        store.resolve_model(&a.user).await.unwrap().model,
        "shared-default"
    );
    let member = a.user.clone();
    store
        .call(move |db| {
            assert!(
                binding_for_owner_choice(
                    &db.conn,
                    &db.provider_key,
                    &member.tenant.0,
                    &member.account.id,
                    &pinned
                )
                .is_err()
            );
            assert_eq!(
                selected(&db.conn, Some(&member))?.unwrap().model,
                "legacy-model"
            );
            Ok(())
        })
        .await
        .unwrap();
    let mut edited = config();
    edited.kind = ProviderKind::Anthropic;
    assert!(matches!(
        store
            .update_provider(
                &admin.user,
                p.id.clone(),
                1,
                ProviderChange {
                    name: "retarget".into(),
                    config: edited,
                    enabled: true,
                    credential: CredentialChange::Keep
                }
            )
            .await,
        Err(StoreError::InvalidInput)
    ));
    let mut hosted = config();
    hosted.base_url = Some("https://attacker.example/v1".into());
    assert!(matches!(
        store
            .create_provider(
                &admin.user,
                ProviderChange {
                    name: "invalid".into(),
                    config: hosted,
                    enabled: true,
                    credential: CredentialChange::Clear
                }
            )
            .await,
        Err(StoreError::InvalidInput)
    ));
    assert!(matches!(
        store
            .probe_binding(&a.user, p.id.clone(), 1, "fixture-model".into(), None)
            .await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store
            .record_model_probe(&admin.user, p.id.clone(), 1, "bad model".into(), None)
            .await,
        Err(StoreError::InvalidInput)
    ));
    let local = ProviderConfig {
        kind: ProviderKind::Compatible,
        base_url: Some("http://127.0.0.1:11434/v1".into()),
        allowed_ips: vec!["127.0.0.1".into()],
        router_providers: vec![],
        zero_data_retention: false,
    };
    let local = store
        .create_provider(
            &admin.user,
            ProviderChange {
                name: "Synthetic local".into(),
                config: local,
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .probe_binding(
                &admin.user,
                local.id.clone(),
                1,
                "local-model".into(),
                Some("high".into())
            )
            .await,
        Err(StoreError::InvalidInput)
    ));
    assert!(
        store
            .probe_binding(&admin.user, local.id, 1, "local-model".into(), None)
            .await
            .unwrap()
            .credential
            .is_none()
    );
    store.logout(&admin.user).await.unwrap();
    assert!(matches!(
        store
            .record_model_probe(&admin.user, p.id.clone(), 1, "fixture-model".into(), None)
            .await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store
            .validate_provider_binding(&admin.user, p.id.clone(), 1)
            .await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.list_providers(&admin.user).await,
        Err(StoreError::Unauthorized)
    ));
    store.logout(&a.user).await.unwrap();
    assert!(matches!(
        store.resolve_model(&a.user).await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn authenticated_cipher_rejects_swaps_tampering_and_config_substitution() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await;
    let a = store
        .create_provider(&admin.user, change("key-one"))
        .await
        .unwrap();
    let b = store
        .create_provider(&admin.user, change("key-two"))
        .await
        .unwrap();
    let id = a.id.clone();
    let other = b.id.clone();
    store.call(move|db|{
        assert!(db.conn.execute("UPDATE provider_revisions SET name='tamper' WHERE provider_id=?1",[&id]).is_err());
        db.conn.execute_batch("DROP TRIGGER provider_revisions_no_update")?;
        db.conn.execute("UPDATE provider_revisions SET credential=(SELECT credential FROM provider_revisions WHERE provider_id=?1) WHERE provider_id=?2",params![other,id])?;Ok(())
    }).await.unwrap();
    assert!(matches!(
        store
            .probe_binding(&admin.user, a.id, 1, "fixture-model".into(), None)
            .await,
        Err(StoreError::Corrupt)
    ));
    let id = b.id.clone();
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE provider_revisions SET credential=zeroblob(50) WHERE provider_id=?1",
                [id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store
            .probe_binding(&admin.user, b.id, 1, "fixture-model".into(), None)
            .await,
        Err(StoreError::Corrupt)
    ));
    // AAD also binds destination configuration and revision, independently of DB triggers.
    let key = [7; 32];
    let c = encrypt(
        &key,
        b"fixture-key",
        &aad("opaque-id", 1, &config()).unwrap(),
    )
    .unwrap();
    assert!(decrypt(&key, &c, &aad("opaque-id", 2, &config()).unwrap()).is_err());
    let mut changed = config();
    changed.kind = ProviderKind::Anthropic;
    assert!(decrypt(&key, &c, &aad("opaque-id", 1, &changed).unwrap()).is_err());
}
#[test]
fn key_marker_missing_wrong_permissions_links_and_wrong_key_fail_closed() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for case in ["missing", "wrong", "permissive", "symlink", "hardlink"] {
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        drop(store);
        let path = dir.path().join("provider-secrets.key");
        let original = fs::read(&path).unwrap();
        let outside = directory();
        let outside_path = outside.path().join("synthetic-key");
        fs::write(&outside_path, &original).unwrap();
        fs::set_permissions(&outside_path, fs::Permissions::from_mode(0o600)).unwrap();
        match case {
            "missing" => fs::remove_file(&path).unwrap(),
            "wrong" => fs::write(&path, [9; 32]).unwrap(),
            "permissive" => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            "symlink" => {
                fs::remove_file(&path).unwrap();
                symlink(&outside_path, &path).unwrap();
            }
            "hardlink" => {
                fs::remove_file(&path).unwrap();
                fs::hard_link(&outside_path, &path).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(Store::open(dir.path()).is_err(), "{case}");
        if case == "missing" {
            assert!(!path.exists());
        }
    }
}
#[tokio::test]
async fn version_one_migrates_without_losing_accounts_or_threads() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch(include_str!(
        "../../../../migrations/assistant/0001_accounts_threads.sql"
    ))
    .unwrap();
    let account = insert_account(
        &conn,
        "Old schema admin".into(),
        "admin@example.test".into(),
        hash_sync(PASSWORD).unwrap(),
        Role::Admin,
        false,
    )
    .unwrap();
    conn.execute("INSERT INTO threads(tenant_id,id,account_id,title,created_at,updated_at) SELECT tenant_id,?1,id,'Survives schema2',0,0 FROM accounts WHERE id=?2",params![Uuid::new_v4().to_string(),account.id]).unwrap();
    drop(conn);
    let store = Store::open(dir.path()).unwrap();
    let admin = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(
        store.list_threads(&admin.user).await.unwrap()[0].title,
        "Survives schema2"
    );
    assert!(
        store
            .list_providers(&admin.user)
            .await
            .unwrap()
            .providers
            .is_empty()
    );
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
                crate::CURRENT_SCHEMA_VERSION
            );
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let user = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(store.list_threads(&user.user).await.unwrap().len(), 1);
}

#[tokio::test]
async fn tool_capability_requires_exact_text_proof_and_current_admin_binding() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await;
    let member = member(&store, &admin.user, "tool-member@example.test").await;
    let profile = store
        .create_provider(&admin.user, change("synthetic-tool-key"))
        .await
        .unwrap();
    assert!(matches!(
        store
            .record_tool_probe(
                &admin.user,
                profile.id.clone(),
                1,
                "tool-model".into(),
                None
            )
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .tool_probe_binding(
                &admin.user,
                profile.id.clone(),
                1,
                "tool-model".into(),
                None
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let plain = store
        .record_model_probe(
            &admin.user,
            profile.id.clone(),
            1,
            "tool-model".into(),
            None,
        )
        .await
        .unwrap();
    assert!(!plain.function_tools);
    let high = store
        .record_model_probe(
            &admin.user,
            profile.id.clone(),
            1,
            "tool-model".into(),
            Some("high".into()),
        )
        .await
        .unwrap();
    assert!(!high.function_tools);
    for (model, effort) in [("other-model", None), ("tool-model", Some("low".into()))] {
        assert!(matches!(
            store
                .record_tool_probe(&admin.user, profile.id.clone(), 1, model.into(), effort)
                .await,
            Err(StoreError::Conflict)
        ));
    }
    assert!(matches!(
        store
            .record_tool_probe(
                &member.user,
                profile.id.clone(),
                1,
                "tool-model".into(),
                None
            )
            .await,
        Err(StoreError::Forbidden)
    ));
    let (binding, challenge) = store
        .tool_probe_binding(
            &admin.user,
            profile.id.clone(),
            1,
            "tool-model".into(),
            None,
        )
        .await
        .unwrap();
    let (_, next_challenge) = store
        .tool_probe_binding(
            &admin.user,
            profile.id.clone(),
            1,
            "tool-model".into(),
            None,
        )
        .await
        .unwrap();
    assert_eq!(binding.revision, 1);
    assert_ne!(challenge, next_challenge);
    assert!(challenge.starts_with("proof-"));
    let proven = store
        .record_tool_probe(
            &admin.user,
            profile.id.clone(),
            1,
            "tool-model".into(),
            None,
        )
        .await
        .unwrap();
    assert!(proven.function_tools);
    let mut legacy = serde_json::to_value(&proven).unwrap();
    legacy.as_object_mut().unwrap().remove("function_tools");
    assert!(
        !serde_json::from_value::<ModelChoice>(legacy)
            .unwrap()
            .function_tools
    );
    assert!(
        store
            .record_model_probe(
                &admin.user,
                profile.id.clone(),
                1,
                "tool-model".into(),
                None
            )
            .await
            .unwrap()
            .function_tools
    );
    let choices = store
        .list_providers(&admin.user)
        .await
        .unwrap()
        .providers
        .remove(0)
        .choices;
    assert!(
        choices
            .iter()
            .find(|c| c.effort.is_none())
            .unwrap()
            .function_tools
    );
    assert!(
        !choices
            .iter()
            .find(|c| c.effort.is_some())
            .unwrap()
            .function_tools
    );
    store
        .set_default_model(&admin.user, Some(selection(&profile.id, "tool-model")))
        .await
        .unwrap();
    assert!(
        store
            .model_settings(&member.user)
            .await
            .unwrap()
            .effective
            .unwrap()
            .function_tools
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(
        store.list_providers(&admin.user).await.unwrap().providers[0]
            .choices
            .iter()
            .any(|c| c.function_tools)
    );
    let changed = store
        .update_provider(
            &admin.user,
            profile.id.clone(),
            1,
            ProviderChange {
                name: "Changed binding".into(),
                config: config(),
                enabled: true,
                credential: CredentialChange::Keep,
            },
        )
        .await
        .unwrap();
    assert_eq!(changed.revision, 2);
    assert!(changed.choices.is_empty());
    assert!(matches!(
        store
            .record_tool_probe(
                &admin.user,
                profile.id.clone(),
                1,
                "tool-model".into(),
                None
            )
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(
        !store
            .record_model_probe(
                &admin.user,
                profile.id.clone(),
                2,
                "tool-model".into(),
                None
            )
            .await
            .unwrap()
            .function_tools
    );
    store.logout(&admin.user).await.unwrap();
    assert!(matches!(
        store
            .record_tool_probe(&admin.user, profile.id, 2, "tool-model".into(), None)
            .await,
        Err(StoreError::Unauthorized)
    ));
}

#[tokio::test]
async fn personal_compatible_destination_owner_storage_and_deleted_account_boundaries() {
    use moosuri_core::ModelSource;
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await.user;
    let own = additional_admin(&store, "private-model-owner@example.test")
        .await
        .user;
    let other = member(&store, &admin, "private-model-other@example.test")
        .await
        .user;
    let mut local = ProviderConfig {
        kind: ProviderKind::Compatible,
        base_url: Some("http://127.0.0.1:11434/v1".into()),
        allowed_ips: vec!["127.0.0.1".into()],
        router_providers: vec![],
        zero_data_retention: false,
    };
    assert!(
        store
            .admit_personal_compatible_config(
                &other,
                local.base_url.clone().unwrap(),
                local.allowed_ips.clone()
            )
            .await
            .is_err()
    );
    // No member chosen metadata/link-local/private allowlist can self-enroll.
    for base in [
        "https://169.254.169.254/v1",
        "https://127.0.0.1/v1",
        "https://10.0.0.1/v1",
        "https://[::ffff:127.0.0.1]/v1",
    ] {
        let ip = url::Url::parse(base)
            .unwrap()
            .host_str()
            .unwrap()
            .trim_matches(['[', ']'])
            .to_owned();
        assert!(
            store
                .admit_personal_compatible_config(&own, base.into(), vec![ip])
                .await
                .is_err()
        );
    }
    local.base_url = Some("https://model.example.test/v1".into());
    // An existing administrator PublicHttps provider with no exact allowlist
    // must not replace the owner's independently resolved public address pins.
    store
        .create_provider(
            &admin,
            ProviderChange {
                name: "Public shared fixture".into(),
                config: ProviderConfig {
                    allowed_ips: vec![],
                    ..local.clone()
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    local.allowed_ips = vec!["8.8.8.8".into()];
    let config = store
        .admit_personal_compatible_config(
            &own,
            local.base_url.clone().unwrap(),
            local.allowed_ips.clone(),
        )
        .await
        .unwrap();
    let row = store
        .create_personal_compatible(
            &own,
            ProviderChange {
                name: "Private endpoint".into(),
                config,
                enabled: true,
                credential: CredentialChange::Replace(Zeroizing::new(
                    "synthetic-private-bearer".into(),
                )),
            },
        )
        .await
        .unwrap();
    assert!(
        !store
            .list_providers(&admin)
            .await
            .unwrap()
            .providers
            .iter()
            .any(|p| p.id == row.id)
    );
    assert!(matches!(
        store.personal_compatible_providers(&other).await,
        Err(StoreError::Forbidden)
    ));
    assert!(
        store
            .personal_compatible_providers(&admin)
            .await
            .unwrap()
            .providers
            .is_empty()
    );
    assert!(matches!(
        store
            .probe_binding(&admin, row.id.clone(), 1, "fixture".into(), None)
            .await,
        Err(StoreError::NotFound)
    ));
    let choice = store
        .record_model_probe(&own, row.id.clone(), 1, "fixture".into(), None)
        .await
        .unwrap();
    assert_eq!(choice.source, ModelSource::PersonalCompatible);
    let s = ModelSelection {
        source: choice.source,
        provider_id: row.id.clone(),
        provider_revision: 1,
        model: choice.model.clone(),
        effort: None,
    };
    assert!(
        store
            .set_model_preference(&other, Some(s.clone()))
            .await
            .is_err()
    );
    store
        .set_model_preference(&own, Some(s.clone()))
        .await
        .unwrap();
    assert!(store.set_default_model(&admin, Some(s)).await.is_err());
    let tool_choice = store
        .record_tool_probe(&own, row.id.clone(), 1, choice.model.clone(), None)
        .await
        .unwrap();
    let id = row.id.clone();
    let t = own.tenant.0.clone();
    let a = own.account.id.clone();
    let oa = other.account.id.clone();
    let ot = other.tenant.0.clone();
    store.call(move|db|{
        assert!(binding_for_choice(&db.conn,&db.provider_key,&choice).is_err());
        assert!(binding_for_owner_choice(&db.conn,&db.provider_key,&ot,&oa,&choice).is_err());
        let binding=binding_for_owner_choice(&db.conn,&db.provider_key,&t,&a,&choice)?;
        assert_eq!(binding.credential.as_deref().map(|s|s.as_str()),Some("synthetic-private-bearer"));
        crate::missions::require_model(&db.conn, &db.provider_key, &t, &a, &tool_choice)?;
        assert!(crate::missions::require_model(&db.conn, &db.provider_key, &ot, &oa, &tool_choice).is_err());
        assert_eq!(crate::missions::require_model(&db.conn, &db.provider_key, &t, &a, &choice), Err(StoreError::ToolsRequired));
        db.conn.execute("UPDATE provider_model_probes SET function_tools=0 WHERE provider_id=?1", [&id])?;
        assert!(binding_for_owner_choice(&db.conn,&db.provider_key,&t,&a,&tool_choice).is_err());
        assert!(binding_for_owner_choice(&db.conn,&db.provider_key,&t,&a,&choice).is_ok());
        let cipher:Vec<u8>=db.conn.query_row("SELECT credential FROM provider_revisions WHERE provider_id=?1",[&id],|r|r.get(0))?;
        assert!(!cipher.windows(b"synthetic-private-bearer".len()).any(|w|w==b"synthetic-private-bearer"));
        assert!(db.conn.execute("UPDATE personal_compatible_providers SET account_id=?1 WHERE provider_id=?2",params![oa,id]).is_err());
        crate::accounts::mark_deleted(&db.conn,&a,99)?;
        crate::accounts::finish_deletions(&mut db.conn)?;
        assert_eq!(db.conn.query_row("SELECT count(*) FROM provider_revisions WHERE provider_id=?1 AND credential IS NOT NULL",[&id],|r|r.get::<_,i64>(0))?,0);
        assert_eq!(db.conn.query_row("SELECT count(*) FROM provider_model_probes WHERE provider_id=?1",[&id],|r|r.get::<_,i64>(0))?,0);
        let (name,config,enabled):(String,String,bool)=db.conn.query_row("SELECT name,config,enabled FROM provider_revisions WHERE provider_id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        assert_eq!(name,"Deleted personal connection");assert!(!config.contains("model.example.test"));assert!(!enabled);
        assert!(owner(&db.conn,&id)?.is_some());assert!(!list(&db.conn)?.providers.iter().any(|p|p.id==id));
        Ok(())
    }).await.unwrap();
}
