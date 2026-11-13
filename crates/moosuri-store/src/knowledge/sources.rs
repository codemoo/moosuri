use super::*;
pub(crate) fn document(
    conn: &Connection,
    tenant: &str,
    object: &str,
    revision: Option<i64>,
) -> Result<SourceDocument> {
    let object = id(object)?;
    let (collection_id,external_id,current,deleted):(String,String,i64,bool)=conn.query_row("SELECT collection_id,external_id,revision,deleted FROM knowledge_sources WHERE tenant_id=?1 AND id=?2",params![tenant,object],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(StoreError::NotFound)?;
    if deleted || collection(conn, tenant, &collection_id)?.deleted {
        return Err(StoreError::NotFound);
    }
    let revision = revision.unwrap_or(current);
    if revision <= 0 {
        return Err(StoreError::InvalidInput);
    }
    let (version,title,text,speaker,observed_at,valid_from,valid_until)=conn.query_row("SELECT version,title,text,speaker,observed_at,valid_from,valid_until FROM knowledge_source_versions WHERE tenant_id=?1 AND source_id=?2 AND revision=?3",params![tenant,object,revision],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,i64>(4)?,r.get::<_,Option<i64>>(5)?,r.get::<_,Option<i64>>(6)?))).optional()?.ok_or(StoreError::NotFound)?;
    Ok(SourceDocument {
        source: SourceSummary {
            id: object.clone(),
            collection_id,
            external_id,
            version,
            revision,
            title,
            speaker,
            observed_at,
            valid_from,
            valid_until,
        },
        text,
        provenance: crate::source_connections::provenance(conn, tenant, &object, revision)?,
    })
}
fn input_valid(value: &SourceInput) -> Result<()> {
    label(&value.external_id, 128)?;
    label(&value.version, 128)?;
    label(&value.title, 200)?;
    speaker(&value.speaker)?;
    validity(value.valid_from, value.valid_until)?;
    if value.text.trim().is_empty()
        || value
            .text
            .chars()
            .any(|c| c.is_control() && !c.is_whitespace())
        || value.text.len() > 64 * 1024
        || value.expected_revision.is_some_and(|v| v <= 0)
    {
        return Err(StoreError::InvalidInput);
    }
    validity(Some(value.observed_at), None)
}
fn content_hash(value: &SourceInput) -> Result<[u8; 32]> {
    hash(&(
        &value.external_id,
        &value.version,
        &value.title,
        &value.text,
        &value.speaker,
        value.observed_at,
        value.valid_from,
        value.valid_until,
    ))
}
fn invalidate(conn: &Connection, tenant: &str, source: &str) -> Result<()> {
    agent_execution::invalidate_source(conn, tenant, source)?;
    let objects = conn
        .prepare(
            "SELECT DISTINCT memory_id FROM knowledge_evidence WHERE tenant_id=?1 AND source_id=?2",
        )?
        .query_map(params![tenant, source], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for object in objects {
        conn.execute("UPDATE knowledge_memory_versions SET status='invalidated' WHERE tenant_id=?1 AND memory_id=?2 AND revision IN (SELECT memory_revision FROM knowledge_evidence WHERE tenant_id=?1 AND memory_id=?2 AND source_id=?3)",params![tenant,object,source])?;
        conn.execute("UPDATE knowledge_memories SET status='invalidated',updated_at=?3 WHERE tenant_id=?1 AND id=?2 AND status='active' AND EXISTS(SELECT 1 FROM knowledge_memory_versions v WHERE v.tenant_id=?1 AND v.memory_id=?2 AND v.revision=knowledge_memories.revision AND v.status='invalidated')",params![tenant,object,now()?])?;
        memories::reindex(conn, tenant, &object)?;
    }
    Ok(())
}

impl Store {
    pub async fn source(
        &self,
        user: &UserContext,
        object: String,
        revision: Option<i64>,
    ) -> Result<SourceDocument> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            document(&db.conn, &user.tenant.0, &object, revision)
        })
        .await
    }
    pub async fn sources(
        &self,
        user: &UserContext,
        collection_id: String,
        after: Option<String>,
    ) -> Result<SourcePage> {
        let user = user.clone();
        self.call(move|db|{
  let user=validate(&db.conn,&user,true,false)?;let group=collection(&db.conn,&user.tenant.0,&collection_id)?;if group.deleted{return Err(StoreError::NotFound);}
  let generation_value=generation(&db.conn,&user.tenant.0)?;let scope=format!("sources:{}",group.id);let after=uncurs(&user.tenant.0,&scope,generation_value,after)?;
  let ids=db.conn.prepare("SELECT id FROM knowledge_sources WHERE tenant_id=?1 AND collection_id=?2 AND deleted=0 AND (?3 IS NULL OR id>?3) ORDER BY id LIMIT 21")?.query_map(params![user.tenant.0,group.id,after],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
  let next_after=if ids.len()>20{Some(cursor(&user.tenant.0,&scope,generation_value,&ids[19])?)}else{None};
  let items=ids.into_iter().take(20).map(|v|document(&db.conn,&user.tenant.0,&v,None).map(|d|d.source)).collect::<Result<_>>()?;Ok(SourcePage{items,next_after,generation:generation_value})
 }).await
    }
    pub async fn ingest_text(
        &self,
        user: &UserContext,
        collection_id: String,
        batch: IngestBatch,
    ) -> Result<IngestReceipt> {
        let user = user.clone();
        self.call(move|db|{
  let user=validate(&db.conn,&user,true,false)?;let group=collection(&db.conn,&user.tenant.0,&collection_id)?;let request=id(&batch.request_id)?;
  if group.source_connection_id.is_some(){return Err(StoreError::Conflict)}
  if batch.complete_snapshot || batch.entries.is_empty() || batch.entries.len()>32 || batch.expected_cursor<0 || batch.next_cursor!=batch.expected_cursor.checked_add(1).ok_or(StoreError::InvalidInput)? {return Err(StoreError::InvalidInput);}
  let mut size=0usize;let mut names=std::collections::BTreeSet::new();for entry in &batch.entries {input_valid(entry)?;size=size.saturating_add(entry.text.len());if !names.insert(&entry.external_id){return Err(StoreError::InvalidInput);}}
  if size>256*1024 || encode(&batch)?.len()>512*1024{return Err(StoreError::InvalidInput);}
  let digest=hash(&(&group.id,&batch))?;
  let previous:Option<(Vec<u8>,String)>=db.conn.query_row("SELECT digest,receipt FROM knowledge_batches WHERE tenant_id=?1 AND request_id=?2",params![user.tenant.0,request],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
  if let Some((old,receipt))=previous {if old!=digest{return Err(StoreError::Conflict);}return decode(&receipt);}
  if !group.connected || group.deleted || group.cursor!=batch.expected_cursor{return Err(StoreError::Conflict);}
  let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
  let source_ids=commit_entries(&tx,&user.tenant.0,&group.id,batch.entries,false)?.0;
  tx.execute("UPDATE knowledge_collections SET cursor=?3,revision=revision+1 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,group.id,batch.next_cursor])?;bump(&tx,&user.tenant.0)?;
  let receipt=IngestReceipt{request_id:request,cursor:batch.next_cursor,source_ids};tx.execute("INSERT INTO knowledge_batches VALUES (?1,?2,?3,?4,?5)",params![user.tenant.0,receipt.request_id,group.id,digest.as_slice(),encode(&receipt)?])?;tx.commit()?;Ok(receipt)
 }).await
    }
}

/// Shared transaction-local entry commit. Caller owns authentication, receipt and
/// cursor fences; all source revisions, invalidations and FTS changes stay atomic.
pub(crate) fn commit_entries(
    conn: &Connection,
    tenant: &str,
    collection_id: &str,
    entries: Vec<SourceInput>,
    automatic_revisions: bool,
) -> Result<(Vec<String>, u32)> {
    let tx = conn;
    let mut source_ids = Vec::new();
    let mut unchanged = 0;
    for mut entry in entries {
        let old:Option<(String,i64,bool)>=tx.query_row("SELECT id,revision,deleted FROM knowledge_sources WHERE tenant_id=?1 AND collection_id=?2 AND external_id=?3",params![tenant,collection_id,entry.external_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        if automatic_revisions && entry.expected_revision.is_none() {
            entry.expected_revision = old.as_ref().map(|v| v.1);
        }
        let digest = content_hash(&entry)?;
        let (object, revision) = if let Some((object, revision, deleted)) = old {
            if deleted || entry.expected_revision != Some(revision) {
                return Err(StoreError::Conflict);
            }
            let (old_hash,version):(Vec<u8>,String)=tx.query_row("SELECT digest,version FROM knowledge_source_versions WHERE tenant_id=?1 AND source_id=?2 AND revision=?3",params![tenant,object,revision],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if old_hash == digest {
                source_ids.push(object);
                unchanged += 1;
                continue;
            }
            if version==entry.version || tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_source_versions WHERE tenant_id=?1 AND source_id=?2 AND version=?3)",params![tenant,object,entry.version],|r|r.get::<_,bool>(0))? {return Err(StoreError::Conflict);}
            let next = revision.checked_add(1).ok_or(StoreError::Corrupt)?;
            if next > 128 {
                return Err(StoreError::Conflict);
            }
            invalidate(tx, tenant, &object)?;
            tx.execute(
                "UPDATE knowledge_sources SET revision=?3 WHERE tenant_id=?1 AND id=?2",
                params![tenant, object, next],
            )?;
            (object, next)
        } else {
            if entry.expected_revision.is_some() {
                return Err(StoreError::Conflict);
            }
            if tx.query_row(
                "SELECT count(*) FROM knowledge_sources WHERE tenant_id=?1",
                [&tenant],
                |r| r.get::<_, i64>(0),
            )? >= 4096
            {
                return Err(StoreError::Conflict);
            }
            let object = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO knowledge_sources VALUES (?1,?2,?3,?4,1,0)",
                params![tenant, object, collection_id, entry.external_id],
            )?;
            (object, 1)
        };
        tx.execute(
            "INSERT INTO knowledge_source_versions VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                tenant,
                object,
                revision,
                entry.version,
                entry.title,
                entry.text,
                entry.speaker,
                entry.observed_at,
                entry.valid_from,
                entry.valid_until,
                digest.as_slice()
            ],
        )?;
        index_text(
            tx,
            tenant,
            "source",
            &object,
            revision,
            &format!("{}\n{}", entry.title, entry.text),
        )?;
        source_ids.push(object);
    }
    Ok((source_ids, unchanged))
}

pub(crate) fn validate_entries(entries: &[SourceInput], allow_empty: bool) -> Result<()> {
    if entries.len() > 32 || (!allow_empty && entries.is_empty()) {
        return Err(StoreError::InvalidInput);
    }
    let mut size = 0usize;
    let mut names = std::collections::BTreeSet::new();
    for entry in entries {
        input_valid(entry)?;
        size = size.saturating_add(entry.text.len());
        if !names.insert(&entry.external_id) {
            return Err(StoreError::InvalidInput);
        }
    }
    if size > 256 * 1024 {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
