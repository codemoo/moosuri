use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};
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
async fn role_restrictions_revocation_personal_selection_and_safe_retarget() {
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
    for id in [&p.id, &q.id] {
        store
            .record_model_probe(&admin.user, id.clone(), 1, "fixture-model".into(), None)
            .await
            .unwrap();
    }
    store
        .set_default_model(&admin.user, Some(selection(&p.id, "fixture-model")))
        .await
        .unwrap();
    store
        .set_model_preference(&a.user, Some(selection(&q.id, "fixture-model")))
        .await
        .unwrap();
    assert_eq!(store.resolve_model(&a.user).await.unwrap().id, q.id);
    assert_eq!(store.resolve_model(&admin.user).await.unwrap().id, p.id);
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
    store.set_model_preference(&a.user, None).await.unwrap();
    assert_eq!(store.resolve_model(&a.user).await.unwrap().id, p.id);
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
                2
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
