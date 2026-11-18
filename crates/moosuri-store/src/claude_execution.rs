//! Opaque owner-owned official CLI environments. No OAuth material enters Store.
use super::*;
use moosuri_core::{ModelChoice, ModelSelection, ModelSource, ProviderKind};
#[derive(Clone, serde::Serialize)]
pub struct ClaudeBindingSummary {
    pub id: String,
    pub revision: i64,
    pub label: String,
    pub status: String,
    pub cleanup_state: String,
    pub effort_metadata_pending: bool,
}
/// Native launch metadata. The environment is an opaque UUID, never a path.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct ClaudeExecutionRef {
    pub id: String,
    pub revision: i64,
    pub environment_id: String,
    pub profile_digest: String,
    pub root_id: String,
}
/// Versioned host-mediated function protocol. Native CLI tools remain disabled.
const HOST_TOOL_HARNESS: &str = "claude-host-functions-v1";
/// Frozen owner/model/effort proof; only Store can create its admission identity.
pub struct ClaudeToolProbe {
    pub binding: providers::ProviderBinding,
    pub marker: String,
    tenant: String,
    account: String,
    id: String,
    selection: ModelSelection,
    environment: ClaudeExecutionRef,
}
fn same_environment(a: &ClaudeExecutionRef, b: &ClaudeExecutionRef) -> bool {
    a.id == b.id
        && a.revision == b.revision
        && a.environment_id == b.environment_id
        && a.profile_digest == b.profile_digest
        && a.root_id == b.root_id
}
fn tool_proven(c: &Connection, tenant: &str, account: &str, choice: &ModelChoice) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM claude_code_tool_probes WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4 AND model=?5 AND effort=?6 AND harness=?7)", params![tenant,account,choice.provider_id,choice.provider_revision,choice.model,choice.effort.as_deref().unwrap_or(""),HOST_TOOL_HARNESS], |row| row.get(0))?)
}
fn reference(
    c: &Connection,
    tenant: &str,
    account: &str,
    id: &str,
    revision: i64,
) -> Result<ClaudeExecutionRef> {
    c.query_row("SELECT environment_id,profile_digest,root_id FROM claude_code_bindings WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND revision=?4 AND status IN('pending','active')",params![tenant,account,id,revision],|r|Ok(ClaudeExecutionRef{id:id.into(),revision,environment_id:r.get(0)?,profile_digest:r.get(1)?,root_id:r.get(2)?})).optional()?.ok_or(StoreError::NotFound)
}
// Settings transport supports these exact levels; a model name never implies support.
fn valid_effort(effort: &str) -> bool {
    matches!(effort, "low" | "medium" | "high" | "xhigh")
}
fn reported_efforts(value: Option<String>) -> Result<Vec<String>> {
    let values: Vec<String> = value
        .map(|value| serde_json::from_str(&value).map_err(|_| StoreError::Corrupt))
        .transpose()?
        .unwrap_or_default();
    if values.len() > 4 || values.iter().any(|value| !valid_effort(value)) {
        return Err(StoreError::Corrupt);
    }
    Ok(values)
}
pub(super) fn choices(c: &Connection, u: &UserContext) -> Result<Vec<ModelChoice>> {
    let mut q=c.prepare("SELECT b.id,b.revision,b.label,p.model,p.verified_at,p.supported_efforts FROM claude_code_bindings b JOIN claude_code_probes p ON p.tenant_id=b.tenant_id AND p.account_id=b.account_id AND p.binding_id=b.id AND p.revision=b.revision WHERE b.tenant_id=?1 AND b.account_id=?2 AND b.status='active' ORDER BY b.label,p.model")?;
    let rows = q
        .query_map(params![u.tenant.0, u.account.id], |r| {
            Ok((
                ModelChoice {
                    source: ModelSource::PersonalClaudeCode,
                    provider_id: r.get(0)?,
                    provider_revision: r.get(1)?,
                    provider_name: r.get(2)?,
                    kind: ProviderKind::Anthropic,
                    model: r.get(3)?,
                    effort: None,
                    verified_at: r.get(4)?,
                    function_tools: false,
                },
                r.get::<_, Option<String>>(5)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut choices = Vec::new();
    for (choice, metadata) in rows {
        let efforts = reported_efforts(metadata)?;
        let mut variants = vec![choice.clone()];
        variants.extend(efforts.into_iter().map(|effort| ModelChoice {
            effort: Some(effort),
            ..choice.clone()
        }));
        for mut variant in variants {
            variant.function_tools = tool_proven(c, &u.tenant.0, &u.account.id, &variant)?;
            choices.push(variant);
        }
    }
    Ok(choices)
}
pub(super) fn selected(c: &Connection, u: &UserContext) -> Result<Option<ModelSelection>> {
    c.query_row("SELECT binding_id,revision,model,effort FROM personal_claude_selections WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|Ok(ModelSelection{source:ModelSource::PersonalClaudeCode,provider_id:r.get(0)?,provider_revision:r.get(1)?,model:r.get(2)?,effort:r.get(3)?})).optional().map_err(Into::into)
}
pub(super) fn verified(c: &Connection, u: &UserContext, s: &ModelSelection) -> Result<ModelChoice> {
    if s.source != ModelSource::PersonalClaudeCode {
        return Err(StoreError::Conflict);
    }
    choices(c, u)?
        .into_iter()
        .find(|v| {
            v.provider_id == s.provider_id
                && v.provider_revision == s.provider_revision
                && v.model == s.model
                && v.effort == s.effort
        })
        .ok_or(StoreError::Conflict)
}
pub(super) fn binding(
    c: &Connection,
    tenant: &str,
    account: &str,
    v: &ModelChoice,
) -> Result<providers::ProviderBinding> {
    if v.source != ModelSource::PersonalClaudeCode {
        return Err(StoreError::Conflict);
    }
    let metadata:Option<Option<String>>=c.query_row("SELECT p.supported_efforts FROM claude_code_bindings b JOIN claude_code_probes p ON p.tenant_id=b.tenant_id AND p.account_id=b.account_id AND p.binding_id=b.id AND p.revision=b.revision WHERE b.tenant_id=?1 AND b.account_id=?2 AND b.id=?3 AND b.revision=?4 AND b.status='active' AND p.model=?5",params![tenant,account,v.provider_id,v.provider_revision,v.model],|r|r.get(0)).optional()?;
    let metadata = metadata.ok_or(StoreError::Conflict)?;
    if v.effort
        .as_ref()
        .is_some_and(|effort| !valid_effort(effort))
    {
        return Err(StoreError::Conflict);
    }
    if let Some(effort) = &v.effort {
        if !reported_efforts(metadata)?.contains(effort) {
            return Err(StoreError::Conflict);
        }
    }
    // Old stored text-only choices remain text-only after a new proof succeeds.
    if v.function_tools && !tool_proven(c, tenant, account, v)? {
        return Err(StoreError::Conflict);
    }
    Ok(providers::ProviderBinding {
        claude_code: Some(reference(
            c,
            tenant,
            account,
            &v.provider_id,
            v.provider_revision,
        )?),
        subscription: None,
        id: v.provider_id.clone(),
        revision: v.provider_revision,
        config: moosuri_core::ProviderConfig {
            kind: ProviderKind::Anthropic,
            base_url: None,
            allowed_ips: vec![],
            router_providers: vec![],
            zero_data_retention: false,
        },
        model: v.model.clone(),
        effort: v.effort.clone(),
        credential: None,
    })
}
impl Store {
    /// Prepare an exact text-eligible choice; catalog presence alone grants no tools.
    pub async fn claude_tool_probe_binding(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
        model: String,
        effort: Option<String>,
    ) -> Result<ClaudeToolProbe> {
        let u = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &u, true, false)?;
            let selection = ModelSelection { source: ModelSource::PersonalClaudeCode,
                provider_id: id, provider_revision: revision, model, effort };
            let choice = verified(&tx, &u, &selection)?;
            let binding = binding(&tx, &u.tenant.0, &u.account.id, &choice)?;
            let environment = binding.claude_code.clone().ok_or(StoreError::Corrupt)?;
            let id = Uuid::new_v4().to_string();
            let marker = format!("proof-{}", random_token()?);
            let at = now()?;
            tx.execute("DELETE FROM claude_code_tool_probe_attempts WHERE expires_at<=?1", [at])?;
            let pending: i64 = tx.query_row("SELECT count(*) FROM claude_code_tool_probe_attempts WHERE tenant_id=?1 AND account_id=?2", params![u.tenant.0,u.account.id], |row|row.get(0))?;
            if pending >= 8 { return Err(StoreError::Conflict); }
            tx.execute("INSERT INTO claude_code_tool_probe_attempts VALUES(?1,?2,?3,?4,?5,?6,?7,?8,0,?9)", params![u.tenant.0,u.account.id,selection.provider_id,selection.provider_revision,selection.model,selection.effort.as_deref().unwrap_or(""),HOST_TOOL_HARNESS,id,at+600])?;
            tx.commit()?;
            Ok(ClaudeToolProbe { binding, marker, tenant:u.tenant.0, account:u.account.id,
                id, selection, environment })
        }).await
    }
    /// Reserve each actual inference before launch, including failed/unknown rounds.
    pub async fn reserve_claude_tool_probe(
        &self,
        u: &UserContext,
        proof: &ClaudeToolProbe,
        round: u8,
    ) -> Result<()> {
        let u = u.clone();
        let (tenant, account, id, selection, environment) = (
            proof.tenant.clone(),
            proof.account.clone(),
            proof.id.clone(),
            proof.selection.clone(),
            proof.environment.clone(),
        );
        self.call(move |db| {
            if !matches!(round, 1 | 2) { return Err(StoreError::InvalidInput); }
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &u, true, false)?;
            if u.tenant.0 != tenant || u.account.id != account { return Err(StoreError::NotFound); }
            let choice = verified(&tx,&u,&selection)?;
            let current = binding(&tx,&tenant,&account,&choice)?.claude_code.ok_or(StoreError::Corrupt)?;
            if !same_environment(&current,&environment) { return Err(StoreError::Conflict); }
            let changed = tx.execute("UPDATE claude_code_tool_probe_attempts SET reserved_rounds=?4 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND reserved_rounds=?4-1 AND expires_at>?5", params![tenant,account,id,round,now()?])?;
            if changed != 1 { return Err(StoreError::Conflict); }
            crate::recovery_budget::reserve(&tx,&tenant,&account,&format!("{id}:{round}"),"probe",0)?;
            tx.commit()?;
            Ok(())
        }).await
    }
    /// Trusted caller verified both complete rounds and host result consumption.
    pub async fn record_claude_tool_probe(
        &self,
        u: &UserContext,
        proof: ClaudeToolProbe,
    ) -> Result<ModelChoice> {
        let u = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &u, true, false)?;
            if u.tenant.0 != proof.tenant || u.account.id != proof.account { return Err(StoreError::NotFound); }
            let choice = verified(&tx,&u,&proof.selection)?;
            let current = binding(&tx,&proof.tenant,&proof.account,&choice)?.claude_code.ok_or(StoreError::Corrupt)?;
            if !same_environment(&current,&proof.environment) { return Err(StoreError::Conflict); }
            let consumed = tx.execute("DELETE FROM claude_code_tool_probe_attempts WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND reserved_rounds=2 AND expires_at>?4", params![proof.tenant,proof.account,proof.id,now()?])?;
            if consumed != 1 { return Err(StoreError::Conflict); }
            tx.execute("INSERT INTO claude_code_tool_probes VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT DO UPDATE SET verified_at=excluded.verified_at", params![proof.tenant,proof.account,choice.provider_id,choice.provider_revision,choice.model,choice.effort.as_deref().unwrap_or(""),HOST_TOOL_HARNESS,now()?])?;
            let result = verified(&tx,&u,&proof.selection)?;
            tx.commit()?;
            Ok(result)
        }).await
    }
    pub async fn claude_retirements(&self, profile: String) -> Result<Vec<String>> {
        self.call(move|db|{let mut q=db.conn.prepare("SELECT environment_id FROM claude_code_retirements WHERE root_id=?1 AND state='pending' ORDER BY environment_id LIMIT 8")?;q.query_map([profile],|r|r.get(0))?.collect::<std::result::Result<Vec<_>,_>>().map_err(Into::into)}).await
    }
    pub async fn acknowledge_claude_retirement(
        &self,
        environment: String,
        root: String,
    ) -> Result<()> {
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let account:String=tx.query_row("SELECT account_id FROM claude_code_retirements WHERE environment_id=?1 AND root_id=?2",params![environment,root],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound)?;tx.execute("UPDATE claude_code_retirements SET state='acknowledged' WHERE environment_id=?1 AND root_id=?2",params![environment,root])?;tx.execute("UPDATE account_deletions SET local_state='complete' WHERE account_id=?1 AND local_state='pending' AND NOT EXISTS(SELECT 1 FROM claude_code_retirements r WHERE r.account_id=account_deletions.account_id AND r.state='pending')",[account])?;tx.commit()?;Ok(())}).await
    }

    pub async fn record_claude_usage_breakdown(
        &self,
        l: &StepLease,
        values: Vec<(String, moosuri_core::RunUsage)>,
    ) -> Result<()> {
        let l = l.clone();
        self.call(move|db|{if values.is_empty()||values.len()>32{return Err(StoreError::InvalidInput)}let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let nonce:Vec<u8>=tx.query_row("SELECT nonce FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3 AND step=?4",params![l.run.tenant,l.run.run,l.run.attempt,l.step],|r|r.get(0))?;if nonce!=l.nonce{return Err(StoreError::Unauthorized)}for(model,usage)in values{if model.is_empty()||model.len()>200||!model.bytes().all(|b|b.is_ascii_graphic()){return Err(StoreError::InvalidInput)}let value=serde_json::to_string(&usage).map_err(|_|StoreError::InvalidInput)?;let old:Option<String>=tx.query_row("SELECT usage FROM claude_code_usage WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3 AND step=?4 AND model=?5",params![l.run.tenant,l.run.run,l.run.attempt,l.step,model],|r|r.get(0)).optional()?;if old.as_ref().is_some_and(|v|v!=&value){return Err(StoreError::Conflict)}tx.execute("INSERT OR IGNORE INTO claude_code_usage VALUES(?1,?2,?3,?4,?5,?6)",params![l.run.tenant,l.run.run,l.run.attempt,l.step,model,value])?;}tx.commit()?;Ok(())}).await
    }

    pub async fn claude_run_model(&self, l: &RunLease) -> Result<String> {
        Ok(self.claude_run_selection(l).await?.model)
    }

    /// Return the exact saved run choice, even if settings changed after enqueue.
    pub async fn claude_run_selection(&self, l: &RunLease) -> Result<ModelSelection> {
        let l = l.clone();
        self.call(move |db| {
            let r = crate::chat::guard(&db.conn, &db.provider_key, &l)?;
            if r.model.source != ModelSource::PersonalClaudeCode {
                return Err(StoreError::Conflict);
            }
            binding(&db.conn, &r.tenant, &r.account, &r.model)?;
            Ok(ModelSelection {
                source: r.model.source,
                provider_id: r.model.provider_id,
                provider_revision: r.model.provider_revision,
                model: r.model.model,
                effort: r.model.effort,
            })
        })
        .await
    }

    pub async fn claude_bindings(&self, u: &UserContext) -> Result<Vec<ClaudeBindingSummary>> {
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            let mut q = db.conn.prepare(
                "SELECT b.id,b.revision,b.label,b.status,
                    coalesce((SELECT state FROM claude_code_retirements r WHERE r.environment_id=b.environment_id),'not_requested'),
                    EXISTS(SELECT 1 FROM claude_code_probes p WHERE p.tenant_id=b.tenant_id AND p.account_id=b.account_id
                        AND p.binding_id=b.id AND p.revision=b.revision AND p.supported_efforts IS NULL)
                 FROM claude_code_bindings b WHERE b.tenant_id=?1 AND b.account_id=?2 ORDER BY b.label,b.id",
            )?;
            q.query_map(params![u.tenant.0, u.account.id], |r| {
                Ok(ClaudeBindingSummary {
                    id: r.get(0)?, revision: r.get(1)?, label: r.get(2)?, status: r.get(3)?,
                    cleanup_state: r.get(4)?, effort_metadata_pending: r.get(5)?,
                })
            })?.collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
        }).await
    }
    pub async fn create_claude_binding(
        &self,
        u: &UserContext,
        label: String,
        profile_digest: String,
        root_id: String,
    ) -> Result<ClaudeExecutionRef> {
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            if label.trim().is_empty()
                || label.len() > 80
                || root_id.len() != 64
                || !root_id.bytes().all(|b| b.is_ascii_hexdigit())
                || profile_digest.len() != 64
                || !profile_digest.bytes().all(|b| b.is_ascii_hexdigit())
            {
                return Err(StoreError::InvalidInput);
            }
            let n: i64 = db.conn.query_row(
                "SELECT count(*) FROM claude_code_bindings WHERE tenant_id=?1 AND account_id=?2",
                params![u.tenant.0, u.account.id],
                |r| r.get(0),
            )?;
            if n >= 8 {
                return Err(StoreError::Conflict);
            }
            let id = Uuid::new_v4().to_string();
            let environment_id = Uuid::new_v4().to_string();
            db.conn.execute(
                "INSERT INTO claude_code_bindings VALUES(?1,?2,?3,1,?4,?5,?6,?7,'pending')",
                params![
                    u.tenant.0,
                    u.account.id,
                    id,
                    label.trim(),
                    environment_id,
                    profile_digest,
                    root_id
                ],
            )?;
            Ok(ClaudeExecutionRef {
                id,
                revision: 1,
                environment_id,
                profile_digest,
                root_id,
            })
        })
        .await
    }
    pub async fn claude_launch_ref(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<ClaudeExecutionRef> {
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            reference(&db.conn, &u.tenant.0, &u.account.id, &id, revision)
        })
        .await
    }
    /// Official login may only mutate an environment with no verified proof.
    /// Active accounts switch credentials by creating a fresh binding.
    pub async fn claude_login_ref(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<ClaudeExecutionRef> {
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            let r = reference(&db.conn, &u.tenant.0, &u.account.id, &id, revision)?;
            let pending: bool = db.conn.query_row(
                "SELECT status='pending' FROM claude_code_bindings WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND revision=?4",
                params![u.tenant.0, u.account.id, id, revision], |row| row.get(0))?;
            if !pending { return Err(StoreError::Conflict); }
            Ok(r)
        }).await
    }
    pub async fn record_claude_probe(
        &self,
        u: &UserContext,
        r: ClaudeExecutionRef,
        model: String,
    ) -> Result<ModelChoice> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let current=reference(&tx,&u.tenant.0,&u.account.id,&r.id,r.revision)?;if current.root_id!=r.root_id||current.profile_digest!=r.profile_digest||current.environment_id!=r.environment_id||model.len()>200||!model.starts_with("claude-")||!model.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'.'){return Err(StoreError::Conflict)}let n:i64=tx.query_row("SELECT count(*) FROM claude_code_probes WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3",params![u.tenant.0,u.account.id,r.id],|r|r.get(0))?;if n>=32{return Err(StoreError::Conflict)}tx.execute("INSERT INTO claude_code_probes(tenant_id,account_id,binding_id,revision,model,verified_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT DO UPDATE SET verified_at=excluded.verified_at",params![u.tenant.0,u.account.id,r.id,r.revision,model,now()?])?;tx.execute("UPDATE claude_code_bindings SET status='active' WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,r.id])?;let v=verified(&tx,&u,&ModelSelection{source:ModelSource::PersonalClaudeCode,provider_id:r.id,provider_revision:r.revision,model,effort:None})?;tx.commit()?;Ok(v)}).await
    }

    /// Backfill catalog metadata without performing or admitting another inference.
    /// Only an exact text-proven model in this owner/environment can be updated.
    pub async fn record_claude_model_efforts(
        &self,
        u: &UserContext,
        expected: ClaudeExecutionRef,
        model: String,
        mut efforts: Vec<String>,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            if efforts.len() > 4 || efforts.iter().any(|value| !valid_effort(value)) {
                return Err(StoreError::InvalidInput);
            }
            // Stable display order regardless of the catalog's ordering/duplicates.
            efforts.sort_by_key(|value| match value.as_str() {
                "low" => 0, "medium" => 1, "high" => 2, _ => 3,
            });
            efforts.dedup();
            let current = reference(&db.conn, &u.tenant.0, &u.account.id, &expected.id, expected.revision)?;
            if current.root_id != expected.root_id
                || current.profile_digest != expected.profile_digest
                || current.environment_id != expected.environment_id
            {
                return Err(StoreError::Conflict);
            }
            let metadata = serde_json::to_string(&efforts).map_err(|_| StoreError::InvalidInput)?;
            let changed = db.conn.execute(
                "UPDATE claude_code_probes SET supported_efforts=?6 WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4 AND model=?5",
                params![u.tenant.0, u.account.id, expected.id, expected.revision, model, metadata],
            )?;
            if changed != 1 { return Err(StoreError::Conflict); }
            Ok(())
        }).await
    }

    /// Model IDs with fetched metadata, including confirmed unsupported models.
    /// Legacy NULL entries remain eligible for a metadata-only discovery refresh.
    pub async fn claude_model_efforts_known(
        &self,
        u: &UserContext,
        expected: &ClaudeExecutionRef,
    ) -> Result<Vec<String>> {
        let u = u.clone();
        let expected = expected.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            let current = reference(&db.conn, &u.tenant.0, &u.account.id, &expected.id, expected.revision)?;
            if current.root_id != expected.root_id
                || current.profile_digest != expected.profile_digest
                || current.environment_id != expected.environment_id
            {
                return Err(StoreError::Conflict);
            }
            let mut query = db.conn.prepare("SELECT model FROM claude_code_probes WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4 AND supported_efforts IS NOT NULL ORDER BY model")?;
            query.query_map(params![u.tenant.0, u.account.id, expected.id, expected.revision], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>().map_err(Into::into)
        }).await
    }
    /// Commit one exact owner/environment probe before any inference subprocess.
    /// Unknown and failed requests consume their slot; settlement is not admission.
    pub async fn reserve_claude_probe(
        &self,
        u: &UserContext,
        expected: &ClaudeExecutionRef,
    ) -> Result<()> {
        let u = u.clone();
        let expected = expected.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &u, true, false)?;
            let current = reference(
                &tx,
                &u.tenant.0,
                &u.account.id,
                &expected.id,
                expected.revision,
            )?;
            if current.root_id != expected.root_id
                || current.profile_digest != expected.profile_digest
                || current.environment_id != expected.environment_id
            {
                return Err(StoreError::Conflict);
            }
            crate::recovery_budget::reserve(
                &tx,
                &u.tenant.0,
                &u.account.id,
                &Uuid::new_v4().to_string(),
                "probe",
                0,
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
    pub async fn disconnect_claude(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &u, true, false)?;
            let changed = tx.execute(
                "UPDATE claude_code_bindings SET status='disconnected',revision=revision+1 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND revision=?4 AND status!='disconnected'",
                params![u.tenant.0, u.account.id, id, revision],
            )?;
            if changed != 1 {
                return Err(StoreError::NotFound);
            }
            crate::calendar::agent::invalidate_personal_model(
                &tx, &u.tenant.0, &u.account.id, &id, revision, ModelSource::PersonalClaudeCode,
            )?;
            tx.commit()?;
            Ok(())
        }).await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory, member};
    #[tokio::test]
    async fn host_tools_require_two_reserved_rounds_and_exact_owner_model_effort() {
        let _g = TEST_LOCK.lock().await;
        let d = directory();
        let s = Store::open(d.path()).unwrap();
        let a = admin(&s).await;
        let b = member(&s, &a.user, "host-tools-member@example.test").await;
        let r = s
            .create_claude_binding(&a.user, "Claude".into(), "a".repeat(64), "b".repeat(64))
            .await
            .unwrap();
        let model = "claude-fixture-v1".to_owned();
        s.record_claude_probe(&a.user, r.clone(), model.clone())
            .await
            .unwrap();
        s.record_claude_model_efforts(&a.user, r.clone(), model.clone(), vec!["high".into()])
            .await
            .unwrap();
        assert!(
            s.claude_tool_probe_binding(&b.user, r.id.clone(), r.revision, model.clone(), None)
                .await
                .is_err()
        );
        assert!(
            s.claude_tool_probe_binding(
                &a.user,
                r.id.clone(),
                r.revision,
                model.clone(),
                Some("low".into())
            )
            .await
            .is_err()
        );
        assert!(
            s.claude_tool_probe_binding(
                &a.user,
                r.id.clone(),
                r.revision,
                "claude-unproven".into(),
                None
            )
            .await
            .is_err()
        );
        let prepare = || {
            s.claude_tool_probe_binding(
                &a.user,
                r.id.clone(),
                r.revision,
                model.clone(),
                Some("high".into()),
            )
        };
        let unreserved = prepare().await.unwrap();
        assert!(
            s.record_claude_tool_probe(&a.user, unreserved)
                .await
                .is_err()
        );
        let one_round = prepare().await.unwrap();
        s.reserve_claude_tool_probe(&a.user, &one_round, 1)
            .await
            .unwrap();
        assert!(
            s.record_claude_tool_probe(&a.user, one_round)
                .await
                .is_err()
        );
        let proof = prepare().await.unwrap();
        assert!(
            s.reserve_claude_tool_probe(&a.user, &proof, 2)
                .await
                .is_err()
        );
        assert!(
            s.reserve_claude_tool_probe(&b.user, &proof, 1)
                .await
                .is_err()
        );
        s.reserve_claude_tool_probe(&a.user, &proof, 1)
            .await
            .unwrap();
        assert!(
            s.reserve_claude_tool_probe(&a.user, &proof, 1)
                .await
                .is_err()
        );
        s.reserve_claude_tool_probe(&a.user, &proof, 2)
            .await
            .unwrap();
        assert!(
            s.reserve_claude_tool_probe(&a.user, &proof, 2)
                .await
                .is_err()
        );
        assert!(
            s.record_claude_tool_probe(&a.user, proof)
                .await
                .unwrap()
                .function_tools
        );
        let choices = s.model_settings(&a.user).await.unwrap().choices;
        assert!(
            !choices
                .iter()
                .find(|c| c.provider_id == r.id && c.effort.is_none())
                .unwrap()
                .function_tools
        );
        assert!(
            choices
                .iter()
                .find(|c| c.provider_id == r.id && c.effort.as_deref() == Some("high"))
                .unwrap()
                .function_tools
        );
        let pending = prepare().await.unwrap();
        s.reserve_claude_tool_probe(&a.user, &pending, 1)
            .await
            .unwrap();
        s.disconnect_claude(&a.user, r.id.clone(), r.revision)
            .await
            .unwrap();
        assert!(
            s.reserve_claude_tool_probe(&a.user, &pending, 2)
                .await
                .is_err()
        );
        assert!(s.record_claude_tool_probe(&a.user, pending).await.is_err());
    }

    #[tokio::test]
    async fn host_proof_admits_new_missions_and_preserves_legacy_runs() {
        let _g = TEST_LOCK.lock().await;
        let d = directory();
        let s = Store::open(d.path()).unwrap();
        let a = admin(&s).await;
        let r = s
            .create_claude_binding(&a.user, "Claude".into(), "a".repeat(64), "b".repeat(64))
            .await
            .unwrap();
        let model = "claude-fixture-v1".to_owned();
        s.record_claude_probe(&a.user, r.clone(), model.clone())
            .await
            .unwrap();
        let selection = ModelSelection {
            source: ModelSource::PersonalClaudeCode,
            provider_id: r.id.clone(),
            provider_revision: r.revision,
            model: model.clone(),
            effort: None,
        };
        s.set_model_preference(&a.user, Some(selection))
            .await
            .unwrap();
        let thread = s
            .create_thread(&a.user, "Legacy text".into())
            .await
            .unwrap();
        let old = s
            .submit_turn(
                &a.user,
                thread.id.clone(),
                Uuid::new_v4().to_string(),
                "Synthetic text".into(),
            )
            .await
            .unwrap();
        let proof = s
            .claude_tool_probe_binding(&a.user, r.id.clone(), r.revision, model, None)
            .await
            .unwrap();
        s.reserve_claude_tool_probe(&a.user, &proof, 1)
            .await
            .unwrap();
        s.reserve_claude_tool_probe(&a.user, &proof, 2)
            .await
            .unwrap();
        let choice = s.record_claude_tool_probe(&a.user, proof).await.unwrap();
        let fresh = s
            .create_thread(&a.user, "Fresh tools".into())
            .await
            .unwrap();
        let new = s
            .submit_turn(
                &a.user,
                fresh.id,
                Uuid::new_v4().to_string(),
                "Synthetic follow-up".into(),
            )
            .await
            .unwrap();
        let u = a.user.clone();
        let run_id = new.run.id.clone();
        s.call(move |db| {
            let old = crate::chat::load_run(&db.conn, &u.tenant.0, &old.run.id)?;
            let new = crate::chat::load_run(&db.conn, &u.tenant.0, &run_id)?;
            assert!(!old.model.function_tools);
            assert!(old.tool_profile.is_none());
            assert!(binding(&db.conn, &u.tenant.0, &u.account.id, &old.model).is_ok());
            assert!(new.model.function_tools);
            assert!(new.tool_profile.is_some());
            assert!(new.mission_commands_enabled);
            assert!(crate::missions::chat::commands_allowed(&new));
            assert!(!crate::missions::chat::commands_allowed(&old));
            let mut legacy = crate::chat::load_run(&db.conn, &u.tenant.0, &run_id)?;
            legacy.mission_commands_enabled = false;
            assert!(!crate::missions::chat::commands_allowed(&legacy));
            assert!(
                crate::missions::require_model(
                    &db.conn,
                    &db.provider_key,
                    &u.tenant.0,
                    &u.account.id,
                    &choice
                )
                .is_ok()
            );
            assert!(
                crate::providers::binding_for_choice(&db.conn, &db.provider_key, &choice).is_err()
            );
            // Revoking only the tool proof fails tool runs closed; text runs retain text eligibility.
            db.conn.execute("DELETE FROM claude_code_tool_probes", [])?;
            assert!(binding(&db.conn, &u.tenant.0, &u.account.id, &new.model).is_err());
            assert!(binding(&db.conn, &u.tenant.0, &u.account.id, &old.model).is_ok());
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn model_efforts_require_exact_owner_binding_and_proof_and_pin_saved_runs() {
        let _g = TEST_LOCK.lock().await;
        let d = directory();
        let s = Store::open(d.path()).unwrap();
        let a = admin(&s).await;
        let b = member(&s, &a.user, "effort-member@example.test").await;
        let r = s
            .create_claude_binding(&a.user, "Claude".into(), "a".repeat(64), "b".repeat(64))
            .await
            .unwrap();
        let model = "claude-fixture-v1".to_owned();
        assert!(
            s.record_claude_model_efforts(&a.user, r.clone(), model.clone(), vec!["high".into()])
                .await
                .is_err()
        );
        s.record_claude_probe(&a.user, r.clone(), model.clone())
            .await
            .unwrap();
        let mut selection = ModelSelection {
            source: ModelSource::PersonalClaudeCode,
            provider_id: r.id.clone(),
            provider_revision: r.revision,
            model: model.clone(),
            effort: Some("xhigh".into()),
        };
        assert!(
            s.set_model_preference(&a.user, Some(selection.clone()))
                .await
                .is_err()
        );
        assert!(
            s.claude_model_efforts_known(&a.user, &r)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            s.record_claude_model_efforts(&b.user, r.clone(), model.clone(), vec!["high".into()])
                .await
                .is_err()
        );
        let mut wrong = r.clone();
        wrong.environment_id = Uuid::new_v4().to_string();
        assert!(
            s.record_claude_model_efforts(&a.user, wrong, model.clone(), vec!["high".into()])
                .await
                .is_err()
        );
        for invalid in ["max", "", "HIGH", "unknown"] {
            assert!(
                s.record_claude_model_efforts(
                    &a.user,
                    r.clone(),
                    model.clone(),
                    vec![invalid.into()]
                )
                .await
                .is_err()
            );
        }
        s.record_claude_model_efforts(
            &a.user,
            r.clone(),
            model.clone(),
            vec!["high".into(), "xhigh".into()],
        )
        .await
        .unwrap();
        assert_eq!(
            s.claude_model_efforts_known(&a.user, &r).await.unwrap(),
            vec![model.clone()]
        );
        let other_model = "claude-other-fixture".to_owned();
        s.record_claude_probe(&a.user, r.clone(), other_model.clone())
            .await
            .unwrap();
        s.record_claude_model_efforts(&a.user, r.clone(), other_model.clone(), vec![])
            .await
            .unwrap();
        let other = s
            .create_claude_binding(
                &a.user,
                "Other Claude".into(),
                "a".repeat(64),
                "b".repeat(64),
            )
            .await
            .unwrap();
        s.record_claude_probe(&a.user, other.clone(), model.clone())
            .await
            .unwrap();
        let settings = s.model_settings(&a.user).await.unwrap();
        let values = settings
            .choices
            .iter()
            .filter(|c| c.provider_id == r.id && c.model == model)
            .map(|c| c.effort.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            values,
            vec![None, Some("high".into()), Some("xhigh".into())]
        );
        assert!(
            settings
                .choices
                .iter()
                .filter(|c| c.provider_id == other.id || c.model == other_model)
                .all(|c| c.effort.is_none())
        );
        s.set_model_preference(&a.user, Some(selection.clone()))
            .await
            .unwrap();
        let thread = s
            .create_thread(&a.user, "Pinned effort".into())
            .await
            .unwrap();
        s.submit_turn(
            &a.user,
            thread.id,
            Uuid::new_v4().to_string(),
            "Synthetic prompt".into(),
        )
        .await
        .unwrap();
        selection.effort = None;
        s.set_model_preference(&a.user, Some(selection.clone()))
            .await
            .unwrap();
        let run = s.claim_run().await.unwrap().unwrap();
        assert_eq!(
            s.claude_run_selection(&run.lease)
                .await
                .unwrap()
                .effort
                .as_deref(),
            Some("xhigh")
        );
        assert_eq!(s.resolve_model(&a.user).await.unwrap().effort, None);
        // Catalog withdrawal makes the saved choice unavailable, never downgrades it.
        s.record_claude_model_efforts(&a.user, r.clone(), model.clone(), vec![])
            .await
            .unwrap();
        assert!(s.claude_run_selection(&run.lease).await.is_err());
        assert_eq!(s.resolve_model(&a.user).await.unwrap().effort, None);
        s.disconnect_claude(&a.user, r.id.clone(), r.revision)
            .await
            .unwrap();
        assert!(
            s.record_claude_model_efforts(&a.user, r.clone(), model, vec![])
                .await
                .is_err()
        );
        assert!(s.claude_model_efforts_known(&a.user, &r).await.is_err());
    }

    #[tokio::test]
    async fn schema75_default_survives_upgrade_and_metadata_backfill_persists_effort() {
        let _g = TEST_LOCK.lock().await;
        let d = directory();
        let s = Store::open(d.path()).unwrap();
        let a = admin(&s).await;
        let r = s
            .create_claude_binding(
                &a.user,
                "Legacy Claude".into(),
                "a".repeat(64),
                "b".repeat(64),
            )
            .await
            .unwrap();
        let model = "claude-legacy-fixture".to_owned();
        s.record_claude_probe(&a.user, r.clone(), model.clone())
            .await
            .unwrap();
        let mut selection = ModelSelection {
            source: ModelSource::PersonalClaudeCode,
            provider_id: r.id.clone(),
            provider_revision: r.revision,
            model: model.clone(),
            effort: None,
        };
        s.set_model_preference(&a.user, Some(selection.clone()))
            .await
            .unwrap();
        s.call(|db| {
            crate::schema::tests::replace_with_predecessor(&mut db.conn, 75);
            Ok(())
        })
        .await
        .unwrap();
        drop(s);
        let s = Store::open(d.path()).unwrap();
        assert_eq!(
            s.model_settings(&a.user).await.unwrap().selection,
            Some(selection.clone())
        );
        assert_eq!(s.resolve_model(&a.user).await.unwrap().effort, None);
        assert!(
            s.claude_model_efforts_known(&a.user, &r)
                .await
                .unwrap()
                .is_empty()
        );
        s.record_claude_model_efforts(&a.user, r.clone(), model.clone(), vec!["low".into()])
            .await
            .unwrap();
        selection.effort = Some("low".into());
        s.set_model_preference(&a.user, Some(selection.clone()))
            .await
            .unwrap();
        drop(s);
        let s = Store::open(d.path()).unwrap();
        assert_eq!(
            s.model_settings(&a.user).await.unwrap().selection,
            Some(selection)
        );
        assert_eq!(
            s.resolve_model(&a.user).await.unwrap().effort.as_deref(),
            Some("low")
        );
        assert_eq!(
            s.claude_model_efforts_known(&a.user, &r).await.unwrap(),
            vec![model]
        );
    }

    #[tokio::test]
    async fn owner_probe_selection_revision_and_no_shared_fallback() {
        let _g = TEST_LOCK.lock().await;
        let d = directory();
        let s = Store::open(d.path()).unwrap();
        let a = admin(&s).await;
        let b = member(&s, &a.user, "member-claude@example.test").await;
        let r = s
            .create_claude_binding(&a.user, "My Claude".into(), "a".repeat(64), "b".repeat(64))
            .await
            .unwrap();
        assert!(s.claude_launch_ref(&b.user, r.id.clone(), 1).await.is_err());
        let choice = s
            .record_claude_probe(&a.user, r.clone(), "claude-fixture-v1".into())
            .await
            .unwrap();
        assert!(!choice.function_tools);
        assert_eq!(choice.effort, None);
        let selection = ModelSelection {
            source: ModelSource::PersonalClaudeCode,
            provider_id: r.id.clone(),
            provider_revision: 1,
            model: choice.model,
            effort: None,
        };
        assert!(
            s.set_model_preference(&b.user, Some(selection.clone()))
                .await
                .is_err()
        );
        s.set_model_preference(&a.user, Some(selection))
            .await
            .unwrap();
        assert!(
            s.resolve_model(&a.user)
                .await
                .unwrap()
                .claude_code
                .is_some()
        );
        assert_eq!(
            s.model_settings(&a.user)
                .await
                .unwrap()
                .effective
                .unwrap()
                .source,
            ModelSource::PersonalClaudeCode
        );
        s.disconnect_claude(&a.user, r.id.clone(), 1).await.unwrap();
        assert!(s.resolve_model(&a.user).await.is_err());
        assert!(s.model_settings(&a.user).await.unwrap().effective.is_none());
        assert!(
            s.record_claude_probe(&a.user, r, "claude-fixture-v1".into())
                .await
                .is_err()
        );
    }
}
#[cfg(test)]
mod deletion_tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory, member};
    #[tokio::test]
    async fn account_deletion_keeps_local_cleanup_pending_until_owned_environment_ack() {
        let _g = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let a = admin(&store).await;
        let b = member(&store, &a.user, "claude-delete@example.test").await;
        let r = store
            .create_claude_binding(
                &b.user,
                "Private CLI".into(),
                "a".repeat(64),
                "b".repeat(64),
            )
            .await
            .unwrap();
        let revision = store
            .managed_accounts(&a.user)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.user.id == b.user.account.id)
            .unwrap()
            .revision;
        let prepared = store
            .prepare_account_management(
                &a.user,
                b.user.account.id.clone(),
                moosuri_core::AccountManagementInput {
                    request_id: Uuid::new_v4().to_string(),
                    expected_revision: revision,
                    action: moosuri_core::AccountManagementAction::Delete,
                },
            )
            .await
            .unwrap();
        store.apply_account_management(prepared).await.unwrap();
        assert!(
            store
                .claude_launch_ref(&b.user, r.id.clone(), r.revision)
                .await
                .is_err()
        );
        let account = store
            .managed_accounts(&a.user)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.user.id == b.user.account.id)
            .unwrap();
        assert!(!account.deletion.unwrap().local_complete);
        assert_eq!(
            store.claude_retirements("b".repeat(64)).await.unwrap(),
            vec![r.environment_id.clone()]
        );
        assert!(
            store
                .acknowledge_claude_retirement(r.environment_id.clone(), "c".repeat(64))
                .await
                .is_err()
        );
        store
            .acknowledge_claude_retirement(r.environment_id, "b".repeat(64))
            .await
            .unwrap();
        let account = store
            .managed_accounts(&a.user)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.user.id == b.user.account.id)
            .unwrap();
        assert!(account.deletion.unwrap().local_complete);
    }
}
