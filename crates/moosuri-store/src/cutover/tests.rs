use super::*;
use crate::{
    Store, StoreError,
    tests::{PASSWORD, TEST_LOCK, admin, member},
};
use age::secrecy::ExposeSecret;
use moosuri_core::*;
use std::{
    collections::BTreeMap,
    os::unix::fs::{PermissionsExt, symlink},
};
fn directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("moosuri-cutover-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}
struct Fixture {
    _root: tempfile::TempDir,
    data: PathBuf,
    kit: PathBuf,
    output: PathBuf,
    head: PathBuf,
    identity: age::x25519::Identity,
    identity_file: PathBuf,
    user: crate::UserContext,
}
async fn fixture() -> Fixture {
    let root = directory();
    let data = root.path().join("data");
    let kit = root.path().join("kit");
    let out = root.path().join("cipher");
    let heads = root.path().join("heads");
    for p in [&kit, &out, &heads] {
        fs::create_dir(p).unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap()
    }
    let store = Store::open(&data).unwrap();
    let user = admin(&store).await.user;
    drop(store);
    let identity = age::x25519::Identity::generate();
    let identity_file = kit.join("identity");
    let mut f = files::create_file(&identity_file).unwrap();
    f.write_all(identity.to_string().expose_secret().as_bytes())
        .unwrap();
    f.sync_all().unwrap();
    backup::setup(&data, &identity.to_public().to_string(), &identity_file).unwrap();
    Fixture {
        _root: root,
        data,
        kit,
        output: out.join("checkpoint.age"),
        head: heads.join("head.json"),
        identity,
        identity_file,
        user,
    }
}
struct Fault(PathBuf, String);
impl Fault {
    fn at(root: &Path, point: &str) -> Self {
        FAULTS
            .lock()
            .unwrap()
            .push((root.to_path_buf(), point.into()));
        Self(root.to_path_buf(), point.into())
    }
}
impl Drop for Fault {
    fn drop(&mut self) {
        FAULTS
            .lock()
            .unwrap()
            .retain(|(p, s)| p != &self.0 || s != &self.1)
    }
}
fn read_state(root: &Path) -> State {
    let (_r, _l) = files::stopped_lock(root).unwrap();
    let (_t, c) = admission::inspect(root).unwrap();
    state(&c).unwrap()
}
fn phase(root: &Path) -> &'static str {
    match read_state(root) {
        State::Active => "active",
        State::Sealed(_) => "sealed",
        State::Ready(..) => "ready",
    }
}
fn tree(root: &Path) -> BTreeMap<String, (u32, String)> {
    fn add(base: &Path, at: &Path, out: &mut BTreeMap<String, (u32, String)>) {
        for e in fs::read_dir(at).unwrap() {
            let p = e.unwrap().path();
            let m = fs::symlink_metadata(&p).unwrap();
            let name = p.strip_prefix(base).unwrap().to_string_lossy().to_string();
            if m.is_dir() {
                out.insert(name, (m.permissions().mode(), "directory".into()));
                add(base, &p, out)
            } else {
                out.insert(name, (m.permissions().mode(), hash(&fs::read(p).unwrap())));
            }
        }
    }
    let mut result = BTreeMap::new();
    add(root, root, &mut result);
    result
}
fn unpack(f: &Fixture) -> BTreeMap<String, Vec<u8>> {
    let plain = age::decrypt(&f.identity, &fs::read(&f.output).unwrap()).unwrap();
    let mut result = BTreeMap::new();
    let mut order = vec![];
    for e in tar::Archive::new(plain.as_slice()).entries().unwrap() {
        let mut e = e.unwrap();
        assert!(e.header().entry_type().is_file());
        let name = e.path().unwrap().to_string_lossy().to_string();
        let mut b = vec![];
        e.read_to_end(&mut b).unwrap();
        order.push(name.clone());
        assert!(result.insert(name, b).is_none())
    }
    assert_eq!(
        order,
        vec![
            "manifest.json",
            "assistant.sqlite",
            JOURNALS[0],
            JOURNALS[1],
            JOURNALS[2],
            MARKER
        ]
    );
    result
}
#[tokio::test]
async fn sealed_package_has_exact_snapshot_inventory_trusted_head_and_all_entrypoint_fences() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    // Capture never requires the server provider key, and does not recreate it.
    fs::remove_file(f.data.join("provider-secrets.key")).unwrap();
    let summary = checkpoint(&f.data, &f.output, &f.head).unwrap();
    let before = tree(&f.data);
    let head: TrustedHead = serde_json::from_slice(&fs::read(&f.head).unwrap()).unwrap();
    assert_eq!(head.seal_id, summary.seal_id);
    assert_eq!(head.checkpoint_id, summary.checkpoint_id);
    assert_eq!(head.ciphertext_sha256, hash(&fs::read(&f.output).unwrap()));
    assert_eq!(head.ciphertext_size, fs::metadata(&f.output).unwrap().len());
    let contents = unpack(&f);
    let manifest: Manifest = serde_json::from_slice(&contents["manifest.json"]).unwrap();
    assert_eq!(manifest.purpose, "assistant-sealed-cutover");
    assert_eq!(head.manifest_sha256, hash(&contents["manifest.json"]));
    assert_eq!(head.seal_sha256, hash(&contents[MARKER]));
    assert!(manifest.authoritative_files.is_empty());
    for e in &manifest.entries {
        assert_eq!(e.size, contents[&e.name].len() as u64);
        assert_eq!(e.sha256, hash(&contents[&e.name]));
    }
    let saved = f.kit.join("inspected.sqlite");
    fs::write(&saved, &contents["assistant.sqlite"]).unwrap();
    let conn = Connection::open(saved).unwrap();
    let State::Sealed(seal) = state(&conn).unwrap() else {
        panic!("archive DB must precede ready receipt")
    };
    assert_eq!(seal.seal_id, summary.seal_id);
    assert!(matches!(Store::open(&f.data), Err(StoreError::Unavailable)));
    assert!(matches!(
        Store::open_with_web(&f.data, WebToolProfile::default()),
        Err(StoreError::Unavailable)
    ));
    assert_eq!(
        backup::setup(
            &f.data,
            &f.identity.to_public().to_string(),
            &f.identity_file
        )
        .err(),
        Some(Error::Sealed)
    );
    assert_eq!(
        backup::create(&f.data, &f.output.with_file_name("ordinary.age")).err(),
        Some(Error::Sealed)
    );
    assert!(
        crate::recover_account(
            &f.data,
            f.user.account().id.clone(),
            Role::Admin,
            "New-synthetic-password-2026".into()
        )
        .await
        .is_err()
    );
    assert_eq!(tree(&f.data), before);
    assert!(!f.data.join("provider-secrets.key").exists());
    let ciphertext = fs::read(&f.output).unwrap();
    let head_bytes = fs::read(&f.head).unwrap();
    let again = checkpoint(&f.data, &f.output, &f.head).unwrap();
    assert_eq!(again.checkpoint_id, summary.checkpoint_id);
    assert_eq!(fs::read(&f.output).unwrap(), ciphertext);
    assert_eq!(fs::read(&f.head).unwrap(), head_bytes);
    let other = f.output.with_file_name("second-copy.age");
    let other_head = f.head.with_file_name("second-head.json");
    checkpoint(&f.data, &other, &other_head).unwrap();
    assert_eq!(fs::read(other).unwrap(), ciphertext);
    assert_eq!(fs::read(other_head).unwrap(), head_bytes);
    for p in [&f.output, &f.head] {
        assert_eq!(fs::metadata(p).unwrap().permissions().mode() & 0o777, 0o600)
    }
    let wrong = age::x25519::Identity::generate();
    assert!(age::decrypt(&wrong, &ciphertext).is_err());
    assert!(backup::decode_manifest(&contents["manifest.json"]).is_err());
}
#[tokio::test]
async fn rejects_before_adoption_for_live_writer_unconfigured_invalid_paths_and_pending_intents() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    let store = Store::open(&f.data).unwrap();
    let clone = store.clone();
    drop(store);
    assert!(matches!(
        checkpoint(&f.data, &f.output, &f.head),
        Err(Error::Busy)
    ));
    drop(clone);
    assert_eq!(phase(&f.data), "active");
    assert!(checkpoint(&f.data, &f.output, &f.output.with_file_name("head.json")).is_err());
    assert_eq!(phase(&f.data), "active");
    for invalid in ["x".repeat(300), "bad\0.age".into()] {
        assert!(matches!(
            checkpoint(&f.data, &f.output.with_file_name(invalid), &f.head),
            Err(Error::InvalidInput)
        ));
        assert_eq!(phase(&f.data), "active");
    }
    fs::write(&f.output, b"retained output").unwrap();
    assert!(matches!(
        checkpoint(&f.data, &f.output, &f.head),
        Err(Error::OutputExists)
    ));
    assert_eq!(phase(&f.data), "active");
    assert_eq!(fs::read(&f.output).unwrap(), b"retained output");
    fs::remove_file(&f.output).unwrap();
    let linked = f.output.with_file_name("linked");
    symlink(f.output.parent().unwrap(), &linked).unwrap();
    assert!(checkpoint(&f.data, &linked.join("bad.age"), &f.head).is_err());
    assert_eq!(phase(&f.data), "active");
    let store = Store::open(&f.data).unwrap();
    let account = f.user.account().id.clone();
    store
        .call(move |db| {
            db.journal.append(&account, 1)?;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    assert!(matches!(
        checkpoint(&f.data, &f.output, &f.head),
        Err(Error::JournalPending)
    ));
    assert_eq!(phase(&f.data), "active");
    assert!(!f.data.join(MARKER).exists());
    assert!(!f.output.exists());
    assert!(!f.head.exists());
    let fresh = directory();
    let data = fresh.path().join("unconfigured");
    let store = Store::open(&data).unwrap();
    admin(&store).await;
    drop(store);
    assert!(matches!(
        checkpoint(&data, &f.output, &f.head),
        Err(Error::Unconfigured)
    ));
    assert_eq!(phase(&data), "active");
    let absent = fresh.path().join("absent");
    assert!(checkpoint(&absent, &f.output, &f.head).is_err());
    assert!(!absent.exists());
}
#[tokio::test]
async fn failures_keep_monotonic_seal_ids_and_resume_only_the_same_ready_bytes() {
    let _g = TEST_LOCK.lock().await;
    for point in [
        "journal_sync",
        "journal_dir_sync",
        "before_seal_commit",
        "after_seal_commit",
        "marker_sync",
        "marker_dir_sync",
        "snapshot",
        "age_finish",
        "stage_sync",
        "stage_dir_sync",
        "before_ready_commit",
        "after_ready_commit",
        "ready_sync",
        "cipher_sync",
        "cipher_after_rename",
        "cipher_dir_sync",
        "head_sync",
        "head_after_rename",
        "head_dir_sync",
    ] {
        let f = fixture().await;
        let fault = Fault::at(&f.data, point);
        assert!(checkpoint(&f.data, &f.output, &f.head).is_err(), "{point}");
        let adopted = !matches!(
            point,
            "journal_sync" | "journal_dir_sync" | "before_seal_commit"
        );
        let (ids, ready) = match read_state(&f.data) {
            State::Active => {
                assert!(!adopted);
                (None, None)
            }
            State::Sealed(s) => {
                assert!(adopted);
                (Some((s.seal_id, s.checkpoint_id)), None)
            }
            State::Ready(s, r) => {
                assert!(adopted);
                (
                    Some((s.seal_id, s.checkpoint_id)),
                    Some(r.head.ciphertext_sha256),
                )
            }
        };
        if adopted {
            assert!(Store::open(&f.data).is_err());
        } else {
            assert!(!f.output.exists());
            assert!(!f.head.exists());
        }
        // A second attempt while the same sync/phase fault remains cannot claim success.
        if !matches!(
            point,
            "after_seal_commit"
                | "before_seal_commit"
                | "before_ready_commit"
                | "after_ready_commit"
                | "cipher_after_rename"
                | "head_after_rename"
        ) {
            assert!(checkpoint(&f.data, &f.output, &f.head).is_err(), "{point}")
        }
        drop(fault);
        let result = checkpoint(&f.data, &f.output, &f.head).unwrap();
        assert_eq!(phase(&f.data), "ready");
        if let Some((seal, id)) = ids {
            assert_eq!((result.seal_id, result.checkpoint_id), (seal, id));
        }
        if let Some(h) = ready {
            assert_eq!(hash(&fs::read(&f.output).unwrap()), h)
        }
        let _ = unpack(&f);
    }
}
#[tokio::test]
async fn marker_loss_partial_and_conflicts_and_ready_stage_corruption_never_unseal_or_reencrypt() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    let fail = Fault::at(&f.data, "after_seal_commit");
    assert!(checkpoint(&f.data, &f.output, &f.head).is_err());
    drop(fail);
    let State::Sealed(s) = read_state(&f.data) else {
        panic!()
    };
    let b = encode(&s, 16384).unwrap();
    let path = f.data.join(MARKER);
    let mut marker = files::create_file(&path).unwrap();
    marker.write_all(&b[..17]).unwrap();
    marker.sync_all().unwrap();
    drop(marker);
    checkpoint(&f.data, &f.output, &f.head).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b);
    fs::write(&path, b"conflicting seal").unwrap();
    assert!(matches!(
        checkpoint(&f.data, &f.output, &f.head),
        Err(Error::Integrity)
    ));
    assert_eq!(fs::read(&path).unwrap(), b"conflicting seal");
    assert!(Store::open(&f.data).is_err());
    fs::write(&path, &b).unwrap();
    let stage = f
        .data
        .join(STAGING)
        .join(format!("{}.age", s.checkpoint_id));
    let original = fs::read(&stage).unwrap();
    fs::write(&stage, b"broken stage").unwrap();
    let output_before = fs::read(&f.output).unwrap();
    assert!(checkpoint(&f.data, &f.output, &f.head).is_err());
    assert_eq!(fs::read(&stage).unwrap(), b"broken stage");
    assert_eq!(fs::read(&f.output).unwrap(), output_before);
    fs::write(&stage, &original[..original.len() / 2]).unwrap();
    // A truncated exact prefix can be completed from the immutable output;
    // conflicting bytes above remain rejected and untouched.
    checkpoint(&f.data, &f.output, &f.head).unwrap();
    assert_eq!(fs::read(&stage).unwrap(), original);
    let unknown = f.data.join(STAGING).join("unknown");
    fs::write(&unknown, b"retained unknown bytes").unwrap();
    assert!(checkpoint(&f.data, &f.output, &f.head).is_err());
    assert_eq!(fs::read(&unknown).unwrap(), b"retained unknown bytes");
    fs::remove_file(&unknown).unwrap();
    // With every file fence removed, the DB ready row still rejects every opener.
    fs::remove_file(&path).unwrap();
    fs::remove_file(&stage).unwrap();
    fs::remove_dir(f.data.join(STAGING)).unwrap();
    let before = tree(&f.data);
    assert!(Store::open(&f.data).is_err());
    assert!(
        backup::setup(
            &f.data,
            &f.identity.to_public().to_string(),
            &f.identity_file
        )
        .is_err()
    );
    assert!(
        crate::recover_account(
            &f.data,
            f.user.account().id.clone(),
            Role::Admin,
            PASSWORD.into()
        )
        .await
        .is_err()
    );
    assert_eq!(tree(&f.data), before);
    // Exact already-published ciphertext can repair missing encrypted staging;
    // no new encryption or identity is adopted after ready.
    let repaired = checkpoint(&f.data, &f.output, &f.head).unwrap();
    assert_eq!(repaired.checkpoint_id, s.checkpoint_id);
    assert_eq!(fs::read(&stage).unwrap(), original);
}
#[tokio::test]
async fn rollback_marker_disagreement_and_missing_installed_singleton_fail_closed() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    let o = backup::Offline::for_cutover(&f.data).unwrap();
    let saved = f.kit.join("before.sqlite");
    let mut db = Connection::open(&saved).unwrap();
    rusqlite::backup::Backup::new(&o.conn, &mut db)
        .unwrap()
        .run_to_completion(128, Duration::from_millis(1), None)
        .unwrap();
    drop((db, o));
    checkpoint(&f.data, &f.output, &f.head).unwrap();
    fs::copy(&saved, f.data.join("assistant.sqlite")).unwrap();
    assert!(Store::open(&f.data).is_err());
    assert!(checkpoint(&f.data, &f.output, &f.head).is_err());
    let f = fixture().await;
    let conn = writable(&f.data).unwrap();
    conn.execute_batch(
        "DROP TRIGGER installation_cutover_no_delete;DELETE FROM installation_cutover;",
    )
    .unwrap();
    drop(conn);
    assert!(Store::open(&f.data).is_err());
    assert!(checkpoint(&f.data, &f.output, &f.head).is_err());
}
#[tokio::test]
async fn full_end_state_snapshot_preserves_source_update_withdrawal_even_without_new_journal_head()
{
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    let store = Store::open(&f.data).unwrap();
    let other = member(&store, &f.user, "retained@example.test").await.user;
    let group = store
        .create_collection(
            &f.user,
            "Synthetic source".into(),
            Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    let source_text = "Evidence canary from first source";
    let input = |text: &str, version: &str, revision: Option<i64>| SourceInput {
        external_id: "synthetic".into(),
        version: version.into(),
        expected_revision: revision,
        title: "Source".into(),
        text: text.into(),
        speaker: None,
        observed_at: 100,
        valid_from: None,
        valid_until: None,
    };
    let batch = |cursor, entry| IngestBatch {
        request_id: Uuid::new_v4().to_string(),
        expected_cursor: cursor,
        next_cursor: cursor + 1,
        complete_snapshot: false,
        entries: vec![entry],
    };
    let source = store
        .ingest_text(
            &f.user,
            group.id.clone(),
            batch(0, input(source_text, "v1", None)),
        )
        .await
        .unwrap()
        .source_ids[0]
        .clone();
    let draft = |text: &str, evidence| MemoryDraft {
        text: text.into(),
        subject: "Synthetic".into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence,
    };
    let memory = store
        .create_memory(
            &f.user,
            Uuid::new_v4().to_string(),
            draft(
                "Derived evidence canary",
                vec![EvidenceRef {
                    source_id: source,
                    source_revision: 1,
                    start_byte: 0,
                    end_byte: source_text.len() as u32,
                    quote: source_text.into(),
                }],
            ),
        )
        .await
        .unwrap();
    let retained = store
        .create_memory(
            &other,
            Uuid::new_v4().to_string(),
            draft("Other tenant retained fact", vec![]),
        )
        .await
        .unwrap();
    let old = f.kit.join("older.sqlite");
    let save = old.clone();
    store
        .call(move |db| {
            let mut out = Connection::open(save)?;
            rusqlite::backup::Backup::new(&db.conn, &mut out)?.run_to_completion(
                128,
                Duration::from_millis(1),
                None,
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let before = store
        .call(|db| Ok(db.destructive_journal.records.len()))
        .await
        .unwrap();
    store
        .ingest_text(
            &f.user,
            group.id,
            batch(1, input("Replacement source", "v2", Some(1))),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .memory(&f.user, memory.id.clone(), None)
            .await
            .unwrap()
            .status,
        MemoryStatus::Invalidated
    );
    assert_eq!(
        store
            .call(|db| Ok(db.destructive_journal.records.len()))
            .await
            .unwrap(),
        before
    );
    drop(store);
    checkpoint(&f.data, &f.output, &f.head).unwrap();
    let bytes = unpack(&f);
    let p = f.kit.join("sealed.sqlite");
    fs::write(&p, &bytes["assistant.sqlite"]).unwrap();
    let sealed = Connection::open(p).unwrap();
    let old = Connection::open(old).unwrap();
    assert_eq!(
        old.query_row(
            "SELECT status FROM knowledge_memories WHERE id=?1",
            [&memory.id],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "active"
    );
    assert_eq!(
        sealed
            .query_row(
                "SELECT status FROM knowledge_memories WHERE id=?1",
                [&memory.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "invalidated"
    );
    assert_eq!(
        sealed
            .query_row(
                "SELECT count(*) FROM knowledge_fts WHERE kind='memory' AND object_id=?1",
                [&memory.id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert_eq!(
        sealed
            .query_row(
                "SELECT status FROM knowledge_memories WHERE id=?1",
                [retained.id],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "active"
    );
}

#[tokio::test]
async fn bounded_codecs_reject_wrong_purpose_schema_coverage_inventory_and_false_heads() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture().await;
    checkpoint(&f.data, &f.output, &f.head).unwrap();
    let contents = unpack(&f);
    let encoded = &contents["manifest.json"];
    decode_manifest(encoded).unwrap();
    let head = fs::read(&f.head).unwrap();
    decode_head(&head).unwrap();
    for problem in 0..8 {
        let mut v: serde_json::Value = serde_json::from_slice(encoded).unwrap();
        match problem {
            0 => v["purpose"] = serde_json::json!("assistant-export"),
            1 => v["format"] = serde_json::json!(2),
            2 => v["seal"]["schema"] = serde_json::json!(crate::CURRENT_SCHEMA_VERSION + 1),
            3 => v["seal"]["coverage"]["schema_baseline"] = serde_json::json!(14),
            4 => {
                v["entries"].as_array_mut().unwrap().pop();
            }
            5 => v["entries"][4]["sha256"] = serde_json::json!("f".repeat(64)),
            6 => v["seal"]["auth_journal"]["snapshot_sequence"] = serde_json::json!(999),
            _ => v["authoritative_files"] = serde_json::json!(["unimplemented-media"]),
        }
        assert!(
            decode_manifest(&serde_json::to_vec(&v).unwrap()).is_err(),
            "{problem}"
        );
    }
    for problem in 0..6 {
        let mut v: serde_json::Value = serde_json::from_slice(&head).unwrap();
        match problem {
            0 => v["purpose"] = serde_json::json!("other"),
            1 => v["latest"] = serde_json::json!(true),
            2 => v["installation_id"] = serde_json::json!("wrong"),
            3 => v["ciphertext_size"] = serde_json::json!(CIPHER_LIMIT + 1),
            4 => v["coverage"]["successor_journal"]["head"] = serde_json::json!("bad"),
            _ => v["coverage"]["successor_journal"]["snapshot_sequence"] = serde_json::json!(999),
        }
        assert!(decode_head(&serde_json::to_vec(&v).unwrap()).is_err());
    }
    assert!(matches!(
        decode_manifest(&vec![b' '; 32769]),
        Err(Error::LimitExceeded)
    ));
    assert!(matches!(
        decode_head(&vec![b' '; 8193]),
        Err(Error::LimitExceeded)
    ));
    let mut w = publication::BoundedWriter::new(vec![], 3);
    assert_eq!(w.write(b"abc").unwrap(), 3);
    assert!(w.write(b"d").is_err());
    assert_eq!(w.into_inner(), b"abc");
    let conn = writable(&f.data).unwrap();
    assert!(
        conn.execute(
            "UPDATE installation_cutover SET phase='active',seal=NULL,checkpoint=NULL",
            []
        )
        .is_err()
    );
    assert!(
        conn.execute("DELETE FROM installation_cutover", [])
            .is_err()
    );
}
#[tokio::test]
async fn abrupt_process_exit_after_seal_and_ready_adoption_resumes_without_activation() {
    if let Some(root) = std::env::var_os("MOOSURI_CUTOVER_EXIT_ROOT") {
        let output = std::env::var_os("MOOSURI_CUTOVER_EXIT_OUTPUT").unwrap();
        let head = std::env::var_os("MOOSURI_CUTOVER_EXIT_HEAD").unwrap();
        checkpoint(Path::new(&root), Path::new(&output), Path::new(&head)).unwrap();
        panic!("child must exit at its selected commit boundary");
    }
    let _g = TEST_LOCK.lock().await;
    for point in ["after_seal_commit", "after_ready_commit"] {
        let f = fixture().await;
        let child=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","cutover::tests::abrupt_process_exit_after_seal_and_ready_adoption_resumes_without_activation","--nocapture"]).env("MOOSURI_CUTOVER_EXIT_ROOT",&f.data).env("MOOSURI_CUTOVER_EXIT_OUTPUT",&f.output).env("MOOSURI_CUTOVER_EXIT_HEAD",&f.head).env("MOOSURI_CUTOVER_EXIT_POINT",point).output().unwrap();
        assert_eq!(child.status.code(), Some(73));
        assert!(Store::open(&f.data).is_err());
        assert!(!f.output.exists());
        assert!(!f.head.exists());
        let (id, expected) = match read_state(&f.data) {
            State::Sealed(s) => (s.checkpoint_id, None),
            State::Ready(s, r) => (s.checkpoint_id, Some(r.head.ciphertext_sha256)),
            State::Active => panic!("seal commit must survive abrupt process exit"),
        };
        let resumed = checkpoint(&f.data, &f.output, &f.head).unwrap();
        assert_eq!(resumed.checkpoint_id, id);
        if let Some(h) = expected {
            assert_eq!(resumed.ciphertext_sha256, h)
        }
        assert_eq!(phase(&f.data), "ready");
        let _ = unpack(&f);
    }
}

#[tokio::test]
#[ignore = "requires an explicitly selected reference Go age binary; synthetic checkpoint only"]
async fn reference_age_decrypts_standard_six_entry_checkpoint() {
    let _g = TEST_LOCK.lock().await;
    let binary = std::env::var_os("MOOSURI_REFERENCE_AGE")
        .expect("explicit reference age binary path required");
    let f = fixture().await;
    checkpoint(&f.data, &f.output, &f.head).unwrap();
    let decoded = std::process::Command::new(binary)
        .args(["--decrypt", "--identity"])
        .arg(&f.identity_file)
        .arg(&f.output)
        .output()
        .unwrap();
    assert!(decoded.status.success());
    let names = tar::Archive::new(decoded.stdout.as_slice())
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "manifest.json",
            "assistant.sqlite",
            JOURNALS[0],
            JOURNALS[1],
            JOURNALS[2],
            MARKER
        ]
    );
}

#[tokio::test]
async fn stale_new_root_admission_rechecks_seal_before_any_source_mutation() {
    let _g = TEST_LOCK.lock().await;
    // Pause after the same source root was genuinely absent, then interleave
    // its initialize/setup/seal before the delayed Store opener resumes.
    let (f, admission) = review_fixture().await;
    let fault = Fault::at(&f.data, "after_seal_commit");
    assert!(checkpoint(&f.data, &f.output, &f.head).is_err());
    drop(fault);
    assert!(!f.data.join(MARKER).exists());
    assert!(!f.data.join(STAGING).exists());
    // Owner-private root with restricted writes remains readable for sealed
    // inspection; reject before restoring the runtime directory mode.
    fs::set_permissions(&f.data, fs::Permissions::from_mode(0o500)).unwrap();
    let before = tree(&f.data);
    assert_eq!(
        fs::metadata(&f.data).unwrap().permissions().mode() & 0o777,
        0o500
    );
    assert!(Store::open_admitted(&f.data, WebToolProfile::default(), admission).is_err());
    let after = tree(&f.data);
    assert_eq!(
        fs::metadata(&f.data).unwrap().permissions().mode() & 0o777,
        0o500,
        "sealed source must stay untouched before rejection"
    );
    assert_eq!(before, after);
}

async fn review_fixture() -> (Fixture, Option<RuntimeAdmission>) {
    let root = directory();
    let data = root.path().join("data");
    let kit = root.path().join("kit");
    let out = root.path().join("cipher");
    let heads = root.path().join("heads");
    for p in [&kit, &out, &heads] {
        fs::create_dir(p).unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap()
    }
    let delayed_admission = None; // stale initially absent decision from a delayed opener
    assert!(!data.exists());
    assert!(delayed_admission.is_none());
    let store = Store::open(&data).unwrap();
    let user = admin(&store).await.user;
    drop(store);
    let identity = age::x25519::Identity::generate();
    let identity_file = kit.join("identity");
    let mut f = files::create_file(&identity_file).unwrap();
    f.write_all(identity.to_string().expose_secret().as_bytes())
        .unwrap();
    f.sync_all().unwrap();
    backup::setup(&data, &identity.to_public().to_string(), &identity_file).unwrap();
    (
        Fixture {
            _root: root,
            data,
            kit,
            output: out.join("checkpoint.age"),
            head: heads.join("head.json"),
            identity,
            identity_file,
            user,
        },
        delayed_admission,
    )
}
