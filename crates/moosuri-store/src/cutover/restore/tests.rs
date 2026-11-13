use super::*;
use crate::{
    Store,
    tests::{PASSWORD, TEST_LOCK, admin, member},
};
use moosuri_core::*;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};
fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}
struct Fixture {
    _temp: tempfile::TempDir,
    data: PathBuf,
    kit: PathBuf,
    cipher: PathBuf,
    head: PathBuf,
    identity: PathBuf,
    dest: PathBuf,
    account: String,
    other: String,
    memory: String,
    retained: String,
}
async fn fixture() -> Fixture {
    let temp = private();
    let data = temp.path().join("source");
    let kit = temp.path().join("kit");
    fs::DirBuilder::new().mode(0o700).create(&kit).unwrap();
    let store = Store::open(&data).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "other@example.test").await.user;
    let provider = store
        .create_provider(
            &a,
            crate::ProviderChange {
                name: "Restore synthetic".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: crate::CredentialChange::Replace(Zeroizing::new(
                    "synthetic-credential".into(),
                )),
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(
            &a,
            provider.id.clone(),
            provider.revision,
            "synthetic".into(),
            None,
        )
        .await
        .unwrap();
    store
        .record_tool_probe(
            &a,
            provider.id.clone(),
            provider.revision,
            "synthetic".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &a,
            Some(ModelSelection {
                provider_id: provider.id,
                provider_revision: provider.revision,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&a, "Old queued chat".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &a,
            thread.id,
            Uuid::new_v4().to_string(),
            "Do not redispatch after restore".into(),
        )
        .await
        .unwrap();
    let collection = store
        .create_collection(&a, "Restored source".into(), Uuid::new_v4().to_string())
        .await
        .unwrap();
    let source = store
        .ingest_text(
            &a,
            collection.id.clone(),
            IngestBatch {
                request_id: Uuid::new_v4().to_string(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: "source".into(),
                    version: "v1".into(),
                    expected_revision: None,
                    title: "Source".into(),
                    text: "Restored current source".into(),
                    speaker: None,
                    observed_at: 1,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap()
        .source_ids[0]
        .clone();
    let draft = |text: &str| MemoryDraft {
        text: text.into(),
        subject: "test".into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    };
    let memory = store
        .create_memory(
            &a,
            Uuid::new_v4().to_string(),
            draft("Withdrawn private canary"),
        )
        .await
        .unwrap();
    store.forget_memory(&a, memory.id.clone(), 1).await.unwrap();
    let retained = store
        .create_memory(
            &b,
            Uuid::new_v4().to_string(),
            draft("Other tenant retained"),
        )
        .await
        .unwrap();
    let account = a.account().id.clone();
    let other = b.account().id.clone();
    let aa = account.clone();
    let bb = other.clone();
    let tenant = a.tenant.0.clone();
    store.call(move|db|{
  db.conn.execute("INSERT INTO source_connections(tenant_id,id,account_id,collection_id,kind,name,scope,remote_account,auth_epoch,sync_enabled,poll_interval,credential,token_hash) VALUES(?1,'synthetic-source',?2,?3,'custom_http','Synthetic','scope','remote',0,1,60,X'01',zeroblob(32))",params![tenant,aa,collection.id])?;
  db.conn.execute("INSERT INTO automatic_memory_grants VALUES(?1,?2,?3,1,1,'owner','owner',0,'synthetic',1,100,100)",params![tenant,collection.id,aa])?;
  db.conn.execute("INSERT INTO automatic_memory_jobs(tenant_id,id,collection_id,source_id,source_revision,grant_revision,state,model,created_at) VALUES(?1,'synthetic-job',?2,?3,1,1,'queued','synthetic',1)",params![tenant,collection.id,source])?;
  db.conn.execute("INSERT INTO automatic_memory_budget VALUES(?1,?2,1,1,10,10)",params![tenant,aa])?;
  db.conn.execute("INSERT INTO model_call_receipts(tenant_id,account_id,run_id,attempt,step,nonce,usage,dispatched_at) VALUES(?1,?2,'historical-receipt',1,0,zeroblob(32),'{}',1)",params![tenant,aa])?;
  // Exercise final account lifecycle columns without importing the unrelated worker.
  let has_status:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('accounts') WHERE name='status')",[],|r|r.get(0))?;
  if !has_status {db.conn.execute_batch("ALTER TABLE accounts ADD COLUMN status TEXT NOT NULL DEFAULT 'active'; ALTER TABLE accounts ADD COLUMN management_revision INTEGER NOT NULL DEFAULT 1;")?;}
  db.conn.execute("UPDATE accounts SET status='suspended' WHERE id=?1",[bb])?;
  db.conn.execute("INSERT INTO mail_connections(tenant_id,account_id,id,email,status,credential,expires_at,scopes) VALUES(?1,?2,'synthetic','other@example.test','active',X'01',1,'[]')",params![tenant,aa])?;
  for (n,state) in [(1,"pending_approval"),(2,"approved"),(3,"dispatch_committed"),(4,"sent")] { db.conn.execute("INSERT INTO mail_actions(tenant_id,account_id,id,revision,connection_id,connection_epoch,auth_epoch,content,raw,digest,message_id,state,expires_at) VALUES(?1,?2,?3,1,'synthetic',1,0,'private draft',X'01','digest','message',?4,1)",params![tenant,aa,format!("action-{n}"),state])?; }
  Ok(())
 }).await.unwrap();
    drop(store);
    let identity = kit.join("identity");
    let recipient = backup::keygen(&identity).unwrap();
    backup::setup(&data, &recipient, &identity).unwrap();
    let cipher = kit.join("checkpoint.age");
    let heads = temp.path().join("heads");
    fs::DirBuilder::new().mode(0o700).create(&heads).unwrap();
    let head = heads.join("head.json");
    checkpoint(&data, &cipher, &head).unwrap();
    let dest = temp.path().join("restored");
    Fixture {
        _temp: temp,
        data,
        kit,
        cipher,
        head,
        identity,
        dest,
        account,
        other,
        memory: memory.id,
        retained: retained.id,
    }
}
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    for e in fs::read_dir(root).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_file() {
            out.insert(
                e.file_name().to_string_lossy().to_string(),
                fs::read(e.path()).unwrap(),
            );
        }
    }
    out
}
fn unpack(f: &Fixture) -> Vec<(String, Vec<u8>)> {
    let id = identity(&f.identity).unwrap();
    let b = age::decrypt(&id, &fs::read(&f.cipher).unwrap()).unwrap();
    tar::Archive::new(b.as_slice())
        .entries()
        .unwrap()
        .map(|e| {
            let mut e = e.unwrap();
            let name = e.path().unwrap().to_string_lossy().to_string();
            let mut b = vec![];
            e.read_to_end(&mut b).unwrap();
            (name, b)
        })
        .collect()
}
fn repack(
    f: &Fixture,
    items: &[(String, Vec<u8>)],
    tweak: impl FnOnce(&mut Vec<u8>),
) -> (PathBuf, PathBuf) {
    let mut tar = tar::Builder::new(vec![]);
    for (n, b) in items {
        let mut h = tar::Header::new_ustar();
        h.set_path(n).unwrap();
        h.set_mode(0o600);
        h.set_uid(0);
        h.set_gid(0);
        h.set_mtime(0);
        h.set_size(b.len() as u64);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_cksum();
        tar.append(&h, b.as_slice()).unwrap();
    }
    tar.finish().unwrap();
    let mut plain = tar.into_inner().unwrap();
    tweak(&mut plain);
    let id = identity(&f.identity).unwrap();
    let cipher = age::encrypt(&id.to_public(), &plain).unwrap();
    let manifest = decode_manifest(&items[0].1).unwrap();
    let mut h = head(
        &manifest,
        &Entry {
            name: String::new(),
            size: cipher.len() as u64,
            sha256: hash(&cipher),
        },
    )
    .unwrap();
    h.ciphertext_size = cipher.len() as u64;
    h.ciphertext_sha256 = hash(&cipher);
    let c = f.kit.join(format!("{}.age", Uuid::new_v4()));
    let p = f.kit.join(format!("{}.json", Uuid::new_v4()));
    let mut out = files::create_file(&c).unwrap();
    out.write_all(&cipher).unwrap();
    let mut out = files::create_file(&p).unwrap();
    out.write_all(&encode(&h, 8192).unwrap()).unwrap();
    (c, p)
}
#[tokio::test]
async fn exact_restore_requires_enrollment_preserves_tenants_withdrawal_and_quarantines_external_work()
 {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    let source = tree(&f.data);
    let inputs = tree(&f.kit);
    let old = decode_head(&fs::read(&f.head).unwrap()).unwrap();
    let summary = restore(&f.cipher, &f.head, &f.identity, &f.dest).unwrap();
    assert_ne!(summary.installation_id, old.installation_id);
    assert_eq!(summary.accounts.len(), 2);
    assert_eq!(source, tree(&f.data));
    assert_eq!(inputs, tree(&f.kit));
    assert!(Store::open(&f.data).is_err());
    let store = Store::open(&f.dest).unwrap();
    assert!(
        store
            .login("admin@example.test".into(), PASSWORD.into())
            .await
            .is_err()
    );
    let account = f.account.clone();
    let other = f.other.clone();
    store.call(move|db|{let (locked,epoch,revision):(bool,i64,i64)=db.conn.query_row("SELECT recovery_required,auth_epoch,management_revision FROM accounts WHERE id=?1",[account],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;assert!(locked);assert!(epoch>0);assert_eq!(revision,2);assert_eq!(db.conn.query_row("SELECT status FROM accounts WHERE id=?1",[other],|r|r.get::<_,String>(0))?,"suspended");assert_eq!(db.conn.query_row("SELECT count(*) FROM sessions",[],|r|r.get::<_,i64>(0))?,0);assert_eq!(db.conn.query_row("SELECT count(*) FROM mail_actions WHERE state IN('pending_approval','approved','dispatch_committed')",[],|r|r.get::<_,i64>(0))?,0);assert_eq!(db.conn.query_row("SELECT state FROM mail_actions WHERE id='action-3'",[],|r|r.get::<_,String>(0))?,"outcome_unknown");assert_eq!(db.conn.query_row("SELECT state FROM mail_actions WHERE id='action-4'",[],|r|r.get::<_,String>(0))?,"sent");assert_eq!(db.conn.query_row("SELECT count(*) FROM mail_connections WHERE credential IS NOT NULL OR status='active'",[],|r|r.get::<_,i64>(0))?,0);Ok(())}).await.unwrap();
    assert!(store.claim_agent_run().await.unwrap().is_none());
    store.call(|db|{for sql in ["SELECT count(*) FROM provider_revisions WHERE credential IS NOT NULL","SELECT count(*) FROM provider_model_probes","SELECT count(*) FROM source_connections WHERE sync_enabled=1 OR token_hash IS NOT NULL OR credential IS NOT NULL","SELECT count(*) FROM automatic_memory_grants WHERE enabled=1","SELECT count(*) FROM automatic_memory_jobs WHERE state IN('queued','acquired','dispatched')"] {assert_eq!(db.conn.query_row(sql,[],|r|r.get::<_,i64>(0))?,0)}assert_eq!(db.conn.query_row("SELECT count(*) FROM model_call_receipts",[],|r|r.get::<_,i64>(0))?,1);assert_eq!(db.conn.query_row("SELECT calls FROM automatic_memory_budget",[],|r|r.get::<_,i64>(0))?,1);Ok(())}).await.unwrap();
    drop(store);
    crate::recover_account(
        &f.dest,
        f.account.clone(),
        Role::Admin,
        "New-temporary-password".into(),
    )
    .await
    .unwrap();
    let store = Store::open(&f.dest).unwrap();
    let restricted = store
        .login("admin@example.test".into(), "New-temporary-password".into())
        .await
        .unwrap();
    assert!(restricted.user.restricted);
    let login = store
        .change_password(&restricted.user, "New-restored-password".into())
        .await
        .unwrap();
    assert!(matches!(
        store.memory(&login.user, f.memory.clone(), None).await,
        Err(crate::StoreError::NotFound)
    ));
    drop(store);
    crate::recover_account(
        &f.dest,
        f.other.clone(),
        Role::Member,
        "Other-new-temporary-password".into(),
    )
    .await
    .unwrap();
    let suspended = Store::open(&f.dest).unwrap();
    assert!(matches!(
        suspended
            .login(
                "other@example.test".into(),
                "Other-new-temporary-password".into()
            )
            .await,
        Err(crate::StoreError::Unauthorized)
    ));
    drop(suspended);
    let conn = Connection::open(f.dest.join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("SELECT status FROM accounts WHERE id=?1", [&f.other], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "suspended"
    );
    assert_eq!(
        conn.query_row(
            "SELECT json_extract(draft,'$.text') FROM knowledge_memory_versions WHERE memory_id=?1",
            [f.retained],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "Other tenant retained"
    );
    drop(conn);
    assert!(
        restore(&f.cipher, &f.head, &f.identity, &f.dest)
            .unwrap()
            .already_published
    );
}
#[tokio::test]
async fn malformed_or_unpinned_inputs_never_publish_and_occupied_root_is_unchanged() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    let originals = tree(&f.kit);
    let missing = f.kit.join("missing");
    assert!(restore(&f.cipher, &missing, &f.identity, &f.dest).is_err());
    assert!(!f.dest.exists());
    let wrong = f.kit.join("wrong-identity");
    backup::keygen(&wrong).unwrap();
    assert!(restore(&f.cipher, &f.head, &wrong, &f.dest).is_err());
    let b = fs::read(&f.cipher).unwrap();
    for bytes in [b[..b.len() / 2].to_vec(), {
        let mut v = b.clone();
        v[100] ^= 1;
        v
    }] {
        let p = f.kit.join(format!("{}.age", Uuid::new_v4()));
        files::create_file(&p).unwrap().write_all(&bytes).unwrap();
        assert!(restore(&p, &f.head, &f.identity, &f.dest).is_err());
        assert!(!f.dest.exists());
    }
    let items = unpack(&f);
    for case in 0..6 {
        let (c, h) = repack(&f, &items, |plain| match case {
            0 => plain.push(1),
            1 => plain.truncate(plain.len() - 512),
            2 => {
                plain[156] = b'2';
            }
            3 => {
                plain[0] = b'/';
            }
            4 => {
                plain[512] ^= 1;
            }
            _ => {
                let n = plain.len() - 1;
                plain[n] = 1;
            }
        });
        assert!(
            restore(&c, &h, &f.identity, &f.dest).is_err(),
            "case {case}"
        );
        assert!(!f.dest.exists());
    }
    fs::DirBuilder::new().mode(0o700).create(&f.dest).unwrap();
    files::create_file(&f.dest.join("occupied"))
        .unwrap()
        .write_all(b"keep")
        .unwrap();
    let before = tree(&f.dest);
    assert!(restore(&f.cipher, &f.head, &f.identity, &f.dest).is_err());
    assert_eq!(before, tree(&f.dest));
    for (name, b) in originals {
        assert_eq!(fs::read(f.kit.join(name)).unwrap(), b)
    }
}
struct Fault(PathBuf, String);
impl Fault {
    fn at(p: &Path, s: &str) -> Self {
        super::super::FAULTS
            .lock()
            .unwrap()
            .push((p.into(), s.into()));
        Self(p.into(), s.into())
    }
}
impl Drop for Fault {
    fn drop(&mut self) {
        super::super::FAULTS
            .lock()
            .unwrap()
            .retain(|(p, s)| p != &self.0 || s != &self.1)
    }
}
#[tokio::test]
async fn every_restore_boundary_retries_without_partial_admission_or_duplicate_publication() {
    let _g = TEST_LOCK.lock().await;
    for point in [
        "restore_cipher",
        "restore_extract",
        "restore_verified",
        "restore_quarantine",
        "restore_publish",
        "restore_after_rename",
    ] {
        let f = fixture().await;
        let fault = Fault::at(&f.dest, point);
        assert!(
            restore(&f.cipher, &f.head, &f.identity, &f.dest).is_err(),
            "{point}"
        );
        if f.dest.exists() {
            assert_eq!(point, "restore_after_rename");
            let sync_failure = Fault::at(&f.dest, "admission_parent_sync");
            let before = tree(&f.dest);
            assert!(matches!(
                Store::open(&f.dest),
                Err(crate::StoreError::Unavailable)
            ));
            assert_eq!(tree(&f.dest), before);
            drop(sync_failure);
            let s = Store::open(&f.dest).unwrap();
            drop(s)
        }
        drop(fault);
        let result = restore(&f.cipher, &f.head, &f.identity, &f.dest).unwrap();
        assert_eq!(result.already_published, point == "restore_after_rename");
        assert!(Store::open(&f.data).is_err());
    }
}

#[tokio::test]
async fn late_publication_sync_failure_fences_all_admission_without_mutating_restored_root() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    let late = Fault::at(&f.dest, "restore_after_rename");
    assert!(matches!(
        restore(&f.cipher, &f.head, &f.identity, &f.dest),
        Err(Error::Unavailable)
    ));
    drop(late);
    assert!(f.dest.is_dir());
    let before = tree(&f.dest);
    let before_source = tree(&f.data);
    let before_kit = tree(&f.kit);
    let modes = |path: &Path| {
        let mut result = BTreeMap::new();
        result.insert(
            String::new(),
            fs::metadata(path).unwrap().permissions().mode(),
        );
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            result.insert(
                entry.file_name().to_string_lossy().to_string(),
                entry.metadata().unwrap().permissions().mode(),
            );
        }
        result
    };
    let before_modes = modes(&f.dest);
    let fault = Fault::at(&f.dest, "admission_parent_sync");
    assert!(matches!(
        Store::open(&f.dest),
        Err(crate::StoreError::Unavailable)
    ));
    assert_eq!(tree(&f.dest), before);
    assert!(matches!(
        crate::recover_account(
            &f.dest,
            f.account.clone(),
            Role::Admin,
            "Must-not-be-installed".into()
        )
        .await,
        Err(crate::StoreError::Unavailable)
    ));
    assert_eq!(tree(&f.dest), before);
    let recipient = identity(&f.identity).unwrap().to_public().to_string();
    assert!(matches!(
        backup::setup(&f.dest, &recipient, &f.identity),
        Err(Error::Unavailable)
    ));
    assert_eq!(tree(&f.dest), before);
    assert!(matches!(
        backup::create(&f.dest, &f.kit.join("must-not-export.age")),
        Err(Error::Unavailable)
    ));
    assert_eq!(tree(&f.dest), before);
    assert!(matches!(
        checkpoint(
            &f.dest,
            &f.kit.join("must-not-seal.age"),
            &f.head.with_file_name("must-not-publish.json")
        ),
        Err(Error::Unavailable)
    ));
    assert_eq!(tree(&f.dest), before);
    assert!(matches!(
        restore(&f.cipher, &f.head, &f.identity, &f.dest),
        Err(Error::Unavailable)
    ));
    assert_eq!(tree(&f.dest), before);
    assert_eq!(modes(&f.dest), before_modes);
    assert_eq!(tree(&f.data), before_source);
    assert_eq!(tree(&f.kit), before_kit);
    // A failed barrier never leaves a held lock or requires re-quarantine.
    drop(fault);
    assert!(
        restore(&f.cipher, &f.head, &f.identity, &f.dest)
            .unwrap()
            .already_published
    );
    assert_eq!(tree(&f.dest), before);
    crate::recover_account(
        &f.dest,
        f.account,
        Role::Admin,
        "Fresh-admission-password".into(),
    )
    .await
    .unwrap();
    let store = Store::open(&f.dest).unwrap();
    assert!(
        store
            .login(
                "admin@example.test".into(),
                "Fresh-admission-password".into()
            )
            .await
            .unwrap()
            .user
            .restricted
    );
    assert_eq!(tree(&f.data), before_source);
}
#[tokio::test]
async fn abrupt_restore_exit_keeps_stages_unusable_and_same_inputs_retry() {
    if let Some(root) = std::env::var_os("MOOSURI_CUTOVER_EXIT_ROOT") {
        restore(
            Path::new(&std::env::var_os("RESTORE_CIPHER").unwrap()),
            Path::new(&std::env::var_os("RESTORE_HEAD").unwrap()),
            Path::new(&std::env::var_os("RESTORE_IDENTITY").unwrap()),
            Path::new(&root),
        )
        .unwrap();
        panic!("must exit")
    }
    let _g = TEST_LOCK.lock().await;
    for point in [
        "restore_verified",
        "restore_quarantine",
        "restore_publish",
        "restore_after_rename",
    ] {
        let f = fixture().await;
        let out=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","cutover::restore::tests::abrupt_restore_exit_keeps_stages_unusable_and_same_inputs_retry","--nocapture"]).env("MOOSURI_CUTOVER_EXIT_ROOT",&f.dest).env("MOOSURI_CUTOVER_EXIT_POINT",point).env("RESTORE_CIPHER",&f.cipher).env("RESTORE_HEAD",&f.head).env("RESTORE_IDENTITY",&f.identity).output().unwrap();
        assert_eq!(
            out.status.code(),
            Some(73),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        for entry in fs::read_dir(f.dest.parent().unwrap()).unwrap() {
            let p = entry.unwrap().path();
            if p.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(STAGE_PREFIX)
            {
                let before = tree(&p);
                assert!(Store::open(&p).is_err());
                assert_eq!(before, tree(&p));
            }
        }
        let result = restore(&f.cipher, &f.head, &f.identity, &f.dest).unwrap();
        assert_eq!(result.already_published, point == "restore_after_rename");
        assert!(Store::open(&f.data).is_err());
    }
}

#[tokio::test]
async fn supported_predecessor_migrates_offline_and_repeated_restore_keeps_journal_lineage() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    let mut items = unpack(&f);
    let mut manifest = decode_manifest(&items[0].1).unwrap();
    manifest.seal.schema = SCHEMA;
    let db = f.kit.join("predecessor.sqlite");
    files::create_file(&db)
        .unwrap()
        .write_all(&items[1].1)
        .unwrap();
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch("DROP TRIGGER installation_restores_no_update;DROP TRIGGER installation_restores_no_delete;DROP TABLE installation_restores;DROP TRIGGER installation_cutover_monotonic;").unwrap();
    conn.execute(
        "UPDATE installation_cutover SET seal=?1",
        [String::from_utf8(encode(&manifest.seal, 16384).unwrap()).unwrap()],
    )
    .unwrap();
    conn.execute_batch(&format!("CREATE TRIGGER installation_cutover_monotonic BEFORE UPDATE ON installation_cutover WHEN NOT ((OLD.phase='active' AND NEW.phase='sealed' AND NEW.singleton=1) OR (OLD.phase='sealed' AND NEW.phase='checkpoint_ready' AND NEW.singleton=1 AND NEW.seal=OLD.seal)) BEGIN SELECT RAISE(ABORT,'cutover transition forbidden'); END; PRAGMA user_version={SCHEMA};")).unwrap();
    drop(conn);
    items[1].1 = fs::read(db).unwrap();
    items[5].1 = encode(&manifest.seal, 16384).unwrap();
    manifest.seal_sha256 = hash(&items[5].1);
    for (entry, (_, bytes)) in manifest.entries.iter_mut().zip(items.iter().skip(1)) {
        entry.sha256 = hash(bytes);
        entry.size = bytes.len() as u64;
    }
    items[0].1 = encode(&manifest, 32768).unwrap();
    let (cipher, head) = repack(&f, &items, |_| {});
    let first = restore(&cipher, &head, &f.identity, &f.dest).unwrap();
    let conn = Connection::open(f.dest.join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        crate::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM installation_restores", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    drop(conn);
    backup::setup(
        &f.dest,
        &identity(&f.identity).unwrap().to_public().to_string(),
        &f.identity,
    )
    .unwrap();
    let second_cipher = f.kit.join("second.age");
    let second_head = f.head.parent().unwrap().join("second.json");
    checkpoint(&f.dest, &second_cipher, &second_head).unwrap();
    let next = f.dest.with_file_name("restored-again");
    let second = restore(&second_cipher, &second_head, &f.identity, &next).unwrap();
    assert_ne!(first.installation_id, second.installation_id);
    let s = Store::open(&next).unwrap();
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM installation_restores", [], |r| r
                    .get::<_, i64>(0))?,
            2
        );
        Ok(())
    })
    .await
    .unwrap();
    assert!(Store::open(&f.dest).is_err());
}

