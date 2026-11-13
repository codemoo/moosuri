use super::*;
use crate::chat::Run;
pub(crate) fn profile(
    conn: &Connection,
    tenant: &str,
    run: &str,
) -> Result<Option<ArtifactRunProfile>> {
    let row:Option<(i64,Option<String>,Option<i64>,bool)>=conn.query_row("SELECT schema_version,selected_id,selected_revision,planned FROM personal_artifact_run_profiles WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    row.map(|(schema,id,revision,planned)|{if schema!=1{return Err(StoreError::Conflict)}let selected=id.map(|id|revision.map(|revision|ArtifactTarget{id,revision}).ok_or(StoreError::Corrupt)).transpose()?;let write=conn.query_row("SELECT EXISTS(SELECT 1 FROM personal_artifact_command_plans WHERE tenant_id=?1 AND run_id=?2)",params![tenant,run],|r|r.get(0))?;let kind:Option<String>=conn.query_row("SELECT kind FROM personal_artifact_command_plans WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|r.get(0)).optional()?;let kind=kind.map(|k|match k.as_str(){"document"=>Ok(ArtifactKind::Document),"goal"=>Ok(ArtifactKind::Goal),"followup"=>Ok(ArtifactKind::Followup),_=>Err(StoreError::Corrupt)}).transpose()?;Ok(ArtifactRunProfile{schema_version:1,selected,planned,write,kind})}).transpose()
}
fn run_refs(conn: &Connection, run: &Run, target: Option<&str>) -> Result<Vec<ArtifactDependency>> {
    let mut refs=conn.prepare("SELECT kind,object_id,revision FROM agent_dependencies WHERE tenant_id=?1 AND run_id=?2")?.query_map(params![run.tenant,run.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?)))?.map(|r|{let(k,id,revision)=r?;Ok(ArtifactDependency{kind:if k=="source"{ArtifactDependencyKind::Source}else{ArtifactDependencyKind::Memory},id,revision})}).collect::<Result<Vec<_>>>()?;
    let artifacts=conn.prepare("SELECT artifact_id,revision FROM personal_artifact_run_dependencies WHERE tenant_id=?1 AND run_id=?2")?.query_map(params![run.tenant,run.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (id, revision) in artifacts {
        if Some(id.as_str()) == target {
            refs.extend(dependencies(conn, &run.tenant, &id, revision)?)
        } else {
            refs.push(ArtifactDependency {
                kind: ArtifactDependencyKind::Artifact,
                id,
                revision,
            })
        }
    }
    Ok(refs)
}
fn add_read(conn: &Connection, run: &Run, object: &str, revision: i64) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO personal_artifact_run_dependencies VALUES(?1,?2,?3,?4)",
        params![run.tenant, run.id, object, revision],
    )?;
    for d in dependencies(conn, &run.tenant, object, revision)? {
        if d.kind != ArtifactDependencyKind::Artifact {
            conn.execute(
                "INSERT OR IGNORE INTO agent_dependencies VALUES(?1,?2,?3,?4,?5)",
                params![run.tenant, run.id, depkind(d.kind), d.id, d.revision],
            )?;
        } else {
            conn.execute(
                "INSERT OR IGNORE INTO personal_artifact_run_dependencies VALUES(?1,?2,?3,?4)",
                params![run.tenant, run.id, d.id, d.revision],
            )?;
        }
    }
    Ok(())
}
pub(super) fn unique_anchor(body: &str, anchor: &str) -> bool {
    if anchor.is_empty() {
        return false;
    }
    let Some(first) = body.find(anchor) else {
        return false;
    };
    let skip = anchor
        .chars()
        .next()
        .expect("nonempty literal anchor")
        .len_utf8();
    !body[first + skip..].contains(anchor)
}
type StoredPlan = (String, String, Option<String>, Option<i64>, i64, String);
pub(crate) fn tool(
    conn: &Connection,
    run: &Run,
    request: &ArtifactToolRequest,
) -> Result<serde_json::Value> {
    let Some(profile) = profile(conn, &run.tenant, &run.id)? else {
        return Ok(serde_json::json!({"error":"unavailable"}));
    };
    if !profile.planned {
        return Err(StoreError::Conflict);
    }
    let result = (|| match request {
        ArtifactToolRequest::Search { kind, after } => {
            let p = list(conn, &run.tenant, *kind, after.clone(), 5, false)?;
            for a in &p.items {
                add_read(conn, run, &a.id, a.revision)?;
            }
            serde_json::to_value(p).map_err(|_| StoreError::Corrupt)
        }
        ArtifactToolRequest::Briefing { after } => {
            let p = list(
                conn,
                &run.tenant,
                Some(ArtifactKind::Followup),
                after.clone(),
                5,
                true,
            )?;
            for a in &p.items {
                add_read(conn, run, &a.id, a.revision)?;
            }
            serde_json::to_value(p).map_err(|_| StoreError::Corrupt)
        }
        ArtifactToolRequest::Read {
            id,
            revision,
            start_byte,
            max_bytes,
        } => {
            if *max_bytes == 0 || *max_bytes > 8192 {
                return Err(StoreError::InvalidInput);
            }
            let v = version(conn, &run.tenant, id, None)?;
            if v.artifact.revision != *revision {
                return Err(StoreError::Conflict);
            }
            let d = v.draft.ok_or(StoreError::NotFound)?;
            let start = *start_byte as usize;
            if start > d.body.len() || !d.body.is_char_boundary(start) {
                return Err(StoreError::InvalidInput);
            }
            let mut end = (start + *max_bytes as usize).min(d.body.len());
            while !d.body.is_char_boundary(end) {
                end -= 1
            }
            if start == end && end < d.body.len() {
                return Err(StoreError::InvalidInput);
            }
            add_read(conn, run, id, *revision)?;
            Ok(
                serde_json::json!({"artifact":v.artifact,"body":&d.body[start..end],"payload":d.payload,"next_start_byte":if end<d.body.len(){Some(end)}else{None}}),
            )
        }
        ArtifactToolRequest::Write {
            title,
            body,
            payload,
            anchor,
        } => {
            if body.len() > 2048 || encode(payload)?.len() > 1024 {
                return Err(StoreError::InvalidInput);
            }
            let external:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM mail_run_dependencies WHERE tenant_id=?1 AND run_id=?2) OR EXISTS(SELECT 1 FROM external_tool_dispatches d JOIN chat_runs r ON r.tenant_id=d.tenant_id AND r.id=d.run_id WHERE r.tenant_id=?1 AND r.thread_id=?3 AND r.sequence<=?4)",params![run.tenant,run.id,run.thread,run.sequence],|r|r.get(0))?;
            if external || crate::calendar::agent::persistence_context(conn, run)? {
                return Ok(serde_json::json!({"error":"external_provenance_unavailable"}));
            }
            let plan:Option<StoredPlan>=conn.query_row("SELECT action,kind,target_id,target_revision,generation,request_id FROM personal_artifact_command_plans WHERE tenant_id=?1 AND run_id=?2 AND schema_version=1",params![run.tenant,run.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
            let Some((action, k, target, revision, generation, request_id)) = plan else {
                return Err(StoreError::Forbidden);
            };
            if k != kind(payload.kind()) {
                return Err(StoreError::Forbidden);
            }
            let current = target
                .as_ref()
                .map(|id| version(conn, &run.tenant, id, revision))
                .transpose()?;
            // Preserve inherited dates unless the CURRENT request supplies an
            // unambiguous explicit UTC:<Unix-seconds> value or explicit removal.
            if let ArtifactPayload::Followup {
                due,
                review,
                snooze,
                ..
            } = payload
            {
                let old = current
                    .as_ref()
                    .and_then(|v| v.draft.as_ref())
                    .map(|d| &d.payload);
                let (a, b, c) = match old {
                    Some(ArtifactPayload::Followup {
                        due,
                        review,
                        snooze,
                        ..
                    }) => (due.as_ref(), review.as_ref(), snooze.as_ref()),
                    _ => (None, None, None),
                };
                for (field, new, old) in [
                    ("due", due.as_ref(), a),
                    ("review", review.as_ref(), b),
                    ("snooze", snooze.as_ref(), c),
                ] {
                    if encode(&new)? == encode(&old)? {
                        continue;
                    }
                    if let Some(d) = new {
                        if d.timezone != "UTC" || !run.text.contains(&format!("UTC:{}", d.utc)) {
                            return Err(StoreError::InvalidInput);
                        }
                    } else if old.is_some() && !run.text.contains(&format!("remove {field}")) {
                        return Err(StoreError::InvalidInput);
                    }
                }
            }
            if let ArtifactPayload::Goal { lifecycle, .. } = payload {
                let old = current
                    .as_ref()
                    .and_then(|v| v.draft.as_ref())
                    .and_then(|d| {
                        if let ArtifactPayload::Goal { lifecycle, .. } = &d.payload {
                            Some(*lifecycle)
                        } else {
                            None
                        }
                    })
                    .unwrap_or(GoalLifecycle::Active);
                if *lifecycle != old {
                    return Err(StoreError::InvalidInput);
                }
            }
            let body = if action == "edit" {
                let old = current
                    .as_ref()
                    .and_then(|v| v.draft.as_ref())
                    .ok_or(StoreError::Conflict)?;
                let anchor = anchor
                    .as_ref()
                    .filter(|s| !s.is_empty() && s.len() <= 512)
                    .ok_or(StoreError::InvalidInput)?;
                if !unique_anchor(&old.body, anchor) {
                    return Err(StoreError::Conflict);
                }
                old.body.replacen(anchor, body, 1)
            } else {
                if anchor.is_some() {
                    return Err(StoreError::InvalidInput);
                }
                body.clone()
            };
            let input = ArtifactWrite {
                request_id,
                id: target.clone(),
                expected_revision: revision,
                draft: ArtifactDraft {
                    title: title.clone(),
                    body,
                    payload: payload.clone(),
                    references: vec![],
                },
            };
            // Replay a committed command before checking generations/head. The receipt is
            // content-free; changed payload under the sealed command UUID still conflicts.
            let hash = digest(&input)?;
            if let Some(r) = receipt(conn, &run.tenant, &input.request_id, &hash)? {
                return serde_json::to_value(r).map_err(|_| StoreError::Corrupt);
            }
            if generation != knowledge::generation(conn, &run.tenant)? {
                return Err(StoreError::Conflict);
            }
            let refs = run_refs(conn, run, target.as_deref())?;
            let r = write(conn, &run.tenant, input, refs, Some(&run.id))?;
            conn.execute("UPDATE personal_artifact_command_plans SET used=1 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;
            // Future context containing the receipt remains dependent on the saved object.
            conn.execute(
                "INSERT OR IGNORE INTO personal_artifact_run_dependencies VALUES(?1,?2,?3,?4)",
                params![run.tenant, run.id, r.id, r.revision],
            )?;
            serde_json::to_value(r).map_err(|_| StoreError::Corrupt)
        }
    })();
    match result {
        Ok(v) => Ok(v),
        Err(StoreError::NotFound) => Ok(serde_json::json!({"error":"not_found"})),
        Err(StoreError::InvalidInput) => Ok(serde_json::json!({"error":"invalid_input"})),
        Err(StoreError::Conflict) => Ok(serde_json::json!({"error":"revision_conflict_no_write"})),
        Err(StoreError::Forbidden) => {
            Ok(serde_json::json!({"error":"current_request_write_not_authorized"}))
        }
        Err(e) => Err(e),
    }
}
impl Store {
    /// Called only after the isolated current-message planning step commits, before
    /// any local retrieval. No retrieved quote or model-provided ID is authority.
    pub async fn prepare_artifact_intent(
        &self,
        lease: &StepLease,
        intent: Option<ArtifactIntent>,
    ) -> Result<ArtifactRunProfile> {
        let lease = lease.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let run=crate::agent_execution::guard_step(&tx,&db.provider_key,&lease)?;let p=profile(&tx,&run.tenant,&run.id)?.ok_or(StoreError::Conflict)?;if p.planned{return Ok(p)}let tools:i64=tx.query_row("SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;if tools!=0{return Err(StoreError::Conflict)}
 if let Some(intent)=intent{if !["create","edit"].contains(&intent.action.as_str())||intent.intent_quote.trim().is_empty()||intent.intent_quote.len()>512||!run.text.contains(&intent.intent_quote){return Err(StoreError::InvalidInput)}let target=if intent.action=="create"{if intent.target_quote.is_some(){return Err(StoreError::InvalidInput)}None}else if let Some(target)=p.selected{Some(target)}else{let quote=intent.target_quote.filter(|s|!s.trim().is_empty()&&s.len()<=128&&run.text.contains(s)).ok_or(StoreError::Conflict)?;let candidates=tx.prepare("SELECT a.id,a.revision,json_extract(v.draft,'$.title') FROM personal_artifacts a JOIN personal_artifact_versions v ON v.tenant_id=a.tenant_id AND v.artifact_id=a.id AND v.revision=a.revision WHERE a.tenant_id=?1 AND a.kind=?2 AND a.status='active' ORDER BY a.id LIMIT 257")?.query_map(params![run.tenant,kind(intent.kind)],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;if candidates.len()>256{return Err(StoreError::Conflict)}let matching=candidates.into_iter().filter(|a|a.2==quote).collect::<Vec<_>>();if matching.len()!=1{return Err(StoreError::Conflict)}Some(ArtifactTarget{id:matching[0].0.clone(),revision:matching[0].1})};if let Some(t)=&target{let v=version(&tx,&run.tenant,&t.id,None)?;if v.artifact.revision!=t.revision||v.artifact.kind!=intent.kind||v.draft.is_none(){return Err(StoreError::Conflict)}}tx.execute("INSERT INTO personal_artifact_command_plans(tenant_id,run_id,schema_version,action,kind,target_id,target_revision,generation,used,request_id) VALUES(?1,?2,1,?3,?4,?5,?6,?7,0,?8)",params![run.tenant,run.id,intent.action,kind(intent.kind),target.as_ref().map(|t|&t.id),target.as_ref().map(|t|t.revision),knowledge::generation(&tx,&run.tenant)?,Uuid::new_v4().to_string()])?;}
 tx.execute("UPDATE personal_artifact_run_profiles SET planned=1 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;let result=profile(&tx,&run.tenant,&run.id)?.ok_or(StoreError::Corrupt)?;tx.commit()?;Ok(result)}).await
    }
}
impl Store {
    pub async fn initialize_artifact_readonly(
        &self,
        lease: &RunLease,
    ) -> Result<ArtifactRunProfile> {
        let lease = lease.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let run=crate::chat::guard(&tx,&db.provider_key,&lease)?;let p=profile(&tx,&run.tenant,&run.id)?.ok_or(StoreError::Conflict)?;if !p.planned{tx.execute("UPDATE personal_artifact_run_profiles SET planned=1 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;}let p=profile(&tx,&run.tenant,&run.id)?.ok_or(StoreError::Corrupt)?;tx.commit()?;Ok(p)}).await
    }
}
pub(crate) fn receipts(conn: &Connection, tenant: &str, run: &str) -> Result<Vec<ArtifactReceipt>> {
    if !conn.query_row("SELECT EXISTS(SELECT 1 FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND length(text)>0)",params![tenant,run],|r|r.get::<_,bool>(0))?{return Ok(vec![])}
    let rows=conn.prepare("SELECT m.request_id,m.artifact_id,m.revision,m.status FROM personal_artifact_command_plans p JOIN personal_artifact_mutations m ON m.tenant_id=p.tenant_id AND m.request_id=p.request_id JOIN personal_artifacts a ON a.tenant_id=m.tenant_id AND a.id=m.artifact_id WHERE p.tenant_id=?1 AND p.run_id=?2 AND a.status='active'")?.query_map(params![tenant,run],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    rows.into_iter()
        .map(|(request_id, id, revision, s)| {
            Ok(ArtifactReceipt {
                request_id,
                url: format!("/app/artifacts/{id}"),
                id,
                revision,
                status: status(&s)?,
            })
        })
        .collect()
}
/// The write, tool receipt, final answer and terminal state share one transaction.
/// A lost worker response therefore cannot turn a committed write into a new draft.
pub(crate) fn finish_confirmation(conn: &Connection, run: &Run) -> Result<()> {
    let (object, revision): (String, i64) = conn.query_row(
        "SELECT m.artifact_id,m.revision FROM personal_artifact_command_plans p JOIN personal_artifact_mutations m ON m.tenant_id=p.tenant_id AND m.request_id=p.request_id WHERE p.tenant_id=?1 AND p.run_id=?2 AND p.used=1",
        params![run.tenant,run.id], |r| Ok((r.get(0)?,r.get(1)?))
    ).optional()?.ok_or(StoreError::Conflict)?;
    let v = version(conn, &run.tenant, &object, None)?;
    if v.draft.is_none() || v.artifact.revision != revision {
        return Err(StoreError::Conflict);
    }
    crate::chat::apply_patches(
        conn,
        run,
        vec![crate::OutputPatch::Append {
            item: "artifact-receipt".into(),
            text: format!("기록을 저장했습니다 · 버전 {revision}\n/app/artifacts/{object}"),
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )?;
    crate::chat::transition(conn, run, RunState::Completed, None)
}
