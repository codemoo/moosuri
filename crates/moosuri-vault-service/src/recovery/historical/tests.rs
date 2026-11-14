use super::*;
use age::secrecy::ExposeSecret;
fn write_fixture(
    directory: &Path,
    name: &str,
    bytes: &[u8],
    mut binding: CheckpointBinding,
    key: &age::x25519::Identity,
) -> (PathBuf, PathBuf, String) {
    binding.format = FORMAT.into();
    let manifest = encode(&JournalManifest {
        journal: binding.clone(),
        journal_size: bytes.len() as u64,
        journal_sha256: hash(bytes),
    })
    .unwrap();
    let archive = directory.join(format!("{name}.age"));
    let receipt = directory.join(format!("{name}.json"));
    let recipient = key.to_public();
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .unwrap();
    let mut writer = encryptor
        .wrap_output(new_private(&archive).unwrap())
        .unwrap();
    writer.write_all(WIRE).unwrap();
    writer
        .write_all(&(manifest.len() as u32).to_be_bytes())
        .unwrap();
    writer.write_all(&manifest).unwrap();
    writer
        .write_all(&(bytes.len() as u64).to_be_bytes())
        .unwrap();
    writer.write_all(bytes).unwrap();
    writer.finish().unwrap().sync_all().unwrap();
    let (ciphertext_size, ciphertext_sha256) = digest_file(&archive, LIMIT).unwrap();
    let head = JournalTrustedHead {
        journal: binding,
        ciphertext_size,
        ciphertext_sha256,
    };
    write_new(&receipt, &encode(&head).unwrap()).unwrap();
    let pin = head.pin().unwrap();
    (archive, receipt, pin)
}
fn append(bytes: &mut Vec<u8>, head: [u8; 32], intent: Intent) -> [u8; 32] {
    let body = encode(&intent).unwrap();
    let mut h = Sha256::new();
    h.update(head);
    h.update(&body);
    let next: [u8; 32] = h.finalize().into();
    bytes.extend((body.len() as u32).to_be_bytes());
    bytes.extend(body);
    bytes.extend(next);
    next
}
#[test]
fn historical_journal_rejects_unpinned_older_divergent_invalid_semantics_and_corrupt_ciphertext() {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let exports = temp.path().join("exports");
    fs::DirBuilder::new().mode(0o700).create(&exports).unwrap();
    let source = temp.path().join("source");
    let mut db = Database::open(&source).unwrap();
    db.close_session("first").unwrap();
    let snapshot = exports.join("snapshot.age");
    let snapshot_head = exports.join("snapshot.json");
    let key = age::x25519::Identity::generate();
    let identity = exports.join("identity");
    write_new(&identity, key.to_string().expose_secret().as_bytes()).unwrap();
    db.capture_snapshot(&exports)
        .unwrap()
        .encrypt(&key.to_public().to_string(), &snapshot, &snapshot_head)
        .unwrap();
    let prefix = fs::read(source.join("vault.journal")).unwrap();
    let old_head = db.head;
    let old_seq = db.sequence;
    let source_id = installation(&db).unwrap();
    db.close_session("second").unwrap();
    let journal = exports.join("journal.age");
    let journal_head = exports.join("journal.json");
    let head = db
        .capture_snapshot(&exports)
        .unwrap()
        .encrypt_journal(&key.to_public().to_string(), &journal, &journal_head)
        .unwrap();
    let pin = head.pin().unwrap();
    let destination = temp.path().join("destination");
    let attempt = |archive: &Path, receipt: &Path, pin: &str| {
        restore_historical(
            &destination,
            &snapshot,
            &snapshot_head,
            &identity,
            &JournalInputs {
                checkpoint: archive,
                trusted_head: receipt,
                trusted_head_sha256: pin,
                identity_file: &identity,
            },
        )
    };
    assert!(attempt(&journal, &journal_head, &"00".repeat(32)).is_err());
    assert!(!destination.exists());
    let cipher = fs::read(&journal).unwrap();
    fs::write(&journal, &cipher[..cipher.len() - 1]).unwrap();
    assert!(attempt(&journal, &journal_head, &pin).is_err());
    fs::write(&journal, &cipher).unwrap();
    let binding = |sequence, head| CheckpointBinding {
        format: FORMAT.into(),
        version: 1,
        checkpoint: uuid::Uuid::new_v4(),
        source: source_id,
        sequence,
        head,
    };
    let (a, h, p) = write_fixture(
        &exports,
        "older",
        &prefix[..24],
        binding(0, "00".repeat(32)),
        &key,
    );
    assert!(attempt(&a, &h, &p).is_err());
    let mut divergent = prefix[..24].to_vec();
    let first = append(
        &mut divergent,
        [0; 32],
        Intent::Close {
            session: hash(b"different"),
        },
    );
    let end = append(
        &mut divergent,
        first,
        Intent::Close {
            session: hash(b"second"),
        },
    );
    let (a, h, p) = write_fixture(
        &exports,
        "divergent",
        &divergent,
        binding(old_seq + 1, hash_hex(end)),
        &key,
    );
    assert!(attempt(&a, &h, &p).is_err());
    let mut invalid = prefix.clone();
    let end = append(
        &mut invalid,
        old_head,
        Intent::Adopt {
            owner: protocol::Owner {
                tenant: uuid::Uuid::new_v4(),
                account: uuid::Uuid::new_v4(),
                installation: uuid::Uuid::new_v4(),
                epoch: 1,
            },
            operation: uuid::Uuid::new_v4(),
            digest: "00".repeat(32),
        },
    );
    let (a, h, p) = write_fixture(
        &exports,
        "invalid-cas",
        &invalid,
        binding(old_seq + 1, hash_hex(end)),
        &key,
    );
    assert!(attempt(&a, &h, &p).is_err());
    let mut truncated = prefix.clone();
    truncated.extend([0, 0, 0, 30]);
    let (a, h, p) = write_fixture(
        &exports,
        "truncated",
        &truncated,
        binding(old_seq + 1, "00".repeat(32)),
        &key,
    );
    assert!(attempt(&a, &h, &p).is_err());
    let mut foreign = head.journal.clone();
    foreign.source = uuid::Uuid::new_v4();
    let actual = fs::read(source.join("vault.journal")).unwrap();
    let (a, h, p) = write_fixture(&exports, "foreign", &actual, foreign, &key);
    assert!(attempt(&a, &h, &p).is_err());
    assert!(!destination.exists());
    assert!(!source.join("vault.sealed").exists());
    attempt(&journal, &journal_head, &pin).unwrap();
}
fn hash_hex(bytes: [u8; 32]) -> String {
    bytes.iter().map(|v| format!("{v:02x}")).collect()
}
