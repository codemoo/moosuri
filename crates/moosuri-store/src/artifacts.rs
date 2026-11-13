//! Versioned local records, common UI/tool mutations and transitive withdrawal.
use super::*;
use moosuri_core::*;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::BTreeSet;
pub(crate) mod agent;
mod exports;
#[cfg(test)]
mod tests;
fn encode<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn decode<T: DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
fn id(v: &str) -> Result<()> {
    Uuid::parse_str(v)
        .map(|_| ())
        .map_err(|_| StoreError::InvalidInput)
}
fn kind(v: ArtifactKind) -> &'static str {
    match v {
        ArtifactKind::Document => "document",
        ArtifactKind::Goal => "goal",
        ArtifactKind::Followup => "followup",
    }
}
fn depkind(v: ArtifactDependencyKind) -> &'static str {
    match v {
        ArtifactDependencyKind::Source => "source",
        ArtifactDependencyKind::Memory => "memory",
        ArtifactDependencyKind::Artifact => "artifact",
    }
}
fn status(v: &str) -> Result<ArtifactStatus> {
    match v {
        "active" => Ok(ArtifactStatus::Active),
        "withdrawn" => Ok(ArtifactStatus::Withdrawn),
        "deleted" => Ok(ArtifactStatus::Deleted),
        _ => Err(StoreError::Corrupt),
    }
}
fn label(v: &str, max: usize) -> Result<()> {
    if v.trim().is_empty() || v.len() > max || v.chars().any(char::is_control) {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn validate_draft(v: &ArtifactDraft) -> Result<()> {
    label(&v.title, 200)?;
    if v.body.len() > 32768
        || v.body.chars().any(|c| c.is_control() && !c.is_whitespace())
        || encode(&v.payload)?.len() > 8192
        || v.references.len() > 12
    {
        return Err(StoreError::InvalidInput);
    }
    match &v.payload {
        ArtifactPayload::Document => {}
        ArtifactPayload::Goal {
            outcome,
            success_criteria,
            progress,
            ..
        } => {
            label(outcome, 2048)?;
            if success_criteria.trim().is_empty()
                || success_criteria.len() > 4096
                || progress.len() > 4096
            {
                return Err(StoreError::InvalidInput);
            }
        }
        ArtifactPayload::Followup {
            person,
            due,
            review,
            snooze,
            ..
        } => {
            label(person, 200)?;
            for d in [due, review, snooze].into_iter().flatten() {
                if !(0..253_402_300_800).contains(&d.utc) || d.timezone != "UTC" {
                    return Err(StoreError::InvalidInput);
                }
            }
        }
    }
    Ok(())
}
pub(crate) fn version(
    conn: &Connection,
    tenant: &str,
    object: &str,
    revision: Option<i64>,
) -> Result<ArtifactVersion> {
    id(object)?;
    let (k,head,s,updated):(String,i64,String,i64)=conn.query_row("SELECT kind,revision,status,updated_at FROM personal_artifacts WHERE tenant_id=?1 AND id=?2",params![tenant,object],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(StoreError::NotFound)?;
    let revision = revision.unwrap_or(head);
    if revision <= 0 {
        return Err(StoreError::InvalidInput);
    }
    let (vs,draft,at):(String,Option<String>,i64)=conn.query_row("SELECT status,draft,created_at FROM personal_artifact_versions WHERE tenant_id=?1 AND artifact_id=?2 AND revision=?3",params![tenant,object,revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(StoreError::NotFound)?;
    let state = if s == "deleted" {
        ArtifactStatus::Deleted
    } else {
        status(&vs)?
    };
    let draft: Option<ArtifactDraft> = if state == ArtifactStatus::Active {
        draft.as_deref().map(decode).transpose()?
    } else {
        None
    };
    Ok(ArtifactVersion {
        artifact: ArtifactSummary {
            id: object.into(),
            kind: match k.as_str() {
                "document" => ArtifactKind::Document,
                "goal" => ArtifactKind::Goal,
                "followup" => ArtifactKind::Followup,
                _ => return Err(StoreError::Corrupt),
            },
            revision,
            status: state,
            title: draft.as_ref().map(|v| v.title.clone()).unwrap_or_default(),
            updated_at: if revision == head { updated } else { at },
        },
        draft,
    })
}
fn dependencies(
    conn: &Connection,
    tenant: &str,
    object: &str,
    revision: i64,
) -> Result<Vec<ArtifactDependency>> {
    let rows=conn.prepare("SELECT kind,object_id,revision FROM personal_artifact_dependencies WHERE tenant_id=?1 AND artifact_id=?2 AND artifact_revision=?3")?.query_map(params![tenant,object,revision],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    rows.into_iter()
        .map(|(k, id, revision)| {
            Ok(ArtifactDependency {
                kind: match k.as_str() {
                    "source" => ArtifactDependencyKind::Source,
                    "memory" => ArtifactDependencyKind::Memory,
                    "artifact" => ArtifactDependencyKind::Artifact,
                    _ => return Err(StoreError::Corrupt),
                },
                id,
                revision,
            })
        })
        .collect()
}
fn closure(
    conn: &Connection,
    tenant: &str,
    refs: Vec<ArtifactDependency>,
) -> Result<BTreeSet<ArtifactDependency>> {
    let mut pending = refs;
    let mut found = BTreeSet::new();
    while let Some(dep) = pending.pop() {
        id(&dep.id)?;
        if dep.revision <= 0 {
            return Err(StoreError::InvalidInput);
        }
        if !found.insert(dep.clone()) {
            continue;
        }
        if found.len() > 256 {
            return Err(StoreError::InvalidInput);
        }
        match dep.kind {
            ArtifactDependencyKind::Source => {
                let d = knowledge::sources::document(conn, tenant, &dep.id, None)?;
                if d.source.revision != dep.revision {
                    return Err(StoreError::Conflict);
                }
            }
            ArtifactDependencyKind::Memory => {
                let d = knowledge::memories::document(conn, tenant, &dep.id, None)?;
                if d.revision != dep.revision || d.status != MemoryStatus::Active {
                    return Err(StoreError::Conflict);
                }
                pending.extend(d.draft.evidence.into_iter().map(|e| ArtifactDependency {
                    kind: ArtifactDependencyKind::Source,
                    id: e.source_id,
                    revision: e.source_revision,
                }));
            }
            ArtifactDependencyKind::Artifact => {
                let d = version(conn, tenant, &dep.id, None)?;
                if d.artifact.revision != dep.revision || d.draft.is_none() {
                    return Err(StoreError::Conflict);
                }
                pending.extend(dependencies(conn, tenant, &dep.id, dep.revision)?);
            }
        }
    }
    Ok(found)
}
fn digest<T: Serialize>(v: &T) -> Result<[u8; 32]> {
    Ok(Sha256::digest(encode(v)?.as_bytes()).into())
}
fn receipt(
    conn: &Connection,
    tenant: &str,
    request: &str,
    expected: &[u8],
) -> Result<Option<ArtifactReceipt>> {
    let row:Option<(Vec<u8>,String,i64,String)>=conn.query_row("SELECT digest,artifact_id,revision,status FROM personal_artifact_mutations WHERE tenant_id=?1 AND request_id=?2",params![tenant,request],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    row.map(|(old, id, revision, s)| {
        if old != expected {
            return Err(StoreError::Conflict);
        }
        Ok(ArtifactReceipt {
            request_id: request.into(),
            url: format!("/app/artifacts/{id}"),
            id,
            revision,
            status: status(&s)?,
        })
    })
    .transpose()
}
fn save_receipt(
    conn: &Connection,
    tenant: &str,
    request: &str,
    hash: &[u8],
    object: &str,
    revision: i64,
    s: &str,
) -> Result<ArtifactReceipt> {
    conn.execute(
        "INSERT INTO personal_artifact_mutations VALUES(?1,?2,?3,?4,?5,?6)",
        params![tenant, request, hash, object, revision, s],
    )?;
    receipt(conn, tenant, request, hash)?.ok_or(StoreError::Corrupt)
}
/// Already-authenticated caller owns the transaction. Extra dependencies come only
/// from Store's delivered-context registry, never the model's citation subset.
pub(crate) fn write(
    conn: &Connection,
    tenant: &str,
    input: ArtifactWrite,
    extra: Vec<ArtifactDependency>,
    origin_run: Option<&str>,
) -> Result<ArtifactReceipt> {
    // Tool argument/conflict errors are returned as results inside a wider step
    // transaction. A failed mutation must not commit a partial purge or version.
    conn.execute_batch("SAVEPOINT artifact_mutation")?;
    let result = write_body(conn, tenant, input, extra, origin_run);
    if result.is_err() {
        conn.execute_batch("ROLLBACK TO artifact_mutation; RELEASE artifact_mutation")?;
    } else {
        conn.execute_batch("RELEASE artifact_mutation")?;
    }
    result
}
fn write_body(
    conn: &Connection,
    tenant: &str,
    mut input: ArtifactWrite,
    extra: Vec<ArtifactDependency>,
    origin_run: Option<&str>,
) -> Result<ArtifactReceipt> {
    id(&input.request_id)?;
    let hash = digest(&input)?;
    if let Some(r) = receipt(conn, tenant, &input.request_id, &hash)? {
        return Ok(r);
    }
    validate_draft(&input.draft)?;
    let mut refs = input.draft.references.clone();
    refs.extend(extra);
    let object = if let Some(object) = &input.id {
        id(object)?;
        let old = version(conn, tenant, object, None)?;
        if old.draft.is_none()
            || input.expected_revision != Some(old.artifact.revision)
            || old.artifact.kind != input.draft.payload.kind()
        {
            return Err(StoreError::Conflict);
        }
        refs.extend(dependencies(conn, tenant, object, old.artifact.revision)?);
        Some(old)
    } else {
        if input.expected_revision.is_some() {
            return Err(StoreError::InvalidInput);
        }
        None
    };
    if refs
        .iter()
        .any(|d| d.kind == ArtifactDependencyKind::Artifact && Some(&d.id) == input.id.as_ref())
    {
        return Err(StoreError::InvalidInput);
    }
    let refs = closure(conn, tenant, refs)?;
    if refs
        .iter()
        .any(|d| d.kind == ArtifactDependencyKind::Artifact && Some(&d.id) == input.id.as_ref())
    {
        return Err(StoreError::InvalidInput);
    }
    // Show a bounded projection of the actual saved provenance. The full closure
    // stays in the dependency table and is never narrowed by citation omission.
    input.draft.references = refs.iter().take(12).cloned().collect();
    let time = now()?;
    let (object, revision) = if let Some(old) = object {
        let revision = old.artifact.revision + 1;
        if revision > 128 {
            return Err(StoreError::Conflict);
        }
        invalidate_inner(
            conn,
            tenant,
            "artifact",
            &old.artifact.id,
            Some(old.artifact.revision),
            origin_run,
        )?;
        conn.execute(
            "UPDATE personal_artifacts SET revision=?3,updated_at=?4 WHERE tenant_id=?1 AND id=?2",
            params![tenant, old.artifact.id, revision, time],
        )?;
        (old.artifact.id, revision)
    } else {
        if conn.query_row(
            "SELECT count(*) FROM personal_artifacts WHERE tenant_id=?1",
            [tenant],
            |r| r.get::<_, i64>(0),
        )? >= 256
        {
            return Err(StoreError::Conflict);
        }
        let object = Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO personal_artifacts VALUES(?1,?2,?3,1,'active',?4)",
            params![tenant, object, kind(input.draft.payload.kind()), time],
        )?;
        (object, 1)
    };
    conn.execute(
        "INSERT INTO personal_artifact_versions VALUES(?1,?2,?3,'active',?4,?5)",
        params![tenant, object, revision, encode(&input.draft)?, time],
    )?;
    for d in refs {
        conn.execute(
            "INSERT INTO personal_artifact_dependencies VALUES(?1,?2,?3,?4,?5,?6)",
            params![tenant, object, revision, depkind(d.kind), d.id, d.revision],
        )?;
    }
    knowledge::bump(conn, tenant)?;
    save_receipt(
        conn,
        tenant,
        &input.request_id,
        &hash,
        &object,
        revision,
        "active",
    )
}
/// Normal/replay source and memory withdrawal call this same content-free body.
pub(crate) fn invalidate_dependency(
    conn: &Connection,
    tenant: &str,
    k: &str,
    object: &str,
    revision: Option<i64>,
) -> Result<()> {
    invalidate_inner(conn, tenant, k, object, revision, None)
}
fn invalidate_inner(
    conn: &Connection,
    tenant: &str,
    k: &str,
    object: &str,
    revision: Option<i64>,
    except: Option<&str>,
) -> Result<()> {
    let mut pending = vec![(k.to_owned(), object.to_owned(), revision)];
    let mut seen = BTreeSet::new();
    while let Some((k, object, revision)) = pending.pop() {
        if !seen.insert((k.clone(), object.clone(), revision)) {
            continue;
        }
        if seen.len() > 32769 {
            return Err(StoreError::Corrupt);
        }
        if k == "artifact" {
            let runs=conn.prepare("SELECT DISTINCT run_id FROM personal_artifact_run_dependencies WHERE tenant_id=?1 AND artifact_id=?2 AND (?3 IS NULL OR revision=?3)")?.query_map(params![tenant,object,revision],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            agent_execution::purge_runs(
                conn,
                tenant,
                runs.into_iter()
                    .filter(|r| Some(r.as_str()) != except)
                    .collect(),
            )?;
        }
        let dependents=conn.prepare("SELECT DISTINCT d.artifact_id,d.artifact_revision FROM personal_artifact_dependencies d JOIN personal_artifact_versions v ON v.tenant_id=d.tenant_id AND v.artifact_id=d.artifact_id AND v.revision=d.artifact_revision WHERE d.tenant_id=?1 AND d.kind=?2 AND d.object_id=?3 AND (?4 IS NULL OR d.revision=?4) AND v.status='active'")?.query_map(params![tenant,k,object,revision],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        for (object, revision) in dependents {
            conn.execute("UPDATE personal_artifact_versions SET status='withdrawn',draft=NULL WHERE tenant_id=?1 AND artifact_id=?2 AND revision=?3",params![tenant,object,revision])?;
            conn.execute("UPDATE personal_artifacts SET status='withdrawn',updated_at=?4 WHERE tenant_id=?1 AND id=?2 AND revision=?3",params![tenant,object,revision,now()?])?;
            pending.push(("artifact".into(), object, Some(revision)));
        }
    }
    Ok(())
}
pub(crate) fn apply_delete(
    conn: &Connection,
    tenant: &str,
    object: &str,
    input: &ArtifactDelete,
) -> Result<()> {
    let hash = digest(&("delete", object, input))?;
    if receipt(conn, tenant, &input.request_id, &hash)?.is_some() {
        return Ok(());
    }
    let current = version(conn, tenant, object, None)?;
    if current.artifact.revision != input.expected_revision
        || current.artifact.status == ArtifactStatus::Deleted
    {
        return Err(StoreError::Corrupt);
    }
    invalidate_dependency(conn, tenant, "artifact", object, None)?;
    conn.execute("UPDATE personal_artifact_versions SET status='deleted',draft=NULL WHERE tenant_id=?1 AND artifact_id=?2",params![tenant,object])?;
    let revision = current.artifact.revision + 1;
    conn.execute("UPDATE personal_artifacts SET revision=?3,status='deleted',updated_at=?4 WHERE tenant_id=?1 AND id=?2",params![tenant,object,revision,now()?])?;
    conn.execute(
        "INSERT INTO personal_artifact_versions VALUES(?1,?2,?3,'deleted',NULL,?4)",
        params![tenant, object, revision, now()?],
    )?;
    knowledge::bump(conn, tenant)?;
    save_receipt(
        conn,
        tenant,
        &input.request_id,
        &hash,
        object,
        revision,
        "deleted",
    )?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    tenant: String,
    generation: i64,
    at: i64,
    kind: Option<ArtifactKind>,
    briefing: bool,
    last: String,
}
pub(crate) fn list(
    conn: &Connection,
    tenant: &str,
    k: Option<ArtifactKind>,
    after: Option<String>,
    limit: usize,
    briefing: bool,
) -> Result<ArtifactPage> {
    if !(1..=20).contains(&limit) {
        return Err(StoreError::InvalidInput);
    }
    let generation = knowledge::generation(conn, tenant)?;
    let wall = now()?;
    let after = after
        .map(|s| {
            if s.len() > 512 {
                return Err(StoreError::InvalidInput);
            }
            let c: Cursor = serde_json::from_str(&s).map_err(|_| StoreError::InvalidInput)?;
            id(&c.last)?;
            if c.tenant != tenant
                || c.generation != generation
                || c.kind != k
                || c.briefing != briefing
                || c.at > wall
                || wall.saturating_sub(c.at) > 300
            {
                return Err(StoreError::Conflict);
            }
            Ok(c)
        })
        .transpose()?;
    let clock = after.as_ref().map(|c| c.at).unwrap_or(wall);
    let ids=conn.prepare("SELECT id FROM personal_artifacts WHERE tenant_id=?1 AND status='active' AND (?2 IS NULL OR kind=?2) ORDER BY id")?.query_map(params![tenant,k.map(kind)],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let mut items = Vec::new();
    for object in ids {
        let v = version(conn, tenant, &object, None)?;
        if briefing {
            let Some(ArtifactDraft {
                payload:
                    ArtifactPayload::Followup {
                        lifecycle,
                        muted,
                        snooze,
                        ..
                    },
                ..
            }) = &v.draft
            else {
                continue;
            };
            if matches!(
                lifecycle,
                FollowupLifecycle::Resolved | FollowupLifecycle::Cancelled
            ) || *muted
                || snooze.as_ref().is_some_and(|d| d.utc > clock)
            {
                continue;
            }
        }
        items.push(v);
    }
    if briefing {
        items.sort_by_key(|v| {
            v.draft.as_ref().and_then(|d| {
                if let ArtifactPayload::Followup { review, due, .. } = &d.payload {
                    Some((
                        review
                            .as_ref()
                            .map(|d| d.utc)
                            .or_else(|| due.as_ref().map(|d| d.utc))
                            .unwrap_or(i64::MAX),
                        -v.artifact.updated_at,
                        v.artifact.id.clone(),
                    ))
                } else {
                    None
                }
            })
        });
    }
    if let Some(after) = after {
        let pos = items
            .iter()
            .position(|v| v.artifact.id == after.last)
            .ok_or(StoreError::Conflict)?;
        items.drain(..=pos);
    }
    let complete = items.len() <= limit;
    let next_after = if complete {
        None
    } else {
        Some(encode(&Cursor {
            tenant: tenant.into(),
            generation,
            at: clock,
            kind: k,
            briefing,
            last: items[limit - 1].artifact.id.clone(),
        })?)
    };
    Ok(ArtifactPage {
        items: items.into_iter().take(limit).map(|v| v.artifact).collect(),
        next_after,
        coverage_complete: complete,
    })
}
impl Store {
    pub async fn write_artifact(
        &self,
        user: &UserContext,
        input: ArtifactWrite,
    ) -> Result<ArtifactReceipt> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r = write(&tx, &user.tenant.0, input, vec![], None)?;
            tx.commit()?;
            Ok(r)
        })
        .await
    }
    pub async fn artifact(
        &self,
        user: &UserContext,
        object: String,
        revision: Option<i64>,
    ) -> Result<ArtifactVersion> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            version(&db.conn, &user.tenant.0, &object, revision)
        })
        .await
    }
    pub async fn artifacts(
        &self,
        user: &UserContext,
        k: Option<ArtifactKind>,
        after: Option<String>,
    ) -> Result<ArtifactPage> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            list(&db.conn, &user.tenant.0, k, after, 20, false)
        })
        .await
    }
    pub async fn followup_briefing(
        &self,
        user: &UserContext,
        after: Option<String>,
    ) -> Result<ArtifactPage> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            list(
                &db.conn,
                &user.tenant.0,
                Some(ArtifactKind::Followup),
                after,
                5,
                true,
            )
        })
        .await
    }
    pub async fn artifact_history(
        &self,
        user: &UserContext,
        object: String,
        before: Option<i64>,
    ) -> Result<ArtifactHistory> {
        let user = user.clone();
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;version(&db.conn,&user.tenant.0,&object,None)?;if before.is_some_and(|v|v<=0){return Err(StoreError::InvalidInput)}let ids=db.conn.prepare("SELECT revision FROM personal_artifact_versions WHERE tenant_id=?1 AND artifact_id=?2 AND (?3 IS NULL OR revision<?3) ORDER BY revision DESC LIMIT 21")?.query_map(params![user.tenant.0,object,before],|r|r.get::<_,i64>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;let next_before=if ids.len()>20{Some(ids[19])}else{None};let items=ids.into_iter().take(20).map(|r|version(&db.conn,&user.tenant.0,&object,Some(r))).collect::<Result<_>>()?;Ok(ArtifactHistory{items,next_before})}).await
    }
    pub async fn delete_artifact(
        &self,
        user: &UserContext,
        object: String,
        input: ArtifactDelete,
    ) -> Result<ArtifactReceipt> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            id(&object)?;
            id(&input.request_id)?;
            let hash = digest(&("delete", &object, &input))?;
            if let Some(r) = receipt(&db.conn, &user.tenant.0, &input.request_id, &hash)? {
                return Ok(r);
            }
            let current = version(&db.conn, &user.tenant.0, &object, None)?;
            if current.artifact.revision != input.expected_revision
                || current.artifact.status == ArtifactStatus::Deleted
            {
                return Err(StoreError::Conflict);
            }
            destructive_intents::commit(
                db,
                &user.tenant.0,
                destructive_intents::Effect::DeleteArtifact {
                    target: object,
                    request: input.request_id.clone(),
                    revision: input.expected_revision,
                },
            )?;
            receipt(&db.conn, &user.tenant.0, &input.request_id, &hash)?.ok_or(StoreError::Corrupt)
        })
        .await
    }
    pub async fn export_artifact(
        &self,
        user: &UserContext,
        object: String,
        revision: i64,
        html: bool,
    ) -> Result<String> {
        let v = self.artifact(user, object, Some(revision)).await?;
        let d = v.draft.ok_or(StoreError::NotFound)?;
        if html {
            Ok(exports::html(&d))
        } else {
            Ok(exports::markdown(&d))
        }
    }
}