#[tokio::test]
async fn authenticated_eof_and_verified_db_journal_disagreements_fail_before_admission() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    let original = fs::read(&f.cipher).unwrap();
    for bytes in [
        original[..original.len() - 1].to_vec(),
        {
            let mut b = original.clone();
            let n = b.len() - 8;
            b[n] ^= 1;
            b
        },
        {
            let mut b = original.clone();
            b.push(0);
            b
        },
    ] {
        let cipher = f.kit.join(format!("{}.age", Uuid::new_v4()));
        files::create_file(&cipher)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        let mut h = decode_head(&fs::read(&f.head).unwrap()).unwrap();
        h.ciphertext_size = bytes.len() as u64;
        h.ciphertext_sha256 = hash(&bytes);
        let path = f.kit.join(format!("{}.json", Uuid::new_v4()));
        files::create_file(&path)
            .unwrap()
            .write_all(&encode(&h, 8192).unwrap())
            .unwrap();
        assert!(matches!(
            restore(&cipher, &path, &f.identity, &f.dest),
            Err(Error::DecryptionFailed)
        ));
        assert!(!f.dest.exists());
    }
    for sql in [
        "UPDATE store_meta SET journal_seq=journal_seq+1",
        "PRAGMA user_version=99",
        "DROP TRIGGER installation_cutover_monotonic;UPDATE installation_cutover SET phase='active',seal=NULL,checkpoint=NULL",
    ] {
        let mut items = unpack(&f);
        let db = f.kit.join(format!("{}.sqlite", Uuid::new_v4()));
        files::create_file(&db)
            .unwrap()
            .write_all(&items[1].1)
            .unwrap();
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(sql).unwrap();
        drop(conn);
        items[1].1 = fs::read(db).unwrap();
        let mut manifest = decode_manifest(&items[0].1).unwrap();
        manifest.entries[0].size = items[1].1.len() as u64;
        manifest.entries[0].sha256 = hash(&items[1].1);
        items[0].1 = encode(&manifest, 32768).unwrap();
        let (cipher, head) = repack(&f, &items, |_| {});
        assert!(
            restore(&cipher, &head, &f.identity, &f.dest).is_err(),
            "{sql}"
        );
        assert!(!f.dest.exists());
    }
    for path in [&f.cipher, &f.head, &f.identity] {
        let missing = f.kit.join("missing");
        let (cipher, head, id) = if path == &f.cipher {
            (&missing, &f.head, &f.identity)
        } else if path == &f.head {
            (&f.cipher, &missing, &f.identity)
        } else {
            (&f.cipher, &f.head, &missing)
        };
        assert!(restore(cipher, head, id, &f.dest).is_err());
        assert!(!f.dest.exists());
    }
    let mut h = decode_head(&fs::read(&f.head).unwrap()).unwrap();
    h.seal_id = Uuid::new_v4().to_string();
    let wrong = f.kit.join("wrong-valid-head.json");
    files::create_file(&wrong)
        .unwrap()
        .write_all(&encode(&h, 8192).unwrap())
        .unwrap();
    assert!(restore(&f.cipher, &wrong, &f.identity, &f.dest).is_err());
    assert!(!f.dest.exists());
}

