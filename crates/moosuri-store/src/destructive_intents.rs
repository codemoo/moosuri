//! Direct opaque destructive intents. A successful append+fsync is adoption;
//! the effect, receipt and DB watermark commit together before readiness resumes.
use super::*;
use serde::{Deserialize, Serialize};
mod journal;
pub(crate) use journal::{Journal, Record};
#[cfg(test)]
mod tests;
pub(crate) const FILE_NAME: &str = "destructive-intents.journal";
/// Coverage begins at schema15 and is retained across later schema upgrades.
pub(crate) const COVERAGE_SCHEMA: i64 = 15;
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Effect {
    ForgetMemory {
        target: String,
    },
    DeleteSource {
        target: String,
    },
    DeleteCollection {
        target: String,
    },
    DisconnectCollection {
        target: String,
    },
    RevokeSource {
        target: String,
    },
    ForgetCommand {
        target: String,
        revision: i64,
        effective_revision: i64,
        command: String,
    },
    DeleteArtifact {
        target: String,
        request: String,
        revision: i64,
    },
    DeleteMedia {
        target: String,
        account: String,
    },
    DeleteMission {
        target: String,
    },
    DeleteThread {
        target: String,
    },
    DisconnectMail {
        target: String,
        epoch: i64,
        credential_revision: i64,
    },
    DeleteSources {
        targets: Vec<String>,
    },
}
fn validate(tenant: &str, effect: &Effect) -> Result<()> {
    fn id(value: &str) -> Result<()> {
        if Uuid::parse_str(value).is_err() {
            Err(StoreError::InvalidInput)
        } else {
            Ok(())
        }
    }
    id(tenant)?;
    match effect {
        Effect::ForgetMemory { target }
        | Effect::DeleteSource { target }
        | Effect::DeleteCollection { target }
        | Effect::DisconnectCollection { target }
        | Effect::RevokeSource { target }
        | Effect::DeleteMission { target }
        | Effect::DeleteThread { target } => id(target)?,
        Effect::DeleteArtifact {
            target,
            request,
            revision,
        } => {
            id(target)?;
            id(request)?;
            if !(1..=128).contains(revision) {
                return Err(StoreError::InvalidInput);
            }
        }
        Effect::DeleteMedia { target, account } => {
            id(target)?;
            id(account)?;
        }
        Effect::ForgetCommand {
            target,
            revision,
            effective_revision,
            command,
        } => {
            id(target)?;
            id(command)?;
            if *revision <= 0
                || *effective_revision <= 0
                || effective_revision > revision
                || *revision == i64::MAX
            {
                return Err(StoreError::InvalidInput);
            }
        }
        Effect::DisconnectMail {
            target,
            epoch,
            credential_revision,
        } => {
            id(target)?;
            if *epoch < 0
                || *credential_revision < 0
                || *epoch == i64::MAX
                || *credential_revision == i64::MAX
            {
                return Err(StoreError::InvalidInput);
            }
        }
        Effect::DeleteSources { targets } => {
            if targets.is_empty() || targets.len() > 32 {
                return Err(StoreError::InvalidInput);
            }
            let mut seen = std::collections::BTreeSet::new();
            for target in targets {
                id(target)?;
                if !seen.insert(target) {
                    return Err(StoreError::InvalidInput);
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn legacy_effect(action: u8, target: &str) -> Result<Effect> {
    Ok(match action {
        1 => Effect::ForgetMemory {
            target: target.into(),
        },
        2 => Effect::DeleteSource {
            target: target.into(),
        },
        3 => Effect::DeleteCollection {
            target: target.into(),
        },
        4 => Effect::DisconnectCollection {
            target: target.into(),
        },
        6 => Effect::RevokeSource {
            target: target.into(),
        },
        _ => return Err(StoreError::InvalidInput),
    })
}
pub(crate) fn open_or_migrate(
    conn: &Connection,
    dir: &Path,
    legacy: &mut knowledge_journal::Journal,
    migrate: bool,
) -> Result<Journal> {
    let id: String = conn.query_row(
        "SELECT installation_id FROM backup_installation WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let (seq, head) = legacy
        .records
        .last()
        .map(|r| (r.seq, r.hash))
        .unwrap_or((0, [0; 32]));
    let journal = if migrate {
        let journal = Journal::initialize(&dir.join(FILE_NAME), &id, seq, head)?;
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(include_str!(
            "../../../migrations/assistant/0015_destructive_intents.sql"
        ))?;
        tx.execute(
            "INSERT INTO destructive_journal_meta VALUES(1,?1,?2,?3,0,?4)",
            params![
                id,
                seq,
                head.as_slice(),
                journal.binding.header_head.as_slice()
            ],
        )?;
        tx.commit()?;
        journal
    } else {
        Journal::open(&dir.join(FILE_NAME))?
    };
    verify(conn, legacy, &journal)?;
    legacy.freeze();
    Ok(journal)
}
/// Validates both complete frozen predecessor and DB prefix without replay/mutation.
pub(crate) fn verify(
    conn: &Connection,
    legacy: &knowledge_journal::Journal,
    journal: &Journal,
) -> Result<(i64, [u8; 32])> {
    let (id,seq,head,applied,applied_head):(String,i64,Vec<u8>,i64,Vec<u8>)=conn.query_row("SELECT installation_id,legacy_sequence,legacy_head,sequence,head FROM destructive_journal_meta WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let installed: String = conn.query_row(
        "SELECT installation_id FROM backup_installation WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    if id != installed && !crate::cutover::restore::journal_lineage_matches(conn, &installed, &id)?
    {
        return Err(StoreError::Corrupt);
    }
    journal.check_binding(&id, seq, &head)?;
    let (actual_seq, actual_head) = legacy
        .records
        .last()
        .map(|r| (r.seq, r.hash))
        .unwrap_or((0, [0; 32]));
    if seq != actual_seq || head != actual_head {
        return Err(StoreError::Corrupt);
    }
    if applied < 0 || applied as usize > journal.records.len() {
        return Err(StoreError::Corrupt);
    }
    let expected = if applied == 0 {
        journal.binding.header_head
    } else {
        journal.records[applied as usize - 1].hash
    };
    if applied_head != expected {
        return Err(StoreError::Corrupt);
    }
    Ok((applied, expected))
}
pub(crate) fn commit(db: &mut Database, tenant: &str, effect: Effect) -> Result<()> {
    let prepared = db.destructive_journal.prepare(tenant, effect)?;
    db.poisoned = true;
    let record = db.destructive_journal.append(prepared)?;
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    apply(&tx, &record)?;
    tx.commit()?;
    db.poisoned = false;
    Ok(())
}
pub(crate) fn apply(conn: &Connection, record: &Record) -> Result<()> {
    let (seq, head): (i64, Vec<u8>) = conn.query_row(
        "SELECT sequence,head FROM destructive_journal_meta WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if seq == record.seq && head == record.hash {
        return Ok(());
    }
    if seq.checked_add(1) != Some(record.seq) || head != record.previous {
        return Err(StoreError::Corrupt);
    }
    match &record.effect {
        Effect::ForgetMemory { target } => {
            knowledge::destructive::apply_body(conn, &record.tenant, target, 1)?
        }
        Effect::DeleteSource { target } => {
            knowledge::destructive::apply_body(conn, &record.tenant, target, 2)?
        }
        Effect::DeleteCollection { target } => {
            knowledge::destructive::apply_body(conn, &record.tenant, target, 3)?
        }
        Effect::DisconnectCollection { target } => {
            knowledge::destructive::apply_body(conn, &record.tenant, target, 4)?
        }
        Effect::RevokeSource { target } => {
            knowledge::destructive::apply_body(conn, &record.tenant, target, 6)?
        }
        Effect::ForgetCommand {
            target,
            revision,
            effective_revision,
            command,
        } => {
            knowledge::bump(conn, &record.tenant)?;
            memory_commands::apply_direct_forget(
                conn,
                &record.tenant,
                command,
                target,
                *revision,
                *effective_revision,
            )?;
        }
        Effect::DeleteArtifact {
            target,
            request,
            revision,
        } => artifacts::apply_delete(
            conn,
            &record.tenant,
            target,
            &moosuri_core::ArtifactDelete {
                request_id: request.clone(),
                expected_revision: *revision,
            },
        )?,
        Effect::DeleteMedia { target, account } => {
            crate::media::apply_delete(conn, &record.tenant, account, target)?
        }
        Effect::DeleteMission { target } => {
            crate::missions::apply_delete(conn, &record.tenant, target)?;
        }
        Effect::DeleteThread { target } => {
            mail::purge_thread(conn, &record.tenant, target)?;
            conn.execute(
                "DELETE FROM threads WHERE tenant_id=?1 AND id=?2",
                params![record.tenant, target],
            )?;
        }
        Effect::DisconnectMail {
            target,
            epoch,
            credential_revision,
        } => mail::apply_disconnect(conn, &record.tenant, target, *epoch, *credential_revision)?,
        Effect::DeleteSources { targets } => {
            for target in targets {
                knowledge::destructive::apply_body(conn, &record.tenant, target, 2)?;
            }
        }
    }
    conn.execute(
        "UPDATE destructive_journal_meta SET sequence=?1,head=?2 WHERE singleton=1",
        params![record.seq, record.hash.as_slice()],
    )?;
    Ok(())
}
pub(crate) fn replay(db: &mut Database) -> Result<()> {
    let (sequence, _) = verify(&db.conn, &db.knowledge_journal, &db.destructive_journal)?;
    // The predecessor replay is deliberately completed first by Store::open.
    let legacy_applied: i64 = db.conn.query_row(
        "SELECT sequence FROM knowledge_journal_meta WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    if legacy_applied != db.destructive_journal.binding.legacy_sequence {
        return Err(StoreError::Corrupt);
    }
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    for record in db
        .destructive_journal
        .records
        .iter()
        .skip(sequence as usize)
    {
        apply(&tx, record)?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn adopt_unapplied_for_test(
    db: &mut Database,
    tenant: &str,
    target: &str,
    action: u8,
) -> Result<()> {
    // Mirrors only the durable adoption boundary; callers model a dead process
    // and may arrange synthetic post-adoption state before reopening.
    let effect = if action == 5 {
        let (id,revision,effective):(String,i64,i64)=db.conn.query_row(
            "SELECT target_id,target_revision,target_effective_revision FROM memory_command_slots WHERE tenant_id=?1 AND command_id=?2",
            params![tenant,target],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        Effect::ForgetCommand {
            target: id,
            revision,
            effective_revision: effective,
            command: target.into(),
        }
    } else {
        legacy_effect(action, target)?
    };
    let prepared = db.destructive_journal.prepare(tenant, effect)?;
    db.destructive_journal.append(prepared)?;
    Ok(())
}
