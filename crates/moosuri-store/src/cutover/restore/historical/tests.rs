use super::*;
use crate::{
    Store,
    tests::{PASSWORD, TEST_LOCK, admin, member},
};
use moosuri_core::{AccountManagementAction, AccountManagementInput, MemoryDraft};
use std::os::unix::fs::PermissionsExt;
fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}
async fn stopped(store: Store) {
    drop(store);
    tokio::time::sleep(Duration::from_millis(100)).await;
}
fn draft(text: &str) -> MemoryDraft {
    MemoryDraft {
        subject: "Synthetic recovery".into(),
        text: text.into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    }
}
#[tokio::test]
async fn historical_export_latest_deletes_rotation_and_source_continuity() {
    let _serial = TEST_LOCK.lock().await;
    let temp = private();
    let root = temp.path().join("source");
    let kit = temp.path().join("kit");
    let out = temp.path().join("out");
    for d in [&kit, &out] {
        fs::DirBuilder::new().mode(0o700).create(d).unwrap();
    }
    let key = kit.join("old.key");
    let next_key = kit.join("new.key");
    let store = Store::open(&root).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "other-recovery@example.test").await.user;
    let c = member(&store, &a, "deleted-recovery@example.test")
        .await
        .user;
    let deleted = store
        .create_memory(
            &c,
            Uuid::new_v4().to_string(),
            draft("Deleted account private fact"),
        )
        .await
        .unwrap();
    let deleted_account = c.account.id.clone();
    let forgotten = store
        .create_memory(
            &a,
            Uuid::new_v4().to_string(),
            draft("Withdraw this old fact"),
        )
        .await
        .unwrap();
    let retained = store
        .create_memory(
            &b,
            Uuid::new_v4().to_string(),
            draft("Retain the other tenant"),
        )
        .await
        .unwrap();
    let owner = a.account.id.clone();
    let other = b.account.id.clone();
    stopped(store).await;
    let recipient = backup::keygen(&key).unwrap();
    backup::setup(&root, &recipient, &key).unwrap();
    let export = out.join("old.age");
    backup::create(&root, &export).unwrap();
    let first = out.join("first.age");
    let head1 = kit.join("first.json");
    let p1 = publish_journal(&root, &export, &first, &head1).unwrap();
    let store = Store::open(&root).unwrap();
    let a = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    store
        .forget_memory(&a, forgotten.id.clone(), 1)
        .await
        .unwrap();
    let revision = store
        .managed_accounts(&a)
        .await
        .unwrap()
        .into_iter()
        .find(|x| x.user.id == deleted_account)
        .unwrap()
        .revision;
    let prepared = store
        .prepare_account_management(
            &a,
            deleted_account.clone(),
            AccountManagementInput {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: revision,
                action: AccountManagementAction::Delete,
            },
        )
        .await
        .unwrap();
    store.apply_account_management(prepared).await.unwrap();
    // Target created after the snapshot is absent there; its UUID tombstone must
    // not make an otherwise valid historical restore fail.
    let late = store
        .create_memory(
            &a,
            Uuid::new_v4().to_string(),
            draft("Created after export"),
        )
        .await
        .unwrap();
    store.forget_memory(&a, late.id, 1).await.unwrap();
    stopped(store).await;
    let recipient = backup::keygen(&next_key).unwrap();
    backup::setup(&root, &recipient, &next_key).unwrap();
    let failed = out.join("head-failed.age");
    let failed_head = kit.join("head-failed.json");
    crate::cutover::FAULTS
        .lock()
        .unwrap()
        .push((root.clone(), "journal_before_head_publish".into()));
    assert!(publish_journal(&root, &export, &failed, &failed_head).is_err());
    crate::cutover::FAULTS
        .lock()
        .unwrap()
        .retain(|(r, _)| r != &root);
    assert!(failed.exists());
    assert!(!failed_head.exists());
    let latest = out.join("latest.age");
    let head = kit.join("latest.json");
    let p = publish_journal(&root, &export, &latest, &head).unwrap();
    // Neither publisher invocation permanently sealed or rewrote the source.
    let store = Store::open(&root).unwrap();
    assert!(
        store
            .login("admin@example.test".into(), PASSWORD.into())
            .await
            .is_ok()
    );
    stopped(store).await;
    let dest = temp.path().join("restored");
    assert!(
        restore_historical(
            &export,
            &first,
            &head1,
            &p.trusted_head_sha256,
            &key,
            &key,
            &dest
        )
        .is_err()
    );
    assert!(!dest.exists());
    assert!(
        restore_historical(
            &export,
            &latest,
            &head,
            &p.trusted_head_sha256,
            &key,
            &key,
            &dest
        )
        .is_err()
    );
    assert!(!dest.exists());
    let missing = kit.join("missing.key");
    assert!(
        restore_historical(
            &export,
            &latest,
            &head,
            &p.trusted_head_sha256,
            &missing,
            &next_key,
            &dest
        )
        .is_err()
    );
    assert!(!dest.exists());
    assert!(
        restore_historical(
            &export,
            &missing,
            &head,
            &p.trusted_head_sha256,
            &key,
            &next_key,
            &dest
        )
        .is_err()
    );
    assert!(!dest.exists());
    assert!(
        restore_historical(
            &export,
            &first,
            &head,
            &p.trusted_head_sha256,
            &key,
            &next_key,
            &dest
        )
        .is_err()
    );
    assert!(!dest.exists());
    crate::cutover::FAULTS
        .lock()
        .unwrap()
        .push((dest.clone(), "historical_after_rename".into()));
    assert!(
        restore_historical(
            &export,
            &latest,
            &head,
            &p.trusted_head_sha256,
            &key,
            &next_key,
            &dest
        )
        .is_err()
    );
    crate::cutover::FAULTS
        .lock()
        .unwrap()
        .retain(|(r, _)| r != &dest);
    assert!(dest.exists());
    let summary = restore_historical(
        &export,
        &latest,
        &head,
        &p.trusted_head_sha256,
        &key,
        &next_key,
        &dest,
    )
    .unwrap();
    assert_ne!(
        summary.installation_id,
        backup::configuration(&Connection::open(root.join("assistant.sqlite")).unwrap())
            .unwrap()
            .installation_id
    );
    let conn = Connection::open(dest.join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM knowledge_memory_versions WHERE memory_id=?1",
            [&forgotten.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM knowledge_memory_versions WHERE memory_id=?1",
            [&retained.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM knowledge_memory_versions WHERE memory_id=?1",
            [&deleted.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert!(
        conn.query_row(
            "SELECT deleted FROM accounts WHERE id=?1",
            [&deleted_account],
            |r| r.get::<_, bool>(0)
        )
        .unwrap()
    );
    assert!(summary.already_published);
    for id in [owner.clone(), other] {
        assert_eq!(
            conn.query_row(
                "SELECT role,recovery_required FROM accounts WHERE id=?1",
                [id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
            )
            .unwrap(),
            ("member".into(), true)
        );
    }
    assert!(
        conn.query_row("SELECT trusted_head FROM installation_restores", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap()
        .contains(PURPOSE)
    );
    drop(conn);
    assert!(
        restore_historical(
            &export,
            &latest,
            &head,
            &p.trusted_head_sha256,
            &key,
            &next_key,
            &dest
        )
        .unwrap()
        .already_published
    );
    // Old input/head remains valid only under its old pin; freshness is supplied
    // by the independently maintained record, never inferred from timestamp.
    assert_ne!(p1.trusted_head_sha256, p.trusted_head_sha256);
    let store = Store::open(&dest).unwrap();
    assert!(
        store
            .login("admin@example.test".into(), PASSWORD.into())
            .await
            .is_err()
    );
    stopped(store).await;
    crate::recover_account(
        &dest,
        owner,
        moosuri_core::Role::Admin,
        "Historical-new-enrollment-password".into(),
    )
    .await
    .unwrap();
    let store = Store::open(&dest).unwrap();
    let restricted = store
        .login(
            "admin@example.test".into(),
            "Historical-new-enrollment-password".into(),
        )
        .await
        .unwrap();
    let u = store
        .change_password(&restricted.user, "Historical-current-password".into())
        .await
        .unwrap()
        .user;
    let provider = store
        .create_provider(
            &u,
            crate::ProviderChange {
                name: "Synthetic new provider".into(),
                enabled: true,
                config: moosuri_core::ProviderConfig {
                    kind: moosuri_core::ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: crate::CredentialChange::Replace(Zeroizing::new(
                    "synthetic-new-key".into(),
                )),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .probe_binding(&u, provider.id, provider.revision, "synthetic".into(), None)
            .await,
        Err(crate::StoreError::Unavailable)
    ));
    assert!(matches!(
        store.usage_summary(&u).await,
        Err(crate::StoreError::Unavailable)
    ));
    assert!(matches!(
        store.account_usage(&u).await,
        Err(crate::StoreError::Unavailable)
    ));
    assert!(matches!(
        store
            .subscription_access(
                crate::SubscriptionAuthority::User(u.clone()),
                crate::SubscriptionRef {
                    tenant: u.tenant.0.clone(),
                    account: u.account.id.clone(),
                    id: Uuid::new_v4().to_string(),
                    revision: 1,
                    model: "synthetic".into(),
                    effort: None
                }
            )
            .await,
        Err(crate::StoreError::Unavailable)
    ));
    stopped(store).await;
    assert!(
        restore_historical(
            &export,
            &latest,
            &head,
            &p.trusted_head_sha256,
            &key,
            &next_key,
            &dest
        )
        .unwrap()
        .already_published
    );
    let store = Store::open(&dest).unwrap();
    assert!(
        store
            .login(
                "admin@example.test".into(),
                "Historical-current-password".into()
            )
            .await
            .is_ok()
    );
    stopped(store).await;
}

fn artifact(body: &str) -> moosuri_core::ArtifactWrite {
    moosuri_core::ArtifactWrite {
        request_id: Uuid::new_v4().to_string(),
        id: None,
        expected_revision: None,
        draft: moosuri_core::ArtifactDraft {
            title: "Synthetic historical document".into(),
            body: body.into(),
            payload: moosuri_core::ArtifactPayload::Document,
            references: vec![],
        },
    }
}
#[tokio::test]
async fn historical_restore_withdraws_older_artifact_revision_and_rejects_a_valid_fork() {
    let _serial = TEST_LOCK.lock().await;
    let temp = private();
    let root = temp.path().join("source");
    let fork = temp.path().join("fork");
    let out = temp.path().join("out");
    let kit = temp.path().join("kit");
    for d in [&out, &kit] {
        fs::DirBuilder::new().mode(0o700).create(d).unwrap();
    }
    let store = Store::open(&root).unwrap();
    let a = admin(&store).await.user;
    let doc = store
        .write_artifact(&a, artifact("Snapshot body"))
        .await
        .unwrap();
    let m1 = store
        .create_memory(&a, Uuid::new_v4().to_string(), draft("Primary branch fact"))
        .await
        .unwrap();
    let m2 = store
        .create_memory(&a, Uuid::new_v4().to_string(), draft("Other branch fact"))
        .await
        .unwrap();
    stopped(store).await;
    let key = kit.join("key");
    let recipient = backup::keygen(&key).unwrap();
    backup::setup(&root, &recipient, &key).unwrap();
    let old = out.join("old.age");
    backup::create(&root, &old).unwrap();
    fs::DirBuilder::new().mode(0o700).create(&fork).unwrap();
    for e in fs::read_dir(&root).unwrap() {
        let e = e.unwrap();
        files::copy_file(&e.path(), &fork.join(e.file_name()), backup::DB_LIMIT).unwrap();
    }
    let store = Store::open(&root).unwrap();
    let a = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    store.forget_memory(&a, m1.id.clone(), 1).await.unwrap();
    let mut edit = artifact("Later unbacked revision");
    edit.id = Some(doc.id.clone());
    edit.expected_revision = Some(1);
    let edited = store.write_artifact(&a, edit).await.unwrap();
    let deletion_request = Uuid::new_v4().to_string();
    store
        .delete_artifact(
            &a,
            doc.id.clone(),
            moosuri_core::ArtifactDelete {
                request_id: deletion_request.clone(),
                expected_revision: edited.revision,
            },
        )
        .await
        .unwrap();
    stopped(store).await;
    let newer = out.join("newer.age");
    backup::create(&root, &newer).unwrap();
    let journal = out.join("journal.age");
    let head = kit.join("head.json");
    let p = publish_journal(&root, &old, &journal, &head).unwrap();
    let dest = temp.path().join("restored");
    restore_historical(
        &old,
        &journal,
        &head,
        &p.trusted_head_sha256,
        &key,
        &key,
        &dest,
    )
    .unwrap();
    let conn = Connection::open(dest.join("assistant.sqlite")).unwrap();
    assert_eq!(conn.query_row("SELECT count(*) FROM personal_artifact_versions WHERE artifact_id=?1 AND draft IS NOT NULL",[&doc.id],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM personal_artifact_mutations WHERE request_id=?1",
            [&deletion_request],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    drop(conn);
    // Fork uses the same valid installation/header and its own fully committed
    // Store journal. An independently pinned fork cannot extend a snapshot whose
    // complete successor prefix already committed a different first record.
    let store = Store::open(&fork).unwrap();
    let a = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    store.forget_memory(&a, m2.id, 1).await.unwrap();
    stopped(store).await;
    let fj = out.join("fork.age");
    let fh = kit.join("fork.json");
    let fp = publish_journal(&fork, &newer, &fj, &fh).unwrap();
    let rejected = temp.path().join("fork-rejected");
    assert!(
        restore_historical(
            &newer,
            &fj,
            &fh,
            &fp.trusted_head_sha256,
            &key,
            &key,
            &rejected
        )
        .is_err()
    );
    assert!(!rejected.exists());
}
