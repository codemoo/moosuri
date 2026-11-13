//! Exact sealed-checkpoint restore. This module never admits Store on a sealed root.
use super::*;
use fs2::FileExt;
use rusqlite::{OptionalExtension, params};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use zeroize::Zeroizing;

const STAGE_PREFIX: &str = ".moosuri-restore-stage-";
const PLAIN_LIMIT: u64 = backup::DB_LIMIT + 3 * backup::JOURNAL_LIMIT + 128 * 1024;
const RESTORE_SCHEMA: i64 = 20;
#[derive(Clone, Serialize)]
pub struct EnrollmentAccount {
    pub account_id: String,
    pub role: String,
    pub status: String,
}
#[derive(Clone, Serialize)]
pub struct RestoreSummary {
    pub installation_id: String,
    pub checkpoint_id: String,
    pub accounts: Vec<EnrollmentAccount>,
    pub already_published: bool,
}
pub(crate) fn reject_stage(path: &Path) -> Result<()> {
    if path
        .components()
        .any(|c| c.as_os_str().to_string_lossy().starts_with(STAGE_PREFIX))
    {
        Err(Error::UnsupportedInstallation)
    } else {
        Ok(())
    }
}
fn table(conn: &Connection, name: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
        [name],
        |r| r.get(0),
    )?)
}
/// The original journal remains byte-identical across restores. Only an immutable,
/// verified restore event may bind that lineage to a distinct installation UUID.
pub(crate) fn journal_lineage_matches(
    conn: &Connection,
    installed: &str,
    lineage: &str,
) -> crate::Result<bool> {
    if !table(conn, "installation_restores").map_err(|_| crate::StoreError::Corrupt)? {
        return Ok(false);
    }
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM installation_restores WHERE new_installation_id=?1 AND journal_installation_id=?2)", params![installed,lineage], |r|r.get(0))?)
}
fn read_small(path: &Path, cap: u64) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    files::read_file(path, cap)?
        .take(cap + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Unavailable)?;
    if bytes.len() as u64 > cap {
        return Err(Error::LimitExceeded);
    }
    Ok(bytes)
}
fn identity(path: &Path) -> Result<age::x25519::Identity> {
    let bytes = Zeroizing::new(read_small(path, 4096)?);
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::KeyMissing)?;
    let lines = text
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .collect::<Vec<_>>();
    if lines.len() != 1 {
        return Err(Error::KeyMissing);
    }
    lines[0].parse().map_err(|_| Error::KeyMissing)
}
fn destination(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(Error::InvalidInput);
    }
    reject_stage(path)?;
    let name = path.file_name().ok_or(Error::InvalidInput)?;
    let parent = files::directory(path.parent().ok_or(Error::InvalidInput)?)?;
    for p in parent.ancestors() {
        if [
            "Cargo.toml",
            ".git",
            "package.json",
            "assistant.sqlite",
            "writer.lock",
            MARKER,
            STAGING,
        ]
        .iter()
        .any(|n| fs::symlink_metadata(p.join(n)).is_ok())
        {
            return Err(Error::InvalidInput);
        }
    }
    Ok(parent.join(name))
}
struct Stage {
    path: PathBuf,
    identity: (u64, u64),
    published: bool,
}
impl Stage {
    fn new(parent: &Path) -> Result<Self> {
        let path = parent.join(format!("{STAGE_PREFIX}{}", Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| Error::Unavailable)?;
        let m = fs::symlink_metadata(&path).map_err(|_| Error::Unavailable)?;
        files::sync_dir(parent)?;
        Ok(Self {
            path,
            identity: (m.dev(), m.ino()),
            published: false,
        })
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if !self.published
            && fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.is_dir() && (m.dev(), m.ino()) == self.identity)
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
fn copy_cipher(input: &Path, output: &Path, h: &TrustedHead, start: Instant) -> Result<()> {
    let mut source = files::read_file(input, CIPHER_LIMIT)?;
    if source.metadata().map_err(|_| Error::Unavailable)?.len() != h.ciphertext_size {
        return Err(Error::Integrity);
    }
    let mut out = files::create_file(output)?;
    let mut sha = Sha256::new();
    let mut n = 0u64;
    let mut buf = [0; 65536];
    loop {
        deadline(start)?;
        let got = source.read(&mut buf).map_err(|_| Error::Unavailable)?;
        if got == 0 {
            break;
        }
        n += got as u64;
        if n > h.ciphertext_size {
            return Err(Error::Integrity);
        }
        sha.update(&buf[..got]);
        out.write_all(&buf[..got]).map_err(|_| Error::Unavailable)?;
    }
    if n != h.ciphertext_size || backup::hex(&sha.finalize()) != h.ciphertext_sha256 {
        return Err(Error::Integrity);
    }
    out.sync_all().map_err(|_| Error::Unavailable)
}
fn decrypt(
    cipher: &Path,
    plain: &Path,
    identity: &age::x25519::Identity,
    start: Instant,
) -> Result<()> {
    let input = files::read_file(cipher, CIPHER_LIMIT)?;
    let decryptor = age::Decryptor::new(input).map_err(|_| Error::DecryptionFailed)?;
    let mut input = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .map_err(|_| Error::DecryptionFailed)?;
    let mut out = files::create_file(plain)?;
    let mut count = 0u64;
    let mut buf = [0; 65536];
    // Read until the authenticated age EOF before parsing or opening the DB.
    loop {
        deadline(start)?;
        let n = input.read(&mut buf).map_err(|_| Error::DecryptionFailed)?;
        if n == 0 {
            break;
        }
        count = count.checked_add(n as u64).ok_or(Error::LimitExceeded)?;
        if count > PLAIN_LIMIT {
            return Err(Error::LimitExceeded);
        }
        out.write_all(&buf[..n]).map_err(|_| Error::Unavailable)?;
    }
    out.sync_all().map_err(|_| Error::Unavailable)
}
fn canonical_header(name: &str, size: u64) -> Result<[u8; 512]> {
    let mut h = tar::Header::new_ustar();
    h.set_path(name).map_err(|_| Error::Integrity)?;
    h.set_size(size);
    h.set_mode(0o600);
    h.set_uid(0);
    h.set_gid(0);
    h.set_mtime(0);
    h.set_entry_type(tar::EntryType::Regular);
    h.set_cksum();
    Ok(*h.as_bytes())
}
fn header(input: &mut File, name: &str, cap: u64) -> Result<u64> {
    let mut b = [0; 512];
    input.read_exact(&mut b).map_err(|_| Error::Integrity)?;
    let size = tar::Header::from_byte_slice(&b)
        .size()
        .map_err(|_| Error::Integrity)?;
    if size > cap || b != canonical_header(name, size)? {
        return Err(Error::Integrity);
    }
    Ok(size)
}
fn padding(input: &mut File, size: u64) -> Result<()> {
    let n = (512 - size % 512) % 512;
    let mut b = [0; 512];
    input
        .read_exact(&mut b[..n as usize])
        .map_err(|_| Error::Integrity)?;
    if b[..n as usize].iter().any(|b| *b != 0) {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn extract(root: &Path, plain: &Path, h: &TrustedHead, start: Instant) -> Result<Manifest> {
    let mut input = files::read_file(plain, PLAIN_LIMIT)?;
    let size = header(&mut input, "manifest.json", 32768)?;
    let mut b = vec![0; size as usize];
    input.read_exact(&mut b).map_err(|_| Error::Integrity)?;
    padding(&mut input, size)?;
    if hash(&b) != h.manifest_sha256 {
        return Err(Error::Integrity);
    }
    let m = decode_manifest(&b)?;
    let expected = head(
        &m,
        &Entry {
            name: String::new(),
            sha256: h.ciphertext_sha256.clone(),
            size: h.ciphertext_size,
        },
    )?;
    if !same(&expected, h)? {
        return Err(Error::Integrity);
    }
    for e in &m.entries {
        deadline(start)?;
        if header(&mut input, &e.name, e.size)? != e.size {
            return Err(Error::Integrity);
        }
        let mut out = files::create_file(&root.join(&e.name))?;
        let mut left = e.size;
        let mut sha = Sha256::new();
        let mut buf = [0; 65536];
        while left > 0 {
            deadline(start)?;
            let n = (left as usize).min(buf.len());
            input
                .read_exact(&mut buf[..n])
                .map_err(|_| Error::Integrity)?;
            out.write_all(&buf[..n]).map_err(|_| Error::Unavailable)?;
            sha.update(&buf[..n]);
            left -= n as u64;
        }
        if backup::hex(&sha.finalize()) != e.sha256 {
            return Err(Error::Integrity);
        }
        out.sync_all().map_err(|_| Error::Unavailable)?;
        padding(&mut input, e.size)?;
    }
    let mut end = [0; 1024];
    input.read_exact(&mut end).map_err(|_| Error::Integrity)?;
    let mut extra = [0; 1];
    if end.iter().any(|b| *b != 0) || input.read(&mut extra).map_err(|_| Error::Integrity)? != 0 {
        return Err(Error::Integrity);
    }
    let seal = read_small(&root.join(MARKER), 16384)?;
    if hash(&seal) != h.seal_sha256 || seal != encode(&m.seal, 16384)? {
        return Err(Error::Integrity);
    }
    Ok(m)
}
fn verify_snapshot(
    conn: &Connection,
    root: &Path,
    m: &Manifest,
) -> Result<(
    crate::journal::Journal,
    crate::knowledge_journal::Journal,
    destructive_intents::Journal,
)> {
    let schema: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if schema != m.seal.schema || !(SCHEMA..=crate::CURRENT_SCHEMA_VERSION).contains(&schema) {
        return Err(Error::UnsupportedInstallation);
    }
    if conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? != "ok"
        || conn.prepare("PRAGMA foreign_key_check")?.exists([])?
    {
        return Err(Error::Integrity);
    }
    let State::Sealed(seal) = state(conn)? else {
        return Err(Error::Integrity);
    };
    if !same(&seal, &m.seal)? {
        return Err(Error::Integrity);
    }
    let config = backup::configuration(conn)?;
    if config.installation_id != seal.installation_id
        || config.active_epoch != Some(seal.key_epoch)
        || config
            .retained_epochs
            .last()
            .is_none_or(|e| e.recipient != seal.recipient)
    {
        return Err(Error::Integrity);
    }
    let auth = crate::journal::Journal::open(&root.join(JOURNALS[0]))?;
    let knowledge = crate::knowledge_journal::Journal::open(&root.join(JOURNALS[1]))?;
    let destructive = destructive_intents::Journal::open(&root.join(JOURNALS[2]))?;
    let (a, k) = backup::journal_heads(conn, &auth, &knowledge)?;
    let (applied, applied_head) = destructive_intents::verify(conn, &knowledge, &destructive)?;
    let (terminal, terminal_head) = destructive
        .records
        .last()
        .map(|r| (r.seq, r.hash))
        .unwrap_or((0, destructive.binding.header_head));
    let coverage = Coverage {
        version: 1,
        schema_baseline: 15,
        legacy_sequence: destructive.binding.legacy_sequence,
        legacy_head: backup::hex(&destructive.binding.legacy_head),
        successor_format: "MSDSTR01".into(),
        successor_journal: JournalHead {
            sequence: terminal,
            head: backup::hex(&terminal_head),
            snapshot_sequence: applied,
            snapshot_head: backup::hex(&applied_head),
        },
    };
    if !same(&a, &seal.auth_journal)?
        || !same(&k, &seal.knowledge_journal)?
        || !same(&coverage, &seal.coverage)?
    {
        return Err(Error::Integrity);
    }
    // Opening journals must not have silently repaired/truncated the pinned inventory.
    for e in &seal.entries {
        let actual = backup::entry(&root.join(&e.name), &e.name, backup::JOURNAL_LIMIT)?;
        if !same(&actual, e)? {
            return Err(Error::Integrity);
        }
    }
    Ok((auth, knowledge, destructive))
}
fn migrate(conn: &Connection, captured: i64) -> Result<()> {
    if captured < RESTORE_SCHEMA {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0020_restore.sql"
        ))?;
    }
    if captured < 21 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0021_media.sql"
        ))?;
    }
    if captured < 22 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0022_schedule_chat.sql"
        ))?;
    }
    if captured < 23 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0023_web_push.sql"
        ))?;
    }
    if captured < 24 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0024_vault_authority.sql"
        ))?;
    }
    if captured < 25 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0025_missions.sql"
        ))?;
    }
    if captured < 26 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0026_calendar.sql"
        ))?;
    }
    if captured < 27 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0027_private_files.sql"
        ))?;
    }
    if captured < 28 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0028_recurring_missions.sql"
        ))?;
    }
    if captured < 29 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0029_mission_chat.sql"
        ))?;
    }
    if captured < 30 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0030_calendar_conversation.sql"
        ))?;
    }
    if captured < 31 {
        conn.execute_batch(include_str!(
            "../../../../migrations/assistant/0031_mail_reconciliation.sql"
        ))?;
    }
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current != crate::CURRENT_SCHEMA_VERSION {
        return Err(Error::UnsupportedInstallation);
    }
    Ok(())
}
fn accounts(conn: &Connection) -> Result<Vec<EnrollmentAccount>> {
    let rows = conn
        .prepare("SELECT id,role,status FROM accounts ORDER BY id LIMIT 4097")?
        .query_map([], |r| {
            Ok(EnrollmentAccount {
                account_id: r.get(0)?,
                role: r.get(1)?,
                status: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if rows.is_empty() || rows.len() > 4096 {
        return Err(Error::LimitExceeded);
    }
    Ok(rows)
}
fn quarantine(
    conn: &mut Connection,
    root: &Path,
    m: &Manifest,
    h: &TrustedHead,
    auth: &mut crate::journal::Journal,
    lineage: &str,
) -> Result<RestoreSummary> {
    let installation_id = Uuid::new_v4().to_string();
    let restore_id = Uuid::new_v4().to_string();
    let users = accounts(conn)?;
    let mut epochs = vec![];
    for a in &users {
        let old: i64 = conn.query_row(
            "SELECT auth_epoch FROM accounts WHERE id=?1",
            [&a.account_id],
            |r| r.get(0),
        )?;
        let next = old.checked_add(1).ok_or(Error::Integrity)?;
        let record = auth.append(&a.account_id, next)?;
        epochs.push((a.account_id.clone(), next, record));
    }
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    migrate(&tx, m.seal.schema)?;
    for (id, epoch, _) in &epochs {
        tx.execute(
            "UPDATE accounts SET auth_epoch=?1,recovery_required=1 WHERE id=?2",
            params![epoch, id],
        )?;
    }
    tx.execute(
        "UPDATE accounts SET management_revision=management_revision+1",
        [],
    )?;
    let last = &epochs.last().ok_or(Error::Integrity)?.2;
    tx.execute(
        "UPDATE store_meta SET journal_seq=?1,journal_head=?2 WHERE singleton=1",
        params![last.seq, last.hash.as_slice()],
    )?;
    tx.execute_batch("UPDATE missions SET grant_enabled=0,state=CASE WHEN state IN('queued','planning','executing','verifying','paused') THEN 'quarantined' ELSE state END; DELETE FROM calendar_oauth_sessions; UPDATE calendar_connections SET status='needs_reconnect',credential=NULL,epoch=epoch+1,credential_revision=credential_revision+1; UPDATE calendar_actions SET state=CASE WHEN state IN('dispatch_committed','outcome_unknown') THEN 'quarantined' ELSE state END,payload=NULL,event=NULL; DELETE FROM sessions; DELETE FROM agent_run_leases; UPDATE agent_execution SET invalidated=1; UPDATE agent_steps SET checkpoint=NULL,completion_digest=NULL,state=CASE WHEN state='dispatched' THEN 'unknown' ELSE state END; UPDATE agent_tool_slots SET input=NULL; DELETE FROM memory_command_plans; DELETE FROM memory_target_plans; DELETE FROM memory_command_slots WHERE state='prepared'; UPDATE chat_runs SET state=CASE WHEN state='running' THEN 'outcome_unknown' ELSE 'cancelled' END,error='interrupted',version=version+1 WHERE state IN('queued','running');
    UPDATE mail_reconciliations SET quarantined=1,lease_nonce=NULL,lease_until=NULL,next_due=NULL,status='restore_quarantined';
    DELETE FROM mail_oauth_sessions; UPDATE mail_actions SET state='outcome_unknown',raw=NULL,content=NULL,version=version+1 WHERE state='dispatch_committed'; UPDATE mail_actions SET state='cancelled',raw=NULL,content=NULL,version=version+1 WHERE state IN('pending_approval','approved'); UPDATE mail_connections SET status='needs_reconnect',credential=NULL,epoch=epoch+1,credential_revision=credential_revision+1;
    UPDATE browser_workspaces SET state='closed',reason='restore_quarantine',revision=revision+1,control_hash=NULL,control_session=NULL,control_until=NULL,resume_request=NULL,resumed_request=NULL;
    UPDATE source_connections SET state='needs_reconnect',sync_enabled=0,token_hash=NULL,token_expires=NULL,credential=NULL,credential_epoch=credential_epoch+1,run_generation=run_generation+1,error='restore_quarantine'; UPDATE source_sync_runs SET state='unknown',error='restore_quarantine' WHERE state='running';
    UPDATE automatic_memory_grants SET enabled=0,revision=revision+1; UPDATE automatic_memory_jobs SET state=CASE WHEN state='queued' THEN 'cancelled' ELSE 'outcome_unknown' END,reason='restore_quarantine',nonce=NULL,dispatch_nonce=NULL,deadline=NULL WHERE state IN('queued','acquired','dispatched');
    DELETE FROM personal_model_selections; DELETE FROM installation_model_selection; DELETE FROM provider_model_probes; DELETE FROM google_setup;")?;
    tx.execute(
        "DELETE FROM personal_artifact_command_plans WHERE used=0",
        [],
    )?;
    crate::calendar::agent::quarantine_restore(&tx)?;
    crate::schedules::quarantine_for_restore(&tx)?;
    tx.execute(
        "UPDATE mission_chat_plans SET state='quarantined' WHERE state IN('awaiting','decided')",
        [],
    )?;
    crate::push::quarantine_for_restore(&tx)?;
    tx.execute_batch("DELETE FROM vault_handoffs; DELETE FROM vault_grants;")?;
    crate::media::quarantine(&tx)?;
    tx.execute("DELETE FROM provider_vision_probes", [])?;
    // Immutable provider metadata stays; move each current pointer to a new disabled
    // revision before using the existing permitted old-ciphertext retirement rule.
    let providers = tx
        .prepare("SELECT id,current_revision FROM providers")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (id, old) in providers {
        let next = old.checked_add(1).ok_or(Error::Integrity)?;
        tx.execute("INSERT INTO provider_revisions(provider_id,revision,name,config,enabled,credential) SELECT provider_id,?1,name,config,0,NULL FROM provider_revisions WHERE provider_id=?2 AND revision=?3",params![next,id,old])?;
        tx.execute(
            "UPDATE providers SET current_revision=?1 WHERE id=?2",
            params![next, id],
        )?;
    }
    tx.execute(
        "UPDATE provider_revisions SET credential=NULL WHERE credential IS NOT NULL",
        [],
    )?;
    tx.execute("DELETE FROM provider_key_meta", [])?;
    tx.execute(
        "UPDATE backup_installation SET installation_id=?1,active_epoch=NULL WHERE singleton=1",
        [&installation_id],
    )?;
    tx.execute("DELETE FROM backup_key_epochs", [])?;
    tx.execute(
        "INSERT INTO installation_restores VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            restore_id,
            installation_id,
            m.seal.installation_id,
            lineage,
            h.seal_id,
            h.checkpoint_id,
            h.ciphertext_sha256,
            String::from_utf8(encode(h, 8192)?).map_err(|_| Error::Integrity)?,
            crate::now()?
        ],
    )?;
    // Only this verified private destination may reset its copied singleton.
    tx.execute_batch("DROP TRIGGER installation_cutover_monotonic; DROP TRIGGER installation_cutover_no_delete; UPDATE installation_cutover SET phase='active',seal=NULL,checkpoint=NULL WHERE singleton=1;")?;
    let migration = include_str!("../../../../migrations/assistant/0019_sealed_cutover.sql");
    let triggers = migration
        .split("CREATE TRIGGER")
        .skip(1)
        .map(|s| {
            format!(
                "CREATE TRIGGER{}",
                s.split("-- Standalone").next().unwrap_or(s)
            )
        })
        .collect::<String>();
    // Exclude the migration's user_version; only its two unchanged guards are restored.
    let triggers = triggers
        .split("PRAGMA user_version")
        .next()
        .ok_or(Error::Integrity)?;
    tx.execute_batch(triggers)?;
    tx.commit()?;
    let _key = crate::providers::open_key(conn, root)?;
    Ok(RestoreSummary {
        installation_id,
        checkpoint_id: h.checkpoint_id.clone(),
        accounts: users,
        already_published: false,
    })
}
fn already_published(dest: &Path, h: &TrustedHead) -> Result<RestoreSummary> {
    let (root, _lock) = files::stopped_lock(dest)?;
    let (_temp, conn) = admission::inspect(&root)?;
    require_active(&conn, &root)?;
    if !table(&conn, "installation_restores")? {
        return Err(Error::OutputExists);
    }
    let installed = backup::configuration(&conn)?.installation_id;
    let saved: Option<String> = conn
        .query_row(
            "SELECT trusted_head FROM installation_restores WHERE new_installation_id=?1",
            [&installed],
            |r| r.get(0),
        )
        .optional()?;
    if saved.as_deref()
        != Some(std::str::from_utf8(&encode(h, 8192)?).map_err(|_| Error::Integrity)?)
    {
        return Err(Error::OutputExists);
    }
    for name in [
        "assistant.sqlite",
        JOURNALS[0],
        JOURNALS[1],
        JOURNALS[2],
        "provider-secrets.key",
        "writer.lock",
    ] {
        files::read_file(&root.join(name), CIPHER_LIMIT)?
            .sync_all()
            .map_err(|_| Error::Unavailable)?;
    }
    files::sync_dir(&root)?;
    files::sync_dir(root.parent().ok_or(Error::Integrity)?)?;
    Ok(RestoreSummary {
        installation_id: installed,
        checkpoint_id: h.checkpoint_id.clone(),
        accounts: accounts(&conn)?,
        already_published: true,
    })
}
/// Restore one exact sealed checkpoint into a new root. Inputs are retained unchanged.
/// `trusted_head` must arrive through an independently trusted operator channel.
pub fn restore(
    ciphertext: &Path,
    trusted_head: &Path,
    identity_file: &Path,
    dest: &Path,
) -> Result<RestoreSummary> {
    let started = Instant::now();
    let h = decode_head(&read_small(trusted_head, 8192)?)?;
    let identity = identity(identity_file)?;
    let dest = destination(dest)?;
    // Verify all supplied inputs on every retry, including after rename succeeded.
    let parent = dest.parent().ok_or(Error::InvalidInput)?;
    let mut stage = Stage::new(parent)?;
    let cipher = stage.path.join("input.age");
    let plain = stage.path.join("archive.tar");
    copy_cipher(ciphertext, &cipher, &h, started)?;
    fault(&dest, "restore_cipher")?;
    decrypt(&cipher, &plain, &identity, started)?;
    let m = extract(&stage.path, &plain, &h, started)?;
    fault(&dest, "restore_extract")?;
    let lock = files::create_file(&stage.path.join("writer.lock"))?;
    lock.try_lock_exclusive().map_err(|_| Error::Busy)?;
    let mut conn = Connection::open_with_flags(
        stage.path.join("assistant.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;")?;
    let (mut auth, knowledge, destructive) = verify_snapshot(&conn, &stage.path, &m)?;
    match fs::symlink_metadata(&dest) {
        Ok(_) => return already_published(&dest, &h),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(_) => return Err(Error::Unavailable),
    }
    fault(&dest, "restore_verified")?;
    let summary = quarantine(
        &mut conn,
        &stage.path,
        &m,
        &h,
        &mut auth,
        &destructive.binding.installation,
    )?;
    fault(&dest, "restore_quarantine")?;
    // Assert the newly admitted shape without ever constructing Store.
    destructive_intents::verify(&conn, &knowledge, &destructive)?;
    backup::journal_heads(&conn, &auth, &knowledge)?;
    backup::configuration(&conn)?;
    if conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? != "ok"
        || conn.prepare("PRAGMA foreign_key_check")?.exists([])?
    {
        return Err(Error::Integrity);
    }
    drop((conn, auth, knowledge, destructive));
    for p in [&cipher, &plain, &stage.path.join(MARKER)] {
        fs::remove_file(p).map_err(|_| Error::Unavailable)?;
    }
    for name in [
        "assistant.sqlite",
        JOURNALS[0],
        JOURNALS[1],
        JOURNALS[2],
        "provider-secrets.key",
        "writer.lock",
    ] {
        files::read_file(&stage.path.join(name), CIPHER_LIMIT)?
            .sync_all()
            .map_err(|_| Error::Unavailable)?;
    }
    files::sync_dir(&stage.path)?;
    deadline(started)?;
    fault(&dest, "restore_publish")?;
    match publication::no_replace(&stage.path, &dest) {
        Ok(()) => stage.published = true,
        Err(Error::OutputExists) => return already_published(&dest, &h),
        Err(e) => return Err(e),
    }
    fault(&dest, "restore_after_rename")?;
    files::sync_dir(parent)?;
    drop(lock);
    Ok(summary)
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod integrated_tests;
