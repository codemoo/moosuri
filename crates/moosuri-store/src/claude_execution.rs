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
fn reference(
    c: &Connection,
    tenant: &str,
    account: &str,
    id: &str,
    revision: i64,
) -> Result<ClaudeExecutionRef> {
    c.query_row("SELECT environment_id,profile_digest,root_id FROM claude_code_bindings WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND revision=?4 AND status IN('pending','active')",params![tenant,account,id,revision],|r|Ok(ClaudeExecutionRef{id:id.into(),revision,environment_id:r.get(0)?,profile_digest:r.get(1)?,root_id:r.get(2)?})).optional()?.ok_or(StoreError::NotFound)
}
pub(super) fn choices(c: &Connection, u: &UserContext) -> Result<Vec<ModelChoice>> {
    let mut q=c.prepare("SELECT b.id,b.revision,b.label,p.model,p.verified_at FROM claude_code_bindings b JOIN claude_code_probes p ON p.tenant_id=b.tenant_id AND p.account_id=b.account_id AND p.binding_id=b.id AND p.revision=b.revision WHERE b.tenant_id=?1 AND b.account_id=?2 AND b.status='active' ORDER BY b.label,p.model")?;
    q.query_map(params![u.tenant.0, u.account.id], |r| {
        Ok(ModelChoice {
            source: ModelSource::PersonalClaudeCode,
            provider_id: r.get(0)?,
            provider_revision: r.get(1)?,
            provider_name: r.get(2)?,
            kind: ProviderKind::Anthropic,
            model: r.get(3)?,
            effort: None,
            verified_at: r.get(4)?,
            function_tools: false,
        })
    })?
    .collect::<std::result::Result<Vec<_>, _>>()
    .map_err(Into::into)
}
pub(super) fn selected(c: &Connection, u: &UserContext) -> Result<Option<ModelSelection>> {
    c.query_row("SELECT binding_id,revision,model FROM personal_claude_selections WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|Ok(ModelSelection{source:ModelSource::PersonalClaudeCode,provider_id:r.get(0)?,provider_revision:r.get(1)?,model:r.get(2)?,effort:None})).optional().map_err(Into::into)
}
pub(super) fn verified(c: &Connection, u: &UserContext, s: &ModelSelection) -> Result<ModelChoice> {
    if s.source != ModelSource::PersonalClaudeCode || s.effort.is_some() {
        return Err(StoreError::Conflict);
    }
    choices(c, u)?
        .into_iter()
        .find(|v| {
            v.provider_id == s.provider_id
                && v.provider_revision == s.provider_revision
                && v.model == s.model
        })
        .ok_or(StoreError::Conflict)
}
pub(super) fn binding(
    c: &Connection,
    tenant: &str,
    account: &str,
    v: &ModelChoice,
) -> Result<providers::ProviderBinding> {
    if v.source != ModelSource::PersonalClaudeCode || v.function_tools || v.effort.is_some() {
        return Err(StoreError::Conflict);
    }
    let ok:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM claude_code_bindings b JOIN claude_code_probes p ON p.tenant_id=b.tenant_id AND p.account_id=b.account_id AND p.binding_id=b.id AND p.revision=b.revision WHERE b.tenant_id=?1 AND b.account_id=?2 AND b.id=?3 AND b.revision=?4 AND b.status='active' AND p.model=?5)",params![tenant,account,v.provider_id,v.provider_revision,v.model],|r|r.get(0))?;
    if !ok {
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
        effort: None,
        credential: None,
    })
}
impl Store {
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
        let l = l.clone();
        self.call(move |db| {
            let r = crate::chat::guard(&db.conn, &db.provider_key, &l)?;
            if r.model.source != ModelSource::PersonalClaudeCode {
                return Err(StoreError::Conflict);
            }
            binding(&db.conn, &r.tenant, &r.account, &r.model)?;
            Ok(r.model.model)
        })
        .await
    }

    pub async fn claude_bindings(&self, u: &UserContext) -> Result<Vec<ClaudeBindingSummary>> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;let mut q=db.conn.prepare("SELECT id,revision,label,status,coalesce((SELECT state FROM claude_code_retirements r WHERE r.environment_id=claude_code_bindings.environment_id),'not_requested') FROM claude_code_bindings WHERE tenant_id=?1 AND account_id=?2 ORDER BY label,id")?;q.query_map(params![u.tenant.0,u.account.id],|r|Ok(ClaudeBindingSummary{id:r.get(0)?,revision:r.get(1)?,label:r.get(2)?,status:r.get(3)?,cleanup_state:r.get(4)?}))?.collect::<std::result::Result<Vec<_>,_>>().map_err(Into::into)}).await
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
    pub async fn record_claude_probe(
        &self,
        u: &UserContext,
        r: ClaudeExecutionRef,
        model: String,
    ) -> Result<ModelChoice> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let current=reference(&tx,&u.tenant.0,&u.account.id,&r.id,r.revision)?;if current.root_id!=r.root_id||current.profile_digest!=r.profile_digest||current.environment_id!=r.environment_id||model.len()>200||!model.starts_with("claude-")||!model.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'.'){return Err(StoreError::Conflict)}let n:i64=tx.query_row("SELECT count(*) FROM claude_code_probes WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3",params![u.tenant.0,u.account.id,r.id],|r|r.get(0))?;if n>=32{return Err(StoreError::Conflict)}tx.execute("INSERT INTO claude_code_probes VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT DO UPDATE SET verified_at=excluded.verified_at",params![u.tenant.0,u.account.id,r.id,r.revision,model,now()?])?;tx.execute("UPDATE claude_code_bindings SET status='active' WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,r.id])?;let v=verified(&tx,&u,&ModelSelection{source:ModelSource::PersonalClaudeCode,provider_id:r.id,provider_revision:r.revision,model,effort:None})?;tx.commit()?;Ok(v)}).await
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
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;let n=db.conn.execute("UPDATE claude_code_bindings SET status='disconnected',revision=revision+1 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND revision=?4 AND status!='disconnected'",params![u.tenant.0,u.account.id,id,revision])?;if n!=1{return Err(StoreError::NotFound)}// Selection stays pinned/unavailable; no fallback.
 Ok(())}).await
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory, member};
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