#[tokio::test]
async fn independent_review_retry_preserves_enrollment_and_subsequent_current_content() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    restore(&f.cipher, &f.head, &f.identity, &f.dest).unwrap();
    crate::recover_account(
        &f.dest,
        f.account.clone(),
        Role::Admin,
        "Independent-new-enrollment".into(),
    )
    .await
    .unwrap();
    let store = Store::open(&f.dest).unwrap();
    let first = store
        .login(
            "admin@example.test".into(),
            "Independent-new-enrollment".into(),
        )
        .await
        .unwrap();
    let current = store
        .change_password(&first.user, "Independent-current-password".into())
        .await
        .unwrap()
        .user;
    let added = store
        .create_memory(
            &current,
            Uuid::new_v4().to_string(),
            MemoryDraft {
                text: "Created after restore, must survive retry".into(),
                subject: "Synthetic retry".into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![],
            },
        )
        .await
        .unwrap();
    drop(store);
    let before = tree(&f.dest);
    let retry = restore(&f.cipher, &f.head, &f.identity, &f.dest).unwrap();
    assert!(retry.already_published);
    assert_eq!(
        tree(&f.dest),
        before,
        "retry must not mutate current destination files"
    );
    let store = Store::open(&f.dest).unwrap();
    let login = store
        .login(
            "admin@example.test".into(),
            "Independent-current-password".into(),
        )
        .await
        .unwrap();
    assert!(!login.user.restricted);
    assert!(store.memory(&login.user, added.id, None).await.is_ok());
    assert!(
        store
            .login("admin@example.test".into(), PASSWORD.into())
            .await
            .is_err()
    );
}
