//! Exact owner-bound image proofs. Neither challenge pixels nor model output persist.
use super::*;
use moosuri_core::{ModelChoice, ModelSelection, ModelSource};
const HARNESS: &str = "subscription-image-tiles-v1";
fn source(s: ModelSource) -> Result<&'static str> {
    match s {
        ModelSource::PersonalClaudeCode => Ok("claude"),
        ModelSource::PersonalChatGpt => Ok("chatgpt"),
        _ => Err(StoreError::Conflict),
    }
}
pub(crate) fn proven(
    c: &Connection,
    tenant: &str,
    account: &str,
    choice: &ModelChoice,
) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM personal_subscription_vision_probes WHERE source=?1 AND tenant_id=?2 AND account_id=?3 AND binding_id=?4 AND revision=?5 AND model=?6 AND effort=?7 AND harness=?8)", params![source(choice.source)?,tenant,account,choice.provider_id,choice.provider_revision,choice.model,choice.effort.as_deref().unwrap_or(""),HARNESS], |r|r.get(0))?)
}
fn choice(c: &Connection, u: &UserContext, s: &ModelSelection) -> Result<ModelChoice> {
    let sql = match s.source {
        ModelSource::PersonalClaudeCode => {
            "SELECT EXISTS(SELECT 1 FROM claude_code_bindings WHERE tenant_id=?1 AND account_id=?2 AND id=?3)"
        }
        ModelSource::PersonalChatGpt => {
            "SELECT EXISTS(SELECT 1 FROM chatgpt_bindings WHERE tenant_id=?1 AND account_id=?2 AND id=?3)"
        }
        _ => return Err(StoreError::Conflict),
    };
    let owned: bool = c.query_row(sql, params![u.tenant.0, u.account.id, s.provider_id], |r| {
        r.get(0)
    })?;
    if !owned {
        return Err(StoreError::NotFound);
    }
    match s.source {
        ModelSource::PersonalClaudeCode => crate::claude_execution::verified(c, u, s),
        ModelSource::PersonalChatGpt => crate::subscriptions::verified(c, u, s),
        _ => Err(StoreError::Conflict),
    }
}
fn binding(c: &Connection, u: &UserContext, v: &ModelChoice) -> Result<providers::ProviderBinding> {
    match v.source {
        ModelSource::PersonalClaudeCode => {
            crate::claude_execution::binding(c, &u.tenant.0, &u.account.id, v)
        }
        ModelSource::PersonalChatGpt => {
            crate::subscriptions::binding(c, &u.tenant.0, &u.account.id, v)
        }
        _ => Err(StoreError::Conflict),
    }
}
/// Opaque frozen authority; API callers cannot construct an admission receipt.
pub struct PersonalVisionProbe {
    pub binding: providers::ProviderBinding,
    user: UserContext,
    selection: ModelSelection,
    id: String,
    environment: Option<String>,
}
fn environment(b: &providers::ProviderBinding) -> Result<Option<String>> {
    b.claude_code
        .as_ref()
        .map(|r| serde_json::to_string(r).map_err(|_| StoreError::Corrupt))
        .transpose()
}
fn current(c: &Connection, u: &UserContext, p: &PersonalVisionProbe) -> Result<ModelChoice> {
    let u = validate(c, u, true, false)?;
    validate(c, &p.user, true, false)?;
    if u.tenant.0 != p.user.tenant.0
        || u.account.id != p.user.account.id
        || u.digest != p.user.digest
        || u.epoch != p.user.epoch
    {
        return Err(StoreError::NotFound);
    }
    let v = choice(c, &u, &p.selection)?;
    if environment(&binding(c, &u, &v)?)? != p.environment {
        return Err(StoreError::Conflict);
    }
    Ok(v)
}
impl Store {
    pub async fn personal_vision_proven(
        &self,
        u: &UserContext,
        selection: &ModelSelection,
    ) -> Result<bool> {
        let (u, s) = (u.clone(), selection.clone());
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            let v = choice(&db.conn, &u, &s)?;
            binding(&db.conn, &u, &v)?;
            proven(&db.conn, &u.tenant.0, &u.account.id, &v)
        })
        .await
    }
    pub async fn personal_vision_probe_binding(
        &self,
        u: &UserContext,
        selection: ModelSelection,
    ) -> Result<PersonalVisionProbe> {
        let u = u.clone();
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u=validate(&tx,&u,true,false)?;
            let v=choice(&tx,&u,&selection)?;
            let b=binding(&tx,&u,&v)?;
            let env=environment(&b)?;
            let at=now()?;
            tx.execute("DELETE FROM personal_subscription_vision_attempts WHERE expires_at<=?1",[at])?;
            let pending:i64=tx.query_row("SELECT count(*) FROM personal_subscription_vision_attempts WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|r.get(0))?;
            if pending>=8 {return Err(StoreError::Conflict);}
            let id=Uuid::new_v4().to_string();
            tx.execute("INSERT INTO personal_subscription_vision_attempts VALUES(?1,?2,?3,?4,0,?5)",params![source(selection.source)?,u.tenant.0,u.account.id,id,at+120])?;
            tx.commit()?;
            Ok(PersonalVisionProbe{binding:b,user:u,selection,id,environment:env})
        }).await
    }
    /// Consumes the one inference reservation before any transport launch.
    pub async fn reserve_personal_vision_probe(
        &self,
        u: &UserContext,
        p: &PersonalVisionProbe,
    ) -> Result<()> {
        let (u, p) = (u.clone(), p.frozen());
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            current(&tx,&u,&p)?;
            let changed=tx.execute("UPDATE personal_subscription_vision_attempts SET reserved=1 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND reserved=0 AND expires_at>?4",params![u.tenant.0,u.account.id,p.id,now()?])?;
            if changed!=1{return Err(StoreError::Conflict);}
            crate::recovery_budget::reserve(&tx,&u.tenant.0,&u.account.id,&p.id,"probe",0)?;
            tx.commit()?;
            Ok(())
        }).await
    }
    pub async fn validate_personal_vision_probe(
        &self,
        u: &UserContext,
        p: &PersonalVisionProbe,
    ) -> Result<()> {
        let (u, p) = (u.clone(), p.frozen());
        self.call(move |db| current(&db.conn, &u, &p).map(|_| ()))
            .await
    }
    /// Trusted caller checked actual image answer, model identity and complete finish.
    pub async fn record_personal_vision_probe(
        &self,
        u: &UserContext,
        p: PersonalVisionProbe,
    ) -> Result<ModelChoice> {
        let u = u.clone();
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let v=current(&tx,&u,&p)?;
            let consumed=tx.execute("DELETE FROM personal_subscription_vision_attempts WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND reserved=1 AND expires_at>?4",params![u.tenant.0,u.account.id,p.id,now()?])?;
            if consumed!=1{return Err(StoreError::Conflict);}
            tx.execute("INSERT INTO personal_subscription_vision_probes VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT DO UPDATE SET verified_at=excluded.verified_at",params![source(v.source)?,u.tenant.0,u.account.id,v.provider_id,v.provider_revision,v.model,v.effort.as_deref().unwrap_or(""),HARNESS,now()?])?;
            tx.commit()?;
            Ok(v)
        }).await
    }
}
impl PersonalVisionProbe {
    fn frozen(&self) -> Self {
        Self {
            binding: providers::ProviderBinding {
                claude_code: self.binding.claude_code.clone(),
                subscription: self.binding.subscription.clone(),
                id: self.binding.id.clone(),
                revision: self.binding.revision,
                config: self.binding.config.clone(),
                model: self.binding.model.clone(),
                effort: self.binding.effort.clone(),
                credential: None,
            },
            user: self.user.clone(),
            selection: self.selection.clone(),
            id: self.id.clone(),
            environment: self.environment.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
