//! Manual non-sealing journal publication and pinned historical export restore.
//! A trusted digest must be retained independently. Local timestamps cannot prove freshness.
use super::*;

const PURPOSE: &str = "assistant-historical-journal-v1";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalManifest {
    format: u32,
    purpose: String,
    installation_id: String,
    checkpoint_id: String,
    schema: i64,
    key_epoch: i64,
    recipient: String,
    entries: Vec<Entry>,
    auth_journal: JournalHead,
    knowledge_journal: JournalHead,
    coverage: Coverage,
    export: Entry,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LatestHead {
    format: u32,
    purpose: String,
    manifest: JournalManifest,
    manifest_sha256: String,
    ciphertext: Entry,
}
pub struct JournalPublication {
    pub checkpoint_id: String,
    /// Retain through a channel independent of the installation and archive files.
    pub trusted_head_sha256: String,
}
fn same_entry(a: &Entry, b: &Entry) -> Result<()> {
    if !same(a, b)? {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn ciphertext_entry(path: &Path, name: &str) -> Result<Entry> {
    backup::entry(path, name, CIPHER_LIMIT)
}
fn journal_state(o: &backup::Offline, export: Entry) -> Result<JournalManifest> {
    let config = backup::configuration(&o.conn)?;
    let epoch = config.retained_epochs.last().ok_or(Error::Unconfigured)?;
    let (auth, knowledge) = backup::journal_heads(&o.conn, &o.auth, &o.knowledge)?;
    let (applied, applied_head) =
        destructive_intents::verify(&o.conn, &o.knowledge, &o.destructive)?;
    let (complete, complete_head) = o
        .destructive
        .records
        .last()
        .map(|r| (r.seq, r.hash))
        .unwrap_or((0, o.destructive.binding.header_head));
    if auth.sequence != auth.snapshot_sequence
        || knowledge.sequence != knowledge.snapshot_sequence
        || applied != complete
    {
        return Err(Error::JournalPending);
    }
    let mut entries = vec![];
    for name in JOURNALS {
        entries.push(backup::entry(
            &o.temp.0.join(name),
            name,
            backup::JOURNAL_LIMIT,
        )?);
    }
    Ok(JournalManifest {
        format: 1,
        purpose: PURPOSE.into(),
        installation_id: config.installation_id,
        checkpoint_id: Uuid::new_v4().to_string(),
        schema: crate::CURRENT_SCHEMA_VERSION,
        key_epoch: epoch.epoch,
        recipient: epoch.recipient.clone(),
        entries,
        auth_journal: auth,
        knowledge_journal: knowledge,
        coverage: Coverage {
            version: 1,
            schema_baseline: 15,
            legacy_sequence: o.destructive.binding.legacy_sequence,
            legacy_head: backup::hex(&o.destructive.binding.legacy_head),
            successor_format: "MSDSTR01".into(),
            successor_journal: JournalHead {
                sequence: complete,
                head: backup::hex(&complete_head),
                snapshot_sequence: applied,
                snapshot_head: backup::hex(&applied_head),
            },
        },
        export,
    })
}
fn validate(m: &JournalManifest) -> Result<()> {
    if m.format != 1
        || m.purpose != PURPOSE
        || !(RESTORE_SCHEMA..=crate::CURRENT_SCHEMA_VERSION).contains(&m.schema)
        || m.key_epoch <= 0
        || m.recipient.parse::<age::x25519::Recipient>().is_err()
        || [&m.installation_id, &m.checkpoint_id]
            .iter()
            .any(|v| Uuid::parse_str(v).is_err())
        || m.entries.len() != 3
        || m.export.name != "export.age"
        || !digest(&m.export.sha256)
        || m.export.size == 0
        || m.export.size > CIPHER_LIMIT
        || m.coverage.version != 1
        || m.coverage.schema_baseline != 15
        || m.coverage.successor_format != "MSDSTR01"
        || m.coverage.legacy_sequence != m.knowledge_journal.sequence
        || m.coverage.legacy_head != m.knowledge_journal.head
    {
        return Err(Error::Integrity);
    }
    for (e, n) in m.entries.iter().zip(JOURNALS) {
        if e.name != n || !digest(&e.sha256) || e.size > backup::JOURNAL_LIMIT {
            return Err(Error::Integrity);
        }
    }
    for j in [
        &m.auth_journal,
        &m.knowledge_journal,
        &m.coverage.successor_journal,
    ] {
        if j.sequence < 0
            || j.sequence != j.snapshot_sequence
            || j.head != j.snapshot_head
            || !digest(&j.head)
        {
            return Err(Error::Integrity);
        }
    }
    Ok(())
}
/// Capture complete journals under the stopped writer lock, without sealing or
/// changing the source. The head binds one selected retained encrypted export.
/// Publication is local and manual; carry its digest through an independent channel.
pub fn publish_journal(
    root: &Path,
    export: &Path,
    output: &Path,
    head_output: &Path,
) -> Result<JournalPublication> {
    let start = Instant::now();
    let o = backup::Offline::open(root)?;
    let output = files::output_path(&o.root, output)?;
    let head_output = files::output_path(&o.root, head_output)?;
    let out_parent = output.parent().ok_or(Error::InvalidInput)?;
    let head_parent = head_output.parent().ok_or(Error::InvalidInput)?;
    if out_parent.starts_with(head_parent) || head_parent.starts_with(out_parent) {
        return Err(Error::InvalidInput);
    }
    let m = journal_state(&o, ciphertext_entry(export, "export.age")?)?;
    validate(&m)?;
    let encoded = encode(&m, 32768)?;
    let partial = out_parent.join(format!(".moosuri-journal-{}.partial", Uuid::new_v4()));
    let head_partial = head_parent.join(format!(".moosuri-head-{}.partial", Uuid::new_v4()));
    let result = (|| {
        let f = files::create_file(&partial)?;
        let f = backup::write_archive(
            f,
            &m.recipient.parse().map_err(|_| Error::Integrity)?,
            &m.entries,
            &encoded,
            &o.temp.0.join("unused.sqlite"),
            &o.temp.0,
            start,
        )?;
        f.sync_all().map_err(|_| Error::Unavailable)?;
        let h = LatestHead {
            format: 1,
            purpose: format!("{PURPOSE}-head"),
            manifest: m.clone(),
            manifest_sha256: hash(&encoded),
            ciphertext: ciphertext_entry(&partial, "journal.age")?,
        };
        let bytes = encode(&h, 8192)?;
        let mut f = files::create_file(&head_partial)?;
        f.write_all(&bytes).map_err(|_| Error::Unavailable)?;
        f.sync_all().map_err(|_| Error::Unavailable)?;
        deadline(start)?;
        // Ciphertext first: a failed head publication cannot advertise a missing journal.
        files::publish(&partial, &output)?;
        fault(&o.root, "journal_before_head_publish")?;
        files::publish(&head_partial, &head_output)?;
        Ok(JournalPublication {
            checkpoint_id: m.checkpoint_id,
            trusted_head_sha256: hash(&bytes),
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
        let _ = fs::remove_file(&head_partial);
    }
    result
}
fn extract_archive(
    root: &Path,
    plain: &Path,
    expected: Option<&JournalManifest>,
    start: Instant,
) -> Result<Vec<u8>> {
    let mut input = files::read_file(plain, PLAIN_LIMIT)?;
    let size = header(&mut input, "manifest.json", 32768)?;
    let mut bytes = vec![0; size as usize];
    input.read_exact(&mut bytes).map_err(|_| Error::Integrity)?;
    padding(&mut input, size)?;
    let entries = if let Some(m) = expected {
        if bytes != encode(m, 32768)? {
            return Err(Error::Integrity);
        }
        m.entries.clone()
    } else {
        let backup::DecodedManifest::V2(m) = backup::decode_manifest(&bytes)? else {
            return Err(Error::UnsupportedInstallation);
        };
        if m.purpose != "assistant-export"
            || !(RESTORE_SCHEMA..=crate::CURRENT_SCHEMA_VERSION).contains(&m.schema)
            || !m.authoritative_files.is_empty()
        {
            return Err(Error::UnsupportedInstallation);
        }
        let names = ["assistant.sqlite", JOURNALS[0], JOURNALS[1], JOURNALS[2]];
        if m.entries.iter().map(|e| e.name.as_str()).ne(names) {
            return Err(Error::Integrity);
        }
        m.entries
    };
    for e in entries {
        deadline(start)?;
        if header(&mut input, &e.name, e.size)? != e.size {
            return Err(Error::Integrity);
        }
        let mut out = files::create_file(&root.join(&e.name))?;
        let mut limited = std::io::Read::by_ref(&mut input).take(e.size);
        if io::copy(&mut limited, &mut out).map_err(|_| Error::Integrity)? != e.size {
            return Err(Error::Integrity);
        }
        out.sync_all().map_err(|_| Error::Unavailable)?;
        drop(out);
        same_entry(&backup::entry(&root.join(&e.name), &e.name, e.size)?, &e)?;
        padding(&mut input, e.size)?;
    }
    let mut end = [0; 1024];
    input.read_exact(&mut end).map_err(|_| Error::Integrity)?;
    let mut extra = [0; 1];
    if end.iter().any(|b| *b != 0) || input.read(&mut extra).map_err(|_| Error::Integrity)? != 0 {
        return Err(Error::Integrity);
    }
    Ok(bytes)
}
fn copy_verified(path: &Path, out: &Path, expected: &Entry) -> Result<()> {
    files::copy_file(path, out, CIPHER_LIMIT)?;
    same_entry(&ciphertext_entry(out, &expected.name)?, expected)
}
fn prefix(old: &Path, new: &Path) -> Result<()> {
    let mut a = files::read_file(old, backup::JOURNAL_LIMIT)?;
    let mut b = files::read_file(new, backup::JOURNAL_LIMIT)?;
    let mut x = [0; 65536];
    let mut y = [0; 65536];
    loop {
        let n = a.read(&mut x).map_err(|_| Error::Integrity)?;
        if n == 0 {
            return Ok(());
        }
        b.read_exact(&mut y[..n]).map_err(|_| Error::Integrity)?;
        if x[..n] != y[..n] {
            return Err(Error::Integrity);
        }
    }
}
/// Restore an authenticated historical export plus an explicitly pinned later
/// journal. The caller must obtain the digest from its independent latestness
/// record; a stale digest supplied by the caller cannot be detected locally.
pub fn restore_historical(
    export: &Path,
    journal: &Path,
    trusted_head: &Path,
    latest_head_sha256: &str,
    export_identity: &Path,
    journal_identity: &Path,
    dest: &Path,
) -> Result<RestoreSummary> {
    let start = Instant::now();
    let bytes = read_small(trusted_head, 8192)?;
    if !digest(latest_head_sha256) || hash(&bytes) != latest_head_sha256 {
        return Err(Error::Integrity);
    }
    let h: LatestHead = serde_json::from_slice(&bytes).map_err(|_| Error::Integrity)?;
    validate(&h.manifest)?;
    if h.format != 1
        || h.purpose != format!("{PURPOSE}-head")
        || h.manifest_sha256 != hash(&encode(&h.manifest, 32768)?)
        || h.ciphertext.name != "journal.age"
        || !digest(&h.ciphertext.sha256)
        || h.ciphertext.size == 0
        || h.ciphertext.size > CIPHER_LIMIT
    {
        return Err(Error::Integrity);
    }
    let load_key = |p: &Path| {
        identity(p).map_err(|e| {
            if e == Error::InvalidInput {
                Error::KeyMissing
            } else {
                e
            }
        })
    };
    let export_key = load_key(export_identity)?;
    let journal_key = load_key(journal_identity)?;
    let dest = destination(dest)?;
    let parent = dest.parent().ok_or(Error::InvalidInput)?;
    let mut stage = Stage::new(parent)?;
    let journals = stage.path.join("latest");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&journals)
        .map_err(|_| Error::Unavailable)?;
    let ec = stage.path.join("export.age");
    let jc = stage.path.join("journal.age");
    let ep = stage.path.join("export.tar");
    let jp = stage.path.join("journal.tar");
    copy_verified(export, &ec, &h.manifest.export)?;
    copy_verified(journal, &jc, &h.ciphertext)?;
    decrypt(&ec, &ep, &export_key, start)?;
    decrypt(&jc, &jp, &journal_key, start)?;
    let backup::DecodedManifest::V2(m) =
        backup::decode_manifest(&extract_archive(&stage.path, &ep, None, start)?)?
    else {
        return Err(Error::UnsupportedInstallation);
    };
    extract_archive(&journals, &jp, Some(&h.manifest), start)?;
    if m.schema > h.manifest.schema
        || m.installation_id != h.manifest.installation_id
        || m.coverage.legacy_sequence != h.manifest.coverage.legacy_sequence
        || m.coverage.legacy_head != h.manifest.coverage.legacy_head
    {
        return Err(Error::Integrity);
    }
    let lock = files::create_file(&stage.path.join("writer.lock"))?;
    lock.try_lock_exclusive().map_err(|_| Error::Busy)?;
    let mut conn = Connection::open_with_flags(
        stage.path.join("assistant.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA journal_mode=DELETE;")?;
    if conn.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))? != m.schema
        || !matches!(state(&conn)?, State::Active)
        || backup::configuration(&conn)?.installation_id != m.installation_id
    {
        return Err(Error::Integrity);
    }
    let old_auth = crate::journal::Journal::open(&stage.path.join(JOURNALS[0]))?;
    let old_knowledge = crate::knowledge_journal::Journal::open(&stage.path.join(JOURNALS[1]))?;
    let old_destructive = destructive_intents::Journal::open(&stage.path.join(JOURNALS[2]))?;
    let (a, k) = backup::journal_heads(&conn, &old_auth, &old_knowledge)?;
    if !same(&a, &m.auth_journal)? || !same(&k, &m.knowledge_journal)? {
        return Err(Error::Integrity);
    }
    let config = backup::configuration(&conn)?;
    if config.active_epoch != Some(m.key_epoch)
        || m.key_epoch > h.manifest.key_epoch
        || config
            .retained_epochs
            .last()
            .is_none_or(|e| e.recipient != m.recipient)
    {
        return Err(Error::Integrity);
    }
    let (seq, head) = destructive_intents::verify(&conn, &old_knowledge, &old_destructive)?;
    if seq != m.coverage.successor_journal.snapshot_sequence
        || backup::hex(&head) != m.coverage.successor_journal.snapshot_head
        || old_destructive
            .records
            .last()
            .map(|r| (r.seq, backup::hex(&r.hash)))
            .unwrap_or((0, backup::hex(&old_destructive.binding.header_head)))
            != (
                m.coverage.successor_journal.sequence,
                m.coverage.successor_journal.head.clone(),
            )
    {
        return Err(Error::Integrity);
    }
    for e in m.entries.iter().filter(|e| e.name != "assistant.sqlite") {
        same_entry(
            &backup::entry(&stage.path.join(&e.name), &e.name, backup::DB_LIMIT)?,
            e,
        )?;
    }
    drop((old_auth, old_knowledge, old_destructive));
    for name in JOURNALS {
        prefix(&stage.path.join(name), &journals.join(name))?;
        fs::remove_file(stage.path.join(name)).map_err(|_| Error::Unavailable)?;
        files::copy_file(
            &journals.join(name),
            &stage.path.join(name),
            backup::JOURNAL_LIMIT,
        )?;
    }
    let mut auth = crate::journal::Journal::open(&stage.path.join(JOURNALS[0]))?;
    let knowledge = crate::knowledge_journal::Journal::open(&stage.path.join(JOURNALS[1]))?;
    let destructive = destructive_intents::Journal::open(&stage.path.join(JOURNALS[2]))?;
    // Validate exact bytes after journal codecs, which may otherwise trim a torn tail.
    for e in &h.manifest.entries {
        same_entry(
            &backup::entry(&stage.path.join(&e.name), &e.name, backup::JOURNAL_LIMIT)?,
            e,
        )?;
    }
    let (a, k) = backup::journal_heads(&conn, &auth, &knowledge)?;
    let (_, _) = destructive_intents::verify(&conn, &knowledge, &destructive)?;
    if a.sequence != h.manifest.auth_journal.sequence
        || a.head != h.manifest.auth_journal.head
        || k.sequence != h.manifest.knowledge_journal.sequence
        || k.head != h.manifest.knowledge_journal.head
        || destructive
            .records
            .last()
            .map(|r| (r.seq, backup::hex(&r.hash)))
            .unwrap_or((0, backup::hex(&destructive.binding.header_head)))
            != (
                h.manifest.coverage.successor_journal.sequence,
                h.manifest.coverage.successor_journal.head.clone(),
            )
    {
        return Err(Error::Integrity);
    }
    let receipt = serde_json::json!({"format":1,"purpose":"assistant-historical-restore-v1","accounting_state":"accounting_unknown","latest_head_sha256":latest_head_sha256,"publication":h});
    let trusted_receipt =
        String::from_utf8(encode(&receipt, 8192)?).map_err(|_| Error::Integrity)?;
    match fs::symlink_metadata(&dest) {
        Ok(_) => return published_provenance(&dest, &h.manifest.checkpoint_id, &trusted_receipt),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(_) => return Err(Error::Unavailable),
    }
    migrate(&conn, m.schema)?;
    replay(&mut conn, &auth, &knowledge, &destructive)?;
    let provenance = Provenance {
        schema: crate::CURRENT_SCHEMA_VERSION,
        source_installation: m.installation_id,
        recovery_id: Uuid::new_v4().to_string(),
        checkpoint_id: h.manifest.checkpoint_id.clone(),
        ciphertext_sha256: h.manifest.export.sha256.clone(),
        trusted_head: trusted_receipt,
    };
    let summary = quarantine(
        &mut conn,
        &stage.path,
        &provenance,
        &mut auth,
        &destructive.binding.installation,
    )?;
    crate::accounts::finish_deletions(&mut conn)?;
    backup::journal_heads(&conn, &auth, &knowledge)?;
    destructive_intents::verify(&conn, &knowledge, &destructive)?;
    if conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? != "ok"
        || conn.prepare("PRAGMA foreign_key_check")?.exists([])?
    {
        return Err(Error::Integrity);
    }
    drop((conn, auth, knowledge, destructive));
    for p in [&ec, &jc, &ep, &jp] {
        fs::remove_file(p).map_err(|_| Error::Unavailable)?;
    }
    fs::remove_dir_all(journals).map_err(|_| Error::Unavailable)?;
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
    deadline(start)?;
    match publication::no_replace(&stage.path, &dest) {
        Ok(()) => stage.published = true,
        Err(Error::OutputExists) => {
            return published_provenance(
                &dest,
                &h.manifest.checkpoint_id,
                &provenance.trusted_head,
            );
        }
        Err(e) => return Err(e),
    }
    fault(&dest, "historical_after_rename")?;
    files::sync_dir(parent)?;
    drop(lock);
    Ok(summary)
}

fn exists(c: &Connection, table: &str, tenant: &str, id: &str) -> Result<bool> {
    // Table identifiers are fixed internal call sites, never journal/user text.
    Ok(c.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE tenant_id=?1 AND id=?2)"),
        params![tenant, id],
        |r| r.get(0),
    )?)
}
fn withdraw(c: &Connection, r: &destructive_intents::Record) -> Result<()> {
    use destructive_intents::Effect::*;
    let t = &r.tenant;
    if !c.query_row(
        "SELECT EXISTS(SELECT 1 FROM accounts WHERE tenant_id=?1)",
        [t],
        |r| r.get::<_, bool>(0),
    )? {
        return Ok(());
    }
    let basic = match &r.effect {
        ForgetMemory { target } | ForgetCommand { target, .. } => {
            Some(("knowledge_memories", target, 1))
        }
        DeleteSource { target } => Some(("knowledge_sources", target, 2)),
        DeleteCollection { target } => Some(("knowledge_collections", target, 3)),
        DisconnectCollection { target } => Some(("knowledge_collections", target, 4)),
        RevokeSource { target } => Some(("source_connections", target, 6)),
        _ => None,
    };
    if let Some((table, target, action)) = basic {
        if exists(c, table, t, target)? {
            crate::knowledge::destructive::apply_body(c, t, target, action)?;
        }
        return Ok(());
    }
    match &r.effect {
        DeleteSources { targets } => {
            for target in targets {
                if exists(c, "knowledge_sources", t, target)? {
                    crate::knowledge::destructive::apply_body(c, t, target, 2)?;
                }
            }
        }
        DeleteArtifact { target, .. } => {
            let current: Option<(i64, String)> = c
                .query_row(
                    "SELECT revision,status FROM personal_artifacts WHERE tenant_id=?1 AND id=?2",
                    params![t, target],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            if let Some((revision, status)) = current
                && status != "deleted"
            {
                // Historical replay is a conservative withdrawal, not a
                // fabricated successful mutation receipt at another revision.
                crate::artifacts::invalidate_dependency(c, t, "artifact", target, None)?;
                c.execute("UPDATE personal_artifact_versions SET status='deleted',draft=NULL WHERE tenant_id=?1 AND artifact_id=?2",params![t,target])?;
                let next = revision.checked_add(1).ok_or(Error::Integrity)?;
                c.execute("UPDATE personal_artifacts SET revision=?3,status='deleted',updated_at=?4 WHERE tenant_id=?1 AND id=?2",params![t,target,next,crate::now()?])?;
                c.execute(
                    "INSERT INTO personal_artifact_versions VALUES(?1,?2,?3,'deleted',NULL,?4)",
                    params![t, target, next, crate::now()?],
                )?;
                crate::knowledge::bump(c, t)?;
            }
        }
        DisconnectMail {
            target,
            epoch,
            credential_revision,
        } => {
            if exists(c, "mail_connections", t, target)? {
                c.execute("UPDATE mail_connections SET epoch=MAX(epoch,?3),credential_revision=MAX(credential_revision,?4) WHERE tenant_id=?1 AND id=?2",params![t,target,epoch,credential_revision])?;
                let (e,v):(i64,i64)=c.query_row("SELECT epoch,credential_revision FROM mail_connections WHERE tenant_id=?1 AND id=?2",params![t,target],|r|Ok((r.get(0)?,r.get(1)?)))?;
                crate::mail::apply_disconnect(c, t, target, e, v)?;
            }
        }
        // These existing handlers are target-idempotent and do not require an
        // intermediate revision. Reuse them but leave watermark updates here.
        DeleteMedia { .. }
        | DeleteMission { .. }
        | DeleteThread { .. }
        | DisconnectCalendar { .. } => destructive_intents::apply(c, r)?,
        _ => return Err(Error::Integrity),
    }
    Ok(())
}
fn replay(
    c: &mut Connection,
    auth: &crate::journal::Journal,
    knowledge: &crate::knowledge_journal::Journal,
    destructive: &destructive_intents::Journal,
) -> Result<()> {
    let (a, k) = backup::journal_heads(c, auth, knowledge)?;
    // Format2's predecessor is permanently frozen. Historical snapshots with an
    // unapplied legacy suffix are deliberately unsupported by this bounded path.
    if k.snapshot_sequence != k.sequence {
        return Err(Error::JournalPending);
    }
    let (d, _) = destructive_intents::verify(c, knowledge, destructive)?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    for r in auth.records.iter().skip(a.snapshot_sequence as usize) {
        // UUIDs cannot be reused. An account created after the snapshot has no
        // restored bytes, and its epoch record must still advance the lineage.
        if r.delete {
            crate::accounts::mark_deleted(&tx, &r.account, r.epoch)?;
        }
        tx.execute(
            "UPDATE accounts SET auth_epoch=MAX(auth_epoch,?1),recovery_required=1 WHERE id=?2",
            params![r.epoch, r.account],
        )?;
        tx.execute("DELETE FROM sessions WHERE account_id=?1", [&r.account])?;
        tx.execute(
            "UPDATE store_meta SET journal_seq=?1,journal_head=?2 WHERE singleton=1",
            params![r.seq, r.hash.as_slice()],
        )?;
    }
    for r in destructive.records.iter().skip(d as usize) {
        let (seq, head): (i64, Vec<u8>) = tx.query_row(
            "SELECT sequence,head FROM destructive_journal_meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if seq.checked_add(1) != Some(r.seq) || head != r.previous {
            return Err(Error::Integrity);
        }
        withdraw(&tx, r)?;
        tx.execute(
            "UPDATE destructive_journal_meta SET sequence=?1,head=?2 WHERE singleton=1",
            params![r.seq, r.hash.as_slice()],
        )?;
    }
    // Latest role and suspension state are not reconstructible from opaque auth
    // epochs. Every restored role is conservative; local enrollment explicitly
    // chooses a current role, and old suspended status remains suspended.
    tx.execute("UPDATE accounts SET role='member',recovery_required=1", [])?;
    tx.commit()?;
    Ok(())
}
#[cfg(test)]
mod tests;

/// Historical accounting has a gap after the snapshot. Until an explicit new
/// allocation/reconcile flow exists, every model binding and usage-total view is
/// unavailable. Fresh credentials cannot erase this immutable recovery condition.
pub(crate) fn require_accounting_known(conn: &Connection) -> crate::Result<()> {
    let unknown:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM installation_restores r WHERE json_extract(r.trusted_head,'$.purpose')='assistant-historical-restore-v1' AND json_extract(r.trusted_head,'$.accounting_state')='accounting_unknown')",[],|r|r.get(0))?;
    if unknown {
        Err(crate::StoreError::Unavailable)
    } else {
        Ok(())
    }
}
