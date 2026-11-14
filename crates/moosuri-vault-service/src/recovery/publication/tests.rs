use super::*;
use age::secrecy::ExposeSecret;
fn private(parent: &Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    p
}
pub(super) struct Fixture {
    pub temp: tempfile::TempDir,
    pub source: PathBuf,
    pub scratch: PathBuf,
    pub target: PathBuf,
    pub artifacts: PathBuf,
    pub ledger: PathBuf,
    pub snapshot: PathBuf,
    pub snapshot_head: PathBuf,
    pub identity: PathBuf,
    pub recipient: String,
    pub db: Database,
}
impl Fixture {
    pub fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let source = temp.path().join("source");
        let scratch = private(temp.path(), "scratch");
        let target = private(temp.path(), "target");
        let artifacts = private(temp.path(), "artifacts");
        let ledger = private(temp.path(), "ledger");
        let snapshot = scratch.join("snapshot.age");
        let snapshot_head = scratch.join("snapshot.json");
        let key = age::x25519::Identity::generate();
        let recipient = key.to_public().to_string();
        let identity = scratch.join("offline-test-identity");
        write_new(&identity, key.to_string().expose_secret().as_bytes()).unwrap();
        let mut db = Database::open(&source).unwrap();
        db.close_session("baseline").unwrap();
        db.capture_snapshot(&scratch)
            .unwrap()
            .encrypt(&recipient, &snapshot, &snapshot_head)
            .unwrap();
        init_target(
            &target,
            &artifacts,
            &ledger,
            &snapshot,
            &snapshot_head,
            &recipient,
        )
        .unwrap();
        Self {
            temp,
            source,
            scratch,
            target,
            artifacts,
            ledger,
            snapshot,
            snapshot_head,
            identity,
            recipient,
            db,
        }
    }
    pub fn prepared(&self) -> Prepared {
        self.db
            .capture_snapshot(&self.scratch)
            .unwrap()
            .prepare_publication(&self.recipient, &self.snapshot_head)
            .unwrap()
    }
    fn frame(&self) -> Vec<u8> {
        let mut b = vec![];
        self.prepared().write_to(&mut b).unwrap();
        b
    }
    fn restore(&self, destination: &Path) -> Result<()> {
        let status = target_status(&self.target)?;
        let current = status.current.ok_or(Error::Unavailable)?;
        historical::restore_historical(
            destination,
            &status.snapshot,
            &status.snapshot_head,
            &self.identity,
            &historical::JournalInputs {
                checkpoint: &status.journal.ok_or(Error::Unavailable)?,
                trusted_head: &status.journal_head.ok_or(Error::Unavailable)?,
                trusted_head_sha256: &current.trusted_head_sha256,
                identity_file: &self.identity,
            },
        )
    }
}
fn encode_frame(m: &Request, journal: &[u8]) -> Vec<u8> {
    let meta = encode(m).unwrap();
    let mut out = WIRE.to_vec();
    out.extend((meta.len() as u32).to_be_bytes());
    out.extend(meta);
    out.extend(journal);
    out
}
#[test]
fn publisher_streams_encrypted_only_monotonic_receipts_ack_loss_and_historical_restore() {
    let mut f = Fixture::new();
    assert!(target_status(&f.target).unwrap().current.is_none());
    f.db.close_session("later").unwrap();
    let frame = f.frame();
    FAILURE.with(|v| v.set(Some("after_receipt")));
    assert!(receive(&f.target, frame.as_slice()).is_err());
    FAILURE.with(|v| v.set(None));
    let persisted = target_status(&f.target).unwrap().current.unwrap();
    let retry = receive(&f.target, f.frame().as_slice()).unwrap();
    assert_eq!(retry, persisted);
    assert_eq!(retry.generation, 1);
    assert_eq!(retry.trusted_head_sha256, retry.journal.pin().unwrap());
    assert_eq!(fs::read_dir(&f.ledger).unwrap().count(), 1);
    let generation_dir = f.artifacts.join(generation(1));
    let mut names = fs::read_dir(&generation_dir)
        .unwrap()
        .map(|v| v.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["head.json", "journal.age"]);
    let journal_bytes = fs::read(f.source.join("vault.journal")).unwrap();
    let cipher = fs::read(generation_dir.join("journal.age")).unwrap();
    assert!(
        !cipher
            .windows(journal_bytes.len())
            .any(|w| w == journal_bytes)
    );
    assert!(!f.target.join("identity").exists());
    f.db.close_session("third").unwrap();
    let second = receive(&f.target, f.frame().as_slice()).unwrap();
    assert_eq!(second.generation, 2);
    assert_eq!(
        second.previous_receipt_sha256,
        hash(
            &bytes(
                &f.ledger.join(format!("{}.json", generation(1))),
                JSON_LIMIT
            )
            .unwrap()
        )
    );
    let destination = f.temp.path().join("restored");
    f.restore(&destination).unwrap();
    let mut restored = Database::open(&destination).unwrap();
    let owner = protocol::Owner {
        tenant: uuid::Uuid::new_v4(),
        account: uuid::Uuid::new_v4(),
        installation: uuid::Uuid::new_v4(),
        epoch: 1,
    };
    restored.create_session("fresh", &owner).unwrap();
    drop(restored);
    f.restore(&destination).unwrap();
    assert!(
        Database::open(&destination)
            .unwrap()
            .session("fresh")
            .unwrap()
            == owner
    );
    assert!(!f.source.join("vault.sealed").exists());
}
#[test]
fn publisher_rejects_old_fork_foreign_malformed_and_target_tampering() {
    let mut f = Fixture::new();
    let old = f.frame();
    f.db.close_session("next").unwrap();
    receive(&f.target, f.frame().as_slice()).unwrap();
    assert!(receive(&f.target, old.as_slice()).is_err());
    let p = f.prepared();
    let mut m = p.request.clone();
    let original = fs::read(p.capture.stage.0.join("vault.journal")).unwrap();
    let mut fork = original[..24].to_vec();
    let mut chain = [0; 32];
    for session in ["different", "next"] {
        let body = encode(&Intent::Close {
            session: hash(session.as_bytes()),
        })
        .unwrap();
        let mut h = Sha256::new();
        h.update(chain);
        h.update(&body);
        chain = h.finalize().into();
        fork.extend((body.len() as u32).to_be_bytes());
        fork.extend(body);
        fork.extend(chain);
    }
    m.journal_size = fork.len() as u64;
    m.journal_sha256 = hash(&fork);
    m.binding.head = hash_hex(chain);
    assert!(receive(&f.target, encode_frame(&m, &fork).as_slice()).is_err());
    for mode in [
        "foreign",
        "recipient",
        "baseline",
        "size",
        "truncated",
        "tail",
        "head",
    ] {
        let mut m = p.request.clone();
        let mut body = original.clone();
        match mode {
            "foreign" => m.binding.source = uuid::Uuid::new_v4(),
            "recipient" => m.recipient = age::x25519::Identity::generate().to_public().to_string(),
            "baseline" => m.baseline_sha256 = "00".repeat(32),
            "size" => m.journal_size = JOURNAL_LIMIT + 1,
            "truncated" => {
                body.pop();
            }
            "tail" => body.push(0),
            "head" => m.binding.head = "00".repeat(32),
            _ => unreachable!(),
        }
        assert!(
            receive(&f.target, encode_frame(&m, &body).as_slice()).is_err(),
            "{mode}"
        );
    }
    assert_eq!(
        target_status(&f.target)
            .unwrap()
            .current
            .unwrap()
            .generation,
        1
    );
    let archive = f.artifacts.join(generation(1)).join("journal.age");
    let cipher = fs::read(&archive).unwrap();
    fs::write(&archive, &cipher[..cipher.len() - 1]).unwrap();
    assert!(target_status(&f.target).is_err());
    assert!(receive(&f.target, f.frame().as_slice()).is_err());
    fs::write(&archive, cipher).unwrap();
    let receipt = f.ledger.join(format!("{}.json", generation(1)));
    let raw = fs::read(&receipt).unwrap();
    fs::write(&receipt, b"{}").unwrap();
    assert!(target_status(&f.target).is_err());
    fs::write(&receipt, raw).unwrap();
    assert!(target_status(&f.target).is_ok());
}
fn hash_hex(v: [u8; 32]) -> String {
    v.iter().map(|b| format!("{b:02x}")).collect()
}
#[test]
fn publisher_crash_before_receipt_never_accepts_or_overwrites_orphan_generation() {
    for point in ["before_generation", "after_generation", "before_receipt"] {
        let mut f = Fixture::new();
        f.db.close_session("next").unwrap();
        FAILURE.with(|v| v.set(Some(point)));
        assert!(receive(&f.target, f.frame().as_slice()).is_err());
        FAILURE.with(|v| v.set(None));
        assert!(target_status(&f.target).unwrap().current.is_none());
        let a = receive(&f.target, f.frame().as_slice()).unwrap();
        assert_eq!(a.generation, 1);
        assert_eq!(target_status(&f.target).unwrap().current.unwrap(), a);
        assert!(
            init_target(
                &f.target,
                &f.artifacts,
                &f.ledger,
                &f.snapshot,
                &f.snapshot_head,
                &f.recipient
            )
            .is_err()
        );
    }
}
#[test]
fn publisher_requires_private_disjoint_explicit_receiver_and_healthy_source() {
    let f = Fixture::new();
    assert!(receive(&f.temp.path().join("absent"), f.frame().as_slice()).is_err());
    assert!(!f.temp.path().join("absent").exists());
    let _lock = lock(&f.target).unwrap();
    assert!(target_status(&f.target).is_err());
    let new = f.db.capture_snapshot(&f.scratch).unwrap();
    fs::remove_file(new.stage.0.join("vault.journal")).unwrap();
    assert!(
        new.prepare_publication(&f.recipient, &f.snapshot_head)
            .is_err()
    );
    let absent = f.temp.path().join("never-created");
    assert!(target_status(&absent).is_err());
    assert!(!absent.exists());
}
