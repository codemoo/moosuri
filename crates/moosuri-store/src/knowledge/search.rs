use super::*;
pub(super) fn search(
    conn: &Connection,
    user: UserContext,
    query: KnowledgeQuery,
) -> Result<KnowledgeSearch> {
    search_limit(conn, user, query, 20)
}
pub(crate) fn search_limit(
    conn: &Connection,
    user: UserContext,
    mut query: KnowledgeQuery,
    limit: usize,
) -> Result<KnowledgeSearch> {
    let user = validate(conn, &user, true, false)?;
    label(&query.text, 128)?;
    speaker(&query.speaker)?;
    if let Some(subject) = &query.subject {
        label(subject, 200)?;
    }
    if let Some(collection) = &mut query.collection_id {
        *collection = id(collection)?;
        let group = super::collection(conn, &user.tenant.0, collection)?;
        if group.deleted {
            return Err(StoreError::NotFound);
        }
    }
    if (query.kind == KnowledgeKind::Source && query.subject.is_some())
        || (query.kind == KnowledgeKind::Memory && query.collection_id.is_some())
    {
        return Err(StoreError::InvalidInput);
    }
    validity(query.valid_at, None)?;
    let incoming = query.cursor.take();
    let digest = hash(&query)?;
    let scope = format!(
        "search:{}",
        digest
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let generation_value = generation(conn, &user.tenant.0)?;
    let (offset, time) = match uncurs(&user.tenant.0, &scope, generation_value, incoming)? {
        Some(position) => {
            serde_json::from_str::<(i64, i64)>(&position).map_err(|_| StoreError::InvalidInput)?
        }
        None => (0, query.valid_at.unwrap_or(now()?)),
    };
    if !(0..=4096).contains(&offset) {
        return Err(StoreError::InvalidInput);
    }
    validity(Some(time), None)?;
    let normalized = normalize(&query.text);
    if normalized.is_empty() {
        return Err(StoreError::InvalidInput);
    }
    let kind = if query.kind == KnowledgeKind::Source {
        "source"
    } else {
        "memory"
    };
    // Choose the newest begun memory revision first, then apply expiry.
    // A newer expired or purged revision is not permission to revive an older assertion.
    let base = if query.kind == KnowledgeKind::Source {
        "SELECT s.id,s.revision,v.title,v.text,NULL AS subject,v.speaker,v.valid_from,v.valid_until FROM knowledge_sources s JOIN knowledge_source_versions v ON v.tenant_id=s.tenant_id AND v.source_id=s.id AND v.revision=s.revision JOIN knowledge_collections c ON c.tenant_id=s.tenant_id AND c.id=s.collection_id WHERE s.tenant_id=?1 AND s.deleted=0 AND c.deleted=0 AND (?2 IS NULL OR s.collection_id=?2) AND ?3 IS NULL AND (?4 IS NULL OR v.speaker=?4) AND (v.valid_from IS NULL OR v.valid_from<=?5) AND (v.valid_until IS NULL OR v.valid_until>?5)"
    } else {
        "SELECT m.id,v.revision,json_extract(v.draft,'$.subject') AS title,json_extract(v.draft,'$.text') AS text,json_extract(v.draft,'$.subject') AS subject,json_extract(v.draft,'$.speaker') AS speaker,json_extract(v.draft,'$.valid_from') AS valid_from,json_extract(v.draft,'$.valid_until') AS valid_until FROM knowledge_memories m JOIN knowledge_memory_versions v ON v.tenant_id=m.tenant_id AND v.memory_id=m.id AND v.revision=(SELECT max(v2.revision) FROM knowledge_memory_timeline v2 WHERE v2.tenant_id=m.tenant_id AND v2.memory_id=m.id AND (v2.valid_from IS NULL OR v2.valid_from<=?5)) WHERE m.tenant_id=?1 AND m.status!='forgotten' AND v.status='active' AND ?2 IS NULL AND (?3 IS NULL OR json_extract(v.draft,'$.subject')=?3) AND (?4 IS NULL OR json_extract(v.draft,'$.speaker')=?4) AND (json_extract(v.draft,'$.valid_from') IS NULL OR json_extract(v.draft,'$.valid_from')<=?5) AND (json_extract(v.draft,'$.valid_until') IS NULL OR json_extract(v.draft,'$.valid_until')>?5)"
    };
    let key = tenant_key(&user.tenant.0);
    let fts = normalized.chars().count() >= 3;
    let matching = format!(
        "tenant_key : \"{}\" AND normalized : \"{}\"",
        key,
        normalized.replace('"', "\"\"")
    );
    let (sql, work_truncated) = if fts {
        (
            format!(
                "SELECT b.id,b.revision,b.title,substr(b.text,1,512),b.subject,b.speaker,b.valid_from,b.valid_until FROM ({base}) b WHERE (b.id,b.revision) IN (SELECT object_id,cast(revision AS INTEGER) FROM knowledge_fts WHERE knowledge_fts MATCH ?7 AND tenant_key=?8 AND kind=?9) ORDER BY b.id LIMIT ?11 OFFSET ?10"
            ),
            false,
        )
    } else {
        // Materialize only IDs, never a thousand full documents. Short substring
        // searches inspect at most 1,000 eligible records and disclose that boundary.
        let count: i64 = conn.query_row(
            &format!("SELECT count(*) FROM (SELECT id FROM ({base}) ORDER BY id LIMIT 1001)"),
            params![
                user.tenant.0,
                query.collection_id,
                query.subject,
                query.speaker,
                time
            ],
            |r| r.get(0),
        )?;
        (
            format!(
                "WITH candidates AS MATERIALIZED (SELECT id FROM ({base}) ORDER BY id LIMIT 1000) SELECT b.id,b.revision,b.title,substr(b.text,1,512),b.subject,b.speaker,b.valid_from,b.valid_until FROM ({base}) b WHERE b.id IN (SELECT id FROM candidates) AND instr((SELECT normalized FROM knowledge_fts WHERE tenant_key=?8 AND kind=?9 AND object_id=b.id AND revision=b.revision LIMIT 1),?6)>0 ORDER BY b.id LIMIT ?11 OFFSET ?10"
            ),
            count > 1000,
        )
    };
    let mut statement = conn.prepare(&sql)?;
    let mut hits = statement
        .query_map(
            params![
                user.tenant.0,
                query.collection_id,
                query.subject,
                query.speaker,
                time,
                normalized,
                matching,
                key,
                kind,
                offset,
                limit as i64 + 1
            ],
            |r| {
                Ok(KnowledgeHit {
                    kind: query.kind,
                    id: r.get(0)?,
                    revision: r.get(1)?,
                    title: r.get(2)?,
                    snippet: r.get(3)?,
                    subject: r.get(4)?,
                    speaker: r.get(5)?,
                    valid_from: r.get(6)?,
                    valid_until: r.get(7)?,
                })
            },
        )?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let more = hits.len() > limit;
    hits.truncate(limit);
    let next_cursor = if more {
        Some(cursor(
            &user.tenant.0,
            &scope,
            generation_value,
            &encode(&(offset + limit as i64, time))?,
        )?)
    } else {
        None
    };
    Ok(KnowledgeSearch {
        hits,
        next_cursor,
        truncated: work_truncated,
        generation: generation_value,
    })
}
impl Store {
    pub async fn search_knowledge(
        &self,
        user: &UserContext,
        query: KnowledgeQuery,
    ) -> Result<KnowledgeSearch> {
        let user = user.clone();
        self.call(move |db| search(&db.conn, user, query)).await
    }
}
