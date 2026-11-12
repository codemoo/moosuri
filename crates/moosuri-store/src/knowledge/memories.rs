use super::*;
pub(crate) fn document(
    conn: &Connection,
    tenant: &str,
    object: &str,
    revision: Option<i64>,
) -> Result<MemoryDocument> {
    let object = id(object)?;
    let (current,status,created,updated):(i64,String,i64,i64)=conn.query_row("SELECT revision,status,created_at,updated_at FROM knowledge_memories WHERE tenant_id=?1 AND id=?2",params![tenant,object],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(StoreError::NotFound)?;
    if status == "forgotten" {
        return Err(StoreError::NotFound);
    }
    let revision = revision.unwrap_or(current);
    if revision <= 0 {
        return Err(StoreError::InvalidInput);
    }
    let (draft,status,at):(String,String,i64)=conn.query_row("SELECT draft,status,created_at FROM knowledge_memory_versions WHERE tenant_id=?1 AND memory_id=?2 AND revision=?3",params![tenant,object,revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(StoreError::NotFound)?;
    Ok(MemoryDocument {
        id: object,
        revision,
        status: if status == "active" {
            MemoryStatus::Active
        } else {
            MemoryStatus::Invalidated
        },
        draft: decode(&draft)?,
        created_at: created,
        updated_at: if revision == current { updated } else { at },
    })
}
fn draft_valid(conn: &Connection, tenant: &str, draft: &MemoryDraft) -> Result<()> {
    label(&draft.subject, 200)?;
    speaker(&draft.speaker)?;
    validity(draft.valid_from, draft.valid_until)?;
    if draft.text.trim().is_empty()
        || draft
            .text
            .chars()
            .any(|c| c.is_control() && !c.is_whitespace())
        || draft.text.len() > 4096
        || draft.evidence.len() > 8
        || encode(draft)?.len() > 16 * 1024
    {
        return Err(StoreError::InvalidInput);
    }
    let mut seen = std::collections::BTreeSet::new();
    for evidence in &draft.evidence {
        if evidence.quote.is_empty()
            || evidence.quote.len() > 4096
            || evidence.start_byte >= evidence.end_byte
            || !seen.insert((&evidence.source_id, evidence.start_byte, evidence.end_byte))
        {
            return Err(StoreError::InvalidInput);
        }
        let source = sources::document(conn, tenant, &evidence.source_id, None)?;
        if source.source.revision != evidence.source_revision {
            return Err(StoreError::Conflict);
        }
        if source
            .text
            .get(evidence.start_byte as usize..evidence.end_byte as usize)
            != Some(evidence.quote.as_str())
        {
            return Err(StoreError::InvalidInput);
        }
    }
    Ok(())
}
pub(super) fn reindex(conn: &Connection, tenant: &str, object: &str) -> Result<()> {
    remove_index(conn, tenant, "memory", object)?;
    let rows=conn.prepare("SELECT revision,draft FROM knowledge_memory_versions WHERE tenant_id=?1 AND memory_id=?2 AND status='active' ORDER BY revision")?.query_map(params![tenant,object],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (revision, encoded) in rows {
        let draft: MemoryDraft = decode(&encoded)?;
        conn.execute("INSERT INTO knowledge_fts(tenant_key,kind,object_id,revision,normalized) VALUES (?1,'memory',?2,?3,?4)",params![tenant_key(tenant),object,revision,normalize(&format!("{}\n{}",draft.subject,draft.text))])?;
    }
    Ok(())
}
fn write(
    db: &mut Database,
    user: UserContext,
    object: Option<(String, i64)>,
    request: String,
    draft: MemoryDraft,
) -> Result<MemoryDocument> {
    let user = validate(&db.conn, &user, true, false)?;
    let request = id(&request)?;
    let object = object.map(|(v, r)| id(&v).map(|v| (v, r))).transpose()?;
    let digest = hash(&(&object, &draft))?;
    let receipt:Option<(Vec<u8>,String,i64)>=db.conn.query_row("SELECT digest,memory_id,revision FROM knowledge_writes WHERE tenant_id=?1 AND request_id=?2",params![user.tenant.0,request],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    if let Some((old, memory, revision)) = receipt {
        if old != digest {
            return Err(StoreError::Conflict);
        }
        return document(&db.conn, &user.tenant.0, &memory, Some(revision));
    }
    draft_valid(&db.conn, &user.tenant.0, &draft)?;
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let time = now()?;
    let (object, revision) = if let Some((object, expected)) = object {
        let previous = document(&tx, &user.tenant.0, &object, None)?;
        if expected != previous.revision {
            return Err(StoreError::Conflict);
        }
        agent_execution::invalidate_object(&tx, &user.tenant.0, "memory", &object, None)?;
        let revision = expected.checked_add(1).ok_or(StoreError::Corrupt)?;
        if revision > 128 {
            return Err(StoreError::Conflict);
        }
        tx.execute("UPDATE knowledge_memories SET revision=?3,status='active',updated_at=?4 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,object,revision,time])?;
        (object, revision)
    } else {
        if tx.query_row(
            "SELECT count(*) FROM knowledge_memories WHERE tenant_id=?1",
            [&user.tenant.0],
            |r| r.get::<_, i64>(0),
        )? >= 4096
        {
            return Err(StoreError::Conflict);
        }
        let object = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO knowledge_memories VALUES (?1,?2,1,'active',?3,?3)",
            params![user.tenant.0, object, time],
        )?;
        (object, 1)
    };
    tx.execute(
        "INSERT INTO knowledge_memory_versions VALUES (?1,?2,?3,?4,'active',?5)",
        params![user.tenant.0, object, revision, encode(&draft)?, time],
    )?;
    tx.execute(
        "INSERT INTO knowledge_memory_timeline VALUES (?1,?2,?3,?4)",
        params![user.tenant.0, object, revision, draft.valid_from],
    )?;
    for evidence in &draft.evidence {
        let quote_hash: [u8; 32] = Sha256::digest(evidence.quote.as_bytes()).into();
        tx.execute(
            "INSERT INTO knowledge_evidence VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                user.tenant.0,
                object,
                revision,
                id(&evidence.source_id)?,
                evidence.source_revision,
                evidence.start_byte,
                evidence.end_byte,
                quote_hash.as_slice()
            ],
        )?;
    }
    reindex(&tx, &user.tenant.0, &object)?;
    bump(&tx, &user.tenant.0)?;
    tx.execute(
        "INSERT INTO knowledge_writes VALUES (?1,?2,?3,?4,?5)",
        params![user.tenant.0, request, digest.as_slice(), object, revision],
    )?;
    let result = document(&tx, &user.tenant.0, &object, Some(revision))?;
    tx.commit()?;
    Ok(result)
}
impl Store {
    pub async fn create_memory(
        &self,
        user: &UserContext,
        request_id: String,
        draft: MemoryDraft,
    ) -> Result<MemoryDocument> {
        let user = user.clone();
        self.call(move |db| write(db, user, None, request_id, draft))
            .await
    }
    pub async fn correct_memory(
        &self,
        user: &UserContext,
        object: String,
        expected_revision: i64,
        request_id: String,
        draft: MemoryDraft,
    ) -> Result<MemoryDocument> {
        let user = user.clone();
        self.call(move |db| {
            write(
                db,
                user,
                Some((object, expected_revision)),
                request_id,
                draft,
            )
        })
        .await
    }
    pub async fn memory(
        &self,
        user: &UserContext,
        object: String,
        revision: Option<i64>,
    ) -> Result<MemoryDocument> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            document(&db.conn, &user.tenant.0, &object, revision)
        })
        .await
    }
    pub async fn memories(&self, user: &UserContext, after: Option<String>) -> Result<MemoryPage> {
        let user = user.clone();
        self.call(move|db|{
  let user=validate(&db.conn,&user,true,false)?;let generation_value=generation(&db.conn,&user.tenant.0)?;let after=uncurs(&user.tenant.0,"memories",generation_value,after)?;
  let ids=db.conn.prepare("SELECT id FROM knowledge_memories WHERE tenant_id=?1 AND status!='forgotten' AND (?2 IS NULL OR id>?2) ORDER BY id LIMIT 21")?.query_map(params![user.tenant.0,after],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
  let next_after=if ids.len()>20{Some(cursor(&user.tenant.0,"memories",generation_value,&ids[19])?)}else{None};let items=ids.into_iter().take(20).map(|v|document(&db.conn,&user.tenant.0,&v,None)).collect::<Result<_>>()?;Ok(MemoryPage{items,next_after,generation:generation_value})
 }).await
    }
    pub async fn memory_history(
        &self,
        user: &UserContext,
        object: String,
        before: Option<i64>,
    ) -> Result<MemoryHistory> {
        let user = user.clone();
        self.call(move|db|{
  let user=validate(&db.conn,&user,true,false)?;let current=document(&db.conn,&user.tenant.0,&object,None)?;if before.is_some_and(|v|v<=0){return Err(StoreError::InvalidInput);}
  let revisions=db.conn.prepare("SELECT revision FROM knowledge_memory_versions WHERE tenant_id=?1 AND memory_id=?2 AND (?3 IS NULL OR revision<?3) ORDER BY revision DESC LIMIT 21")?.query_map(params![user.tenant.0,current.id,before],|r|r.get::<_,i64>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
  let next_before=if revisions.len()>20{Some(revisions[19])}else{None};let items=revisions.into_iter().take(20).map(|v|document(&db.conn,&user.tenant.0,&current.id,Some(v))).collect::<Result<_>>()?;Ok(MemoryHistory{items,next_before})
 }).await
    }
}
