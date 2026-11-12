use super::*;
pub(crate) fn forget(conn: &Connection, tenant: &str, object: &str) -> Result<()> {
    let (status, revision): (String, i64) = conn
        .query_row(
            "SELECT status,revision FROM knowledge_memories WHERE tenant_id=?1 AND id=?2",
            params![tenant, object],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(StoreError::Corrupt)?;
    if status == "forgotten" {
        return Ok(());
    }
    agent_execution::invalidate_object(conn, tenant, "memory", object, None)?;
    conn.execute("INSERT OR IGNORE INTO knowledge_suppressions SELECT tenant_id,source_id,quote_hash FROM knowledge_evidence WHERE tenant_id=?1 AND memory_id=?2",params![tenant,object])?;
    conn.execute(
        "DELETE FROM knowledge_memory_versions WHERE tenant_id=?1 AND memory_id=?2",
        params![tenant, object],
    )?;
    conn.execute(
        "DELETE FROM knowledge_memory_timeline WHERE tenant_id=?1 AND memory_id=?2",
        params![tenant, object],
    )?;
    conn.execute("UPDATE knowledge_memories SET status='forgotten',revision=?3,updated_at=?4 WHERE tenant_id=?1 AND id=?2",params![tenant,object,revision.checked_add(1).ok_or(StoreError::Corrupt)?,now()?])?;
    remove_index(conn, tenant, "memory", object)
}
fn delete_source(conn: &Connection, tenant: &str, object: &str) -> Result<()> {
    let (deleted, revision): (bool, i64) = conn
        .query_row(
            "SELECT deleted,revision FROM knowledge_sources WHERE tenant_id=?1 AND id=?2",
            params![tenant, object],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(StoreError::Corrupt)?;
    if deleted {
        return Ok(());
    }
    agent_execution::invalidate_source(conn, tenant, object)?;
    let dependent = conn
        .prepare(
            "SELECT DISTINCT memory_id FROM knowledge_evidence WHERE tenant_id=?1 AND source_id=?2",
        )?
        .query_map(params![tenant, object], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for memory in dependent {
        let current_depends=conn.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_evidence e JOIN knowledge_memories m ON m.tenant_id=e.tenant_id AND m.id=e.memory_id AND m.revision=e.memory_revision WHERE e.tenant_id=?1 AND e.source_id=?2 AND e.memory_id=?3)",params![tenant,object,memory],|r|r.get::<_,bool>(0))?;
        if current_depends {
            forget(conn, tenant, &memory)?;
        } else {
            // A later independent assertion survives; remove dependent historical text
            // while retaining its content-free temporal supersession barrier.
            conn.execute("INSERT OR IGNORE INTO knowledge_suppressions SELECT tenant_id,source_id,quote_hash FROM knowledge_evidence WHERE tenant_id=?1 AND source_id=?2 AND memory_id=?3",params![tenant,object,memory])?;
            conn.execute("DELETE FROM knowledge_memory_versions WHERE tenant_id=?1 AND memory_id=?2 AND revision IN (SELECT memory_revision FROM knowledge_evidence WHERE tenant_id=?1 AND memory_id=?2 AND source_id=?3)",params![tenant,memory,object])?;
            memories::reindex(conn, tenant, &memory)?;
        }
    }
    conn.execute(
        "DELETE FROM knowledge_source_versions WHERE tenant_id=?1 AND source_id=?2",
        params![tenant, object],
    )?;
    conn.execute(
        "UPDATE knowledge_sources SET deleted=1,revision=?3 WHERE tenant_id=?1 AND id=?2",
        params![
            tenant,
            object,
            revision.checked_add(1).ok_or(StoreError::Corrupt)?
        ],
    )?;
    remove_index(conn, tenant, "source", object)
}
pub(crate) fn apply(conn: &Connection, record: &knowledge_journal::Record) -> Result<()> {
    match record.action {
        1 => forget(conn, &record.tenant, &record.target)?,
        2 => delete_source(conn, &record.tenant, &record.target)?,
        3 | 4 => {
            let group = collection(conn, &record.tenant, &record.target)
                .map_err(|_| StoreError::Corrupt)?;
            if record.action == 3 && !group.deleted {
                let sources=conn.prepare("SELECT id FROM knowledge_sources WHERE tenant_id=?1 AND collection_id=?2 AND deleted=0")?.query_map(params![record.tenant,record.target],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
                for source in sources {
                    delete_source(conn, &record.tenant, &source)?;
                }
            }
            if (record.action == 3 && !group.deleted) || (record.action == 4 && group.connected) {
                conn.execute("UPDATE knowledge_collections SET connected=0,deleted=?3,revision=revision+1 WHERE tenant_id=?1 AND id=?2",params![record.tenant,record.target,record.action==3])?;
            }
        }
        5 => {
            bump(conn, &record.tenant)?;
            memory_commands::apply_journal_forget(conn, &record.tenant, &record.target)?;
        }
        _ => return Err(StoreError::Corrupt),
    }
    if record.action != 5 {
        bump(conn, &record.tenant)?;
    }
    conn.execute(
        "UPDATE knowledge_journal_meta SET sequence=?1,head=?2 WHERE singleton=1",
        params![record.seq, record.hash.as_slice()],
    )?;
    Ok(())
}
fn destructive(db: &mut Database, tenant: &str, target: &str, action: u8) -> Result<()> {
    db.poisoned = true;
    let record = db.knowledge_journal.append(tenant, target, action)?;
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    apply(&tx, &record)?;
    tx.commit()?;
    db.poisoned = false;
    Ok(())
}
pub(super) fn replay(db: &mut Database) -> Result<()> {
    let (sequence, head): (i64, Vec<u8>) = db.conn.query_row(
        "SELECT sequence,head FROM knowledge_journal_meta WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if sequence < 0 || sequence as usize > db.knowledge_journal.records.len() {
        return Err(StoreError::Corrupt);
    }
    let expected = if sequence == 0 {
        [0; 32]
    } else {
        db.knowledge_journal.records[sequence as usize - 1].hash
    };
    if head != expected {
        return Err(StoreError::Corrupt);
    }
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    for record in db.knowledge_journal.records.iter().skip(sequence as usize) {
        apply(&tx, record)?;
    }
    tx.commit()?;
    Ok(())
}
impl Store {
    pub async fn forget_memory(
        &self,
        user: &UserContext,
        object: String,
        expected_revision: i64,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let object = id(&object)?;
            let (revision, status): (i64, String) = db
                .conn
                .query_row(
                    "SELECT revision,status FROM knowledge_memories WHERE tenant_id=?1 AND id=?2",
                    params![user.tenant.0, object],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(StoreError::NotFound)?;
            if status == "forgotten" && expected_revision.checked_add(1) == Some(revision) {
                return Ok(());
            }
            if revision != expected_revision || status == "forgotten" {
                return Err(StoreError::Conflict);
            }
            destructive(db, &user.tenant.0, &object, 1)
        })
        .await
    }
    pub async fn delete_source(
        &self,
        user: &UserContext,
        object: String,
        expected_revision: i64,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let object = id(&object)?;
            let (revision, deleted): (i64, bool) = db
                .conn
                .query_row(
                    "SELECT revision,deleted FROM knowledge_sources WHERE tenant_id=?1 AND id=?2",
                    params![user.tenant.0, object],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?
                .ok_or(StoreError::NotFound)?;
            if deleted && expected_revision.checked_add(1) == Some(revision) {
                return Ok(());
            }
            if revision != expected_revision || deleted {
                return Err(StoreError::Conflict);
            }
            destructive(db, &user.tenant.0, &object, 2)
        })
        .await
    }
    pub async fn disconnect_collection(
        &self,
        user: &UserContext,
        object: String,
        expected_revision: i64,
        mode: DisconnectMode,
    ) -> Result<KnowledgeCollection> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let group = collection(&db.conn, &user.tenant.0, &object)?;
            let complete = match mode {
                DisconnectMode::Delete => group.deleted,
                DisconnectMode::Keep => !group.connected,
            };
            if complete && expected_revision.checked_add(1) == Some(group.revision) {
                return Ok(group);
            }
            if group.revision != expected_revision || complete {
                return Err(StoreError::Conflict);
            }
            destructive(
                db,
                &user.tenant.0,
                &group.id,
                if mode == DisconnectMode::Keep { 4 } else { 3 },
            )?;
            collection(&db.conn, &user.tenant.0, &group.id)
        })
        .await
    }
}
