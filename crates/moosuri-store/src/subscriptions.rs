//! Personal ChatGPT storage. Network IO never runs on the SQLite writer.
mod handoff;
use super::*;
pub use handoff::*;
use moosuri_connectors::chatgpt_oauth::{
    IssuedRegistration, TokenBundle, VerifiedIdentity, sealed::SessionSealer,
};
use moosuri_core::{ModelChoice, ModelSelection, ModelSource, ProviderKind};
#[derive(Clone)]
pub struct SubscriptionRef {
    pub tenant: String,
    pub account: String,
    pub id: String,
    pub revision: i64,
    pub model: String,
    pub effort: Option<String>,
}
#[derive(Clone)]
pub enum SubscriptionAuthority {
    User(UserContext),
    Run(RunLease),
}
pub struct SubscriptionLogin {
    pub attempt_id: String,
    pub binding: SubscriptionRef,
    pub host_id: String,
    pub identity: Option<VerifiedIdentity>,
    pub issued: Option<IssuedRegistration>,
    user: UserContext,
}
pub struct SubscriptionRefresh {
    binding: SubscriptionRef,
    authority: SubscriptionAuthority,
    generation: i64,
    nonce: String,
}
pub enum SubscriptionAccess {
    Ready(Zeroizing<String>),
    Refresh(Box<SubscriptionRefresh>, Box<TokenBundle>),
    Wait,
}
#[derive(Clone, serde::Serialize)]
pub struct SubscriptionSummary {
    pub id: String,
    pub revision: i64,
    pub label: String,
    pub status: String,
    pub remote_revoke: String,
    pub handoff_phase: Option<String>,
}
// TS is exported explicitly from core DTOs, not from private store carriers.
fn sealer(key: &[u8; 32]) -> SessionSealer {
    SessionSealer::new(Zeroizing::new(*key))
}
fn context(c: &Connection, b: &SubscriptionRef, g: Option<i64>) -> Result<Vec<u8>> {
    let runtime: String = c.query_row(
        "SELECT runtime_id FROM subscription_runtime WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    serde_json::to_vec(&(
        "moosuri/chatgpt/v1",
        runtime,
        &b.tenant,
        &b.account,
        &b.id,
        g.map(|_| b.revision),
        g,
    ))
    .map_err(|_| StoreError::Corrupt)
}
fn owned(c: &Connection, b: &SubscriptionRef) -> Result<String> {
    c.query_row("SELECT status FROM chatgpt_bindings WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND current_revision=?4",params![b.tenant,b.account,b.id,b.revision],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound)
}
fn authorize(
    c: &Connection,
    key: &[u8; 32],
    a: &SubscriptionAuthority,
    b: &SubscriptionRef,
) -> Result<i64> {
    match a {
        SubscriptionAuthority::User(u) => {
            let u = validate(c, u, true, false)?;
            if u.tenant.0 != b.tenant || u.account.id != b.account {
                return Err(StoreError::NotFound);
            }
            Ok(u.epoch)
        }
        SubscriptionAuthority::Run(l) => {
            let r = chat::guard(c, key, l)?;
            if r.tenant != b.tenant
                || r.account != b.account
                || r.model.source != ModelSource::PersonalChatGpt
                || r.model.provider_id != b.id
                || r.model.provider_revision != b.revision
            {
                return Err(StoreError::Unauthorized);
            }
            Ok(r.epoch)
        }
    }
}
pub(super) fn initialize(c: &Connection) -> Result<()> {
    handoff::expire(c)?;
    if c.query_row("SELECT count(*) FROM subscription_runtime", [], |r| {
        r.get::<_, i64>(0)
    })? == 0
    {
        c.execute(
            "INSERT INTO subscription_runtime VALUES(1,?1,?2)",
            params![
                Uuid::new_v4().to_string(),
                format!("urn:uuid:{}", Uuid::new_v4())
            ],
        )?;
    }
    c.execute_batch("UPDATE chatgpt_auth_attempts SET phase='abandoned' WHERE phase IN('pending','issued'); UPDATE chatgpt_bindings SET status='needs_reauth' WHERE status='connecting'; UPDATE chatgpt_sessions SET refresh_state='rotation_unknown',cipher=NULL WHERE refresh_state='dispatched'; UPDATE chatgpt_bindings SET status='needs_reauth' WHERE EXISTS(SELECT 1 FROM chatgpt_sessions s WHERE s.tenant_id=chatgpt_bindings.tenant_id AND s.account_id=chatgpt_bindings.account_id AND s.binding_id=chatgpt_bindings.id AND s.refresh_state='rotation_unknown'); DELETE FROM chatgpt_sessions WHERE EXISTS(SELECT 1 FROM chatgpt_bindings b WHERE b.tenant_id=chatgpt_sessions.tenant_id AND b.account_id=chatgpt_sessions.account_id AND b.id=chatgpt_sessions.binding_id AND b.status='revoking'); UPDATE chatgpt_bindings SET status='disconnected',remote_revoke='unconfirmed' WHERE status='revoking'; UPDATE chatgpt_sessions SET refresh_state='idle',refresh_nonce=NULL WHERE refresh_state='acquired';")?;
    Ok(())
}
pub(super) fn choices(c: &Connection, u: &UserContext) -> Result<Vec<ModelChoice>> {
    let mut q=c.prepare("SELECT b.id,b.current_revision,r.label,p.model,p.effort,p.verified_at,p.function_tools FROM chatgpt_bindings b JOIN chatgpt_binding_revisions r ON r.tenant_id=b.tenant_id AND r.account_id=b.account_id AND r.binding_id=b.id AND r.revision=b.current_revision JOIN chatgpt_model_probes p ON p.tenant_id=b.tenant_id AND p.account_id=b.account_id AND p.binding_id=b.id AND p.revision=b.current_revision JOIN chatgpt_sessions s ON s.tenant_id=b.tenant_id AND s.account_id=b.account_id AND s.binding_id=b.id AND s.revision=b.current_revision WHERE b.tenant_id=?1 AND b.account_id=?2 AND b.status='active' AND s.cipher IS NOT NULL AND s.plan_enabled=1 AND s.refresh_state!='rotation_unknown' ORDER BY r.label,p.model,p.effort")?;
    let rows = q
        .query_map(params![u.tenant.0, u.account.id], |r| {
            let e: String = r.get(4)?;
            Ok(ModelChoice {
                source: ModelSource::PersonalChatGpt,
                provider_id: r.get(0)?,
                provider_revision: r.get(1)?,
                provider_name: r.get(2)?,
                kind: ProviderKind::OpenAi,
                model: r.get(3)?,
                effort: (!e.is_empty()).then_some(e),
                verified_at: r.get(5)?,
                function_tools: r.get(6)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}
pub(super) fn selected(c: &Connection, u: &UserContext) -> Result<Option<ModelSelection>> {
    c.query_row("SELECT binding_id,revision,model,effort FROM personal_subscription_selections WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|{
        let e:String=r.get(3)?;
        Ok(ModelSelection{
            source:ModelSource::PersonalChatGpt,provider_id:r.get(0)?,provider_revision:r.get(1)?,model:r.get(2)?,effort:(!e.is_empty()).then_some(e)}
        )}
    ).optional().map_err(Into::into)
}
pub(super) fn verified(c: &Connection, u: &UserContext, s: &ModelSelection) -> Result<ModelChoice> {
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
    choice: &ModelChoice,
) -> Result<providers::ProviderBinding> {
    let b = SubscriptionRef {
        tenant: tenant.into(),
        account: account.into(),
        id: choice.provider_id.clone(),
        revision: choice.provider_revision,
        model: choice.model.clone(),
        effort: choice.effort.clone(),
    };
    if owned(c, &b)? != "active" {
        return Err(StoreError::Conflict);
    }
    let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM chatgpt_model_probes p JOIN chatgpt_sessions s ON s.tenant_id=p.tenant_id AND s.account_id=p.account_id AND s.binding_id=p.binding_id AND s.revision=p.revision WHERE p.tenant_id=?1 AND p.account_id=?2 AND p.binding_id=?3 AND p.revision=?4 AND p.model=?5 AND p.effort=?6 AND s.cipher IS NOT NULL AND s.plan_enabled=1 AND s.refresh_state!='rotation_unknown' AND (?7=0 OR p.function_tools=1))",params![tenant,account,b.id,b.revision,b.model,b.effort.as_deref().unwrap_or(""),choice.function_tools],|r|r.get(0))?;
    if !valid {
        return Err(StoreError::Conflict);
    }
    Ok(providers::ProviderBinding {
        id: b.id.clone(),
        revision: b.revision,
        config: moosuri_core::ProviderConfig {
            kind: ProviderKind::OpenAi,
            base_url: None,
            allowed_ips: vec![],
            router_providers: vec![],
            zero_data_retention: false,
        },
        model: b.model.clone(),
        effort: b.effort.clone(),
        credential: None,
        subscription: Some(b),
    })
}
/// Both same-host OAuth and VM staging allocate above every retained reservation.
/// Only sealed verified history supplies subject authority; incomplete client IDs
/// remain non-authoritative metadata, retained for a fresh code attempt.
fn next_registration_revision(
    c: &Connection,
    key: &[u8; 32],
    u: &UserContext,
    id: &str,
) -> Result<(i64, Option<String>, Option<Vec<u8>>)> {
    let maximum:i64=c.query_row("SELECT COALESCE(MAX(revision),0) FROM chatgpt_binding_revisions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3",params![u.tenant.0,u.account.id,id],|r|r.get(0))?;
    let identity:Option<Vec<u8>>=c.query_row("SELECT identity FROM chatgpt_binding_revisions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND identity IS NOT NULL ORDER BY revision DESC LIMIT 1",params![u.tenant.0,u.account.id,id],|r|r.get(0)).optional()?;
    let client = if let Some(ref cipher) = identity {
        let b = SubscriptionRef {
            tenant: u.tenant.0.clone(),
            account: u.account.id.clone(),
            id: id.into(),
            revision: maximum,
            model: String::new(),
            effort: None,
        };
        Some(
            sealer(key)
                .open_identity(cipher, &context(c, &b, None)?)
                .map_err(|_| StoreError::Corrupt)?
                .client_id()
                .to_owned(),
        )
    } else {
        c.query_row("SELECT client_id FROM chatgpt_binding_revisions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND client_id IS NOT NULL ORDER BY revision DESC LIMIT 1",params![u.tenant.0,u.account.id,id],|r|r.get(0)).optional()?
    };
    Ok((
        maximum.checked_add(1).ok_or(StoreError::Conflict)?,
        client,
        identity,
    ))
}
impl Store {
    pub async fn subscriptions(&self, u: &UserContext) -> Result<Vec<SubscriptionSummary>> {
        let u = u.clone();
        self.call(move|db|{
            validate(&db.conn,&u,true,false)?;
            let mut q=db.conn.prepare("SELECT b.id,b.current_revision,r.label,b.status,b.remote_revoke,(SELECT h.phase FROM chatgpt_handoffs h WHERE h.tenant_id=b.tenant_id AND h.account_id=b.account_id AND h.binding_id=b.id ORDER BY h.issued_at DESC,h.rowid DESC LIMIT 1) FROM chatgpt_bindings b JOIN chatgpt_binding_revisions r ON r.tenant_id=b.tenant_id AND r.account_id=b.account_id AND r.binding_id=b.id AND r.revision=b.current_revision WHERE b.tenant_id=?1 AND b.account_id=?2 ORDER BY r.label,b.id")?;
            Ok(q.query_map(params![u.tenant.0,u.account.id],|r|Ok(SubscriptionSummary{
                id:r.get(0)?,revision:r.get(1)?,label:r.get(2)?,status:r.get(3)?,remote_revoke:r.get(4)?,handoff_phase:r.get(5)?}
            ))?.collect::<std::result::Result<Vec<_>,_>>()?) }
        ).await
    }
    pub async fn begin_subscription_login(
        &self,
        u: &UserContext,
        existing: Option<(String, i64)>,
        label: String,
    ) -> Result<SubscriptionLogin> {
        let u = u.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&u,true,false)?;
            let label=name_valid(label,100)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (runtime,host):(String,String)=tx.query_row("SELECT runtime_id,host_id FROM subscription_runtime WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let _=runtime;
            let (id,revision,client,identity)=if let Some((id,expected))=existing {
                let old=SubscriptionRef{
                    tenant:u.tenant.0.clone(),account:u.account.id.clone(),id:id.clone(),revision:expected,model:String::new(),effort:None}
                ;
                owned(&tx,&old)?;
                let (revision,client,identity)=next_registration_revision(&tx,&db.provider_key,&u,&id)?;
                (id,revision,client,identity)}
            else{
                let count:i64=tx.query_row("SELECT count(*) FROM chatgpt_bindings WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|r.get(0))?;
                if count>=16{
                    return Err(StoreError::Conflict)}
                (Uuid::new_v4().to_string(),1,None,None)}
            ;
            let b=SubscriptionRef{
                tenant:u.tenant.0.clone(),account:u.account.id.clone(),id:id.clone(),revision,model:String::new(),effort:None}
            ;
            let verified_identity=identity.as_ref().map(|v|sealer(&db.provider_key).open_identity(v,&context(&tx,&b,None)?).map_err(|_|StoreError::Corrupt)).transpose()?;
            let issued=if verified_identity.is_none(){
                client.clone().map(|v|IssuedRegistration::from_saved_metadata(v,host.clone()).map_err(|_|StoreError::Corrupt)).transpose()?}
            else{
                None}
            ;
            tx.execute("INSERT INTO chatgpt_bindings(tenant_id,account_id,id,current_revision,status) VALUES(?1,?2,?3,?4,'connecting') ON CONFLICT(tenant_id,account_id,id) DO UPDATE SET current_revision=excluded.current_revision,status='connecting',remote_revoke='not_requested'",params![u.tenant.0,u.account.id,id,revision])?;
            tx.execute("INSERT INTO chatgpt_binding_revisions VALUES(?1,?2,?3,?4,?5,?6,?7)",params![u.tenant.0,u.account.id,id,revision,label,client,identity])?;
            tx.execute("DELETE FROM chatgpt_sessions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3",params![u.tenant.0,u.account.id,id])?;
            let attempt=Uuid::new_v4().to_string();
            tx.execute("INSERT INTO chatgpt_auth_attempts VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'pending')",params![attempt,u.tenant.0,u.account.id,u.digest.as_slice(),u.epoch,id,revision,now()?+300])?;
            tx.commit()?;
            Ok(SubscriptionLogin{
                attempt_id:attempt,binding:b,host_id:host,identity:verified_identity,issued,user:u}
            )
        }
        ).await
    }
    pub async fn save_subscription_registration(
        &self,
        t: &SubscriptionLogin,
        r: &IssuedRegistration,
    ) -> Result<()> {
        let (u, a, b, client, host) = (
            t.user.clone(),
            t.attempt_id.clone(),
            t.binding.clone(),
            r.client_id().to_owned(),
            r.host_id().to_owned(),
        );
        self.call(move|db|{
            validate_attempt(&db.conn,&u,&a,&b,"pending")?;
            let expected:String=db.conn.query_row("SELECT host_id FROM subscription_runtime WHERE singleton=1",[],|r|r.get(0))?;
            if host!=expected{
                return Err(StoreError::Conflict)}
            let old:Option<String>=db.conn.query_row("SELECT client_id FROM chatgpt_binding_revisions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4",params![b.tenant,b.account,b.id,b.revision],|r|r.get(0))?;
            if old.as_ref().is_some_and(|v|v!=&client){
                return Err(StoreError::Conflict)}
            let tx=db.conn.transaction()?;
            tx.execute("UPDATE chatgpt_binding_revisions SET client_id=?5 WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4",params![b.tenant,b.account,b.id,b.revision,client])?;
            tx.execute("UPDATE chatgpt_auth_attempts SET phase='issued' WHERE id=?1",[a])?;
            tx.commit()?;
            Ok(())}
        ).await
    }
    pub async fn complete_subscription_login(
        &self,
        t: &SubscriptionLogin,
        bundle: TokenBundle,
    ) -> Result<()> {
        let (u, a, b) = (t.user.clone(), t.attempt_id.clone(), t.binding.clone());
        self.call(move|db|{
            validate_attempt(&db.conn,&u,&a,&b,"issued")?;
            let (client,previous):(String,Option<Vec<u8>>)=db.conn.query_row("SELECT client_id,identity FROM chatgpt_binding_revisions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4",params![b.tenant,b.account,b.id,b.revision],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let h:String=db.conn.query_row("SELECT host_id FROM subscription_runtime WHERE singleton=1",[],|r|r.get(0))?;
            if bundle.identity().client_id()!=client||bundle.identity().host_id()!=h{
                return Err(StoreError::Conflict)}
            if let Some(p)=previous{
                let old=sealer(&db.provider_key).open_identity(&p,&context(&db.conn,&b,None)?).map_err(|_|StoreError::Corrupt)?;
                if &old!=bundle.identity(){
                    return Err(StoreError::Conflict)}
            }
            let cipher=sealer(&db.provider_key).seal(&bundle,&context(&db.conn,&b,Some(1))?).map_err(|_|StoreError::Corrupt)?;
            let identity=sealer(&db.provider_key).seal_identity(bundle.identity(),&context(&db.conn,&b,None)?).map_err(|_|StoreError::Corrupt)?;
            let enabled=bundle.plan_grant()==moosuri_connectors::chatgpt_oauth::PlanGrant::Enabled;
            let tx=db.conn.transaction()?;
            tx.execute("INSERT INTO chatgpt_sessions(tenant_id,account_id,binding_id,revision,generation,cipher,expires_at,plan_enabled) VALUES(?1,?2,?3,?4,1,?5,?6,?7)",params![b.tenant,b.account,b.id,b.revision,cipher,bundle.expires_at_unix() as i64,enabled])?;
            tx.execute("UPDATE chatgpt_binding_revisions SET identity=?5 WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4",params![b.tenant,b.account,b.id,b.revision,identity])?;
            tx.execute("UPDATE chatgpt_bindings SET status=?4 WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![b.tenant,b.account,b.id,if enabled{
                "active"}
            else{
                "plan_disabled"}
            ])?;
            tx.execute("UPDATE chatgpt_auth_attempts SET phase='completed' WHERE id=?1",[a])?;
            tx.commit()?;
            Ok(())}
        ).await
    }
    pub async fn fail_subscription_login(&self, t: &SubscriptionLogin) -> Result<()> {
        let (a, b) = (t.attempt_id.clone(), t.binding.clone());
        self.call(move|db|{
            db.conn.execute("UPDATE chatgpt_auth_attempts SET phase='failed' WHERE id=?1 AND phase IN('pending','issued')",[a])?;
            db.conn.execute("UPDATE chatgpt_bindings SET status='needs_reauth' WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND current_revision=?4 AND status='connecting'",params![b.tenant,b.account,b.id,b.revision])?;
            Ok(())}
        ).await
    }
    pub async fn subscription_attempt(&self, u: &UserContext, id: String) -> Result<String> {
        let u = u.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&u,true,false)?;
            db.conn.query_row("SELECT phase FROM chatgpt_auth_attempts WHERE id=?1 AND tenant_id=?2 AND account_id=?3 AND session_digest=?4 AND auth_epoch=?5",params![id,u.tenant.0,u.account.id,u.digest.as_slice(),u.epoch],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound)}
        ).await
    }
    pub async fn subscription_ref(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
        model: String,
        effort: Option<String>,
    ) -> Result<SubscriptionRef> {
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            if model.len() > 256
                || model.is_empty()
                || effort.as_ref().is_some_and(|v| v.len() > 32)
            {
                return Err(StoreError::InvalidInput);
            }
            let b = SubscriptionRef {
                tenant: u.tenant.0,
                account: u.account.id,
                id,
                revision,
                model,
                effort,
            };
            if owned(&db.conn, &b)? != "active" {
                return Err(StoreError::Conflict);
            }
            Ok(b)
        })
        .await
    }
    pub async fn subscription_access(
        &self,
        a: SubscriptionAuthority,
        b: SubscriptionRef,
    ) -> Result<SubscriptionAccess> {
        self.call(move|db|{
            let epoch=authorize(&db.conn,&db.provider_key,&a,&b)?;
            if owned(&db.conn,&b)?!="active"{
                return Err(StoreError::Conflict)}
            let (g,cipher,expires,state,cooldown,until):(i64,Option<Vec<u8>>,i64,String,i64,Option<i64>)=db.conn.query_row("SELECT generation,cipher,expires_at,refresh_state,cooldown_until,refresh_until FROM chatgpt_sessions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4 AND plan_enabled=1",params![b.tenant,b.account,b.id,b.revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
            if state=="rotation_unknown"||state=="dispatched"{
                return if state=="dispatched"{
                    Ok(SubscriptionAccess::Wait)}
                else{
                    Err(StoreError::Conflict)}
            }
            let current=sealer(&db.provider_key).open(&cipher.ok_or(StoreError::Conflict)?,&context(&db.conn,&b,Some(g))?).map_err(|_|StoreError::Corrupt)?;
            if expires>now()?+60 {
                return current.plan_access_token().map(|v|SubscriptionAccess::Ready(Zeroizing::new(v.to_owned()))).map_err(|_|StoreError::Conflict)}
            if cooldown>now()? || state=="acquired"&&until.is_some_and(|v|v>now().unwrap_or(i64::MAX)){
                return Ok(SubscriptionAccess::Wait)}
            let n=Uuid::new_v4().to_string();
            let changed=db.conn.execute("UPDATE chatgpt_sessions SET refresh_state='acquired',refresh_nonce=?5,refresh_epoch=?6,refresh_until=?7 WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND generation=?4 AND refresh_state IN('idle','acquired')",params![b.tenant,b.account,b.id,g,n,epoch,now()?+45])?;
            if changed!=1{
                return Ok(SubscriptionAccess::Wait)}
            Ok(SubscriptionAccess::Refresh(Box::new(SubscriptionRefresh{
                binding:b,authority:a,generation:g,nonce:n}
            ), Box::new(current)))
        }
        ).await
    }
    pub async fn dispatch_subscription_refresh(&self, l: &SubscriptionRefresh) -> Result<()> {
        let (b, a, g, n) = (
            l.binding.clone(),
            l.authority.clone(),
            l.generation,
            l.nonce.clone(),
        );
        self.call(move|db|{
            let epoch=authorize(&db.conn,&db.provider_key,&a,&b)?;
            if owned(&db.conn,&b)?!="active"{
                return Err(StoreError::Conflict)}
            if db.conn.execute("UPDATE chatgpt_sessions SET refresh_state='dispatched' WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4 AND generation=?5 AND refresh_nonce=?6 AND refresh_state='acquired' AND refresh_epoch=?7 AND refresh_until>?8",params![b.tenant,b.account,b.id,b.revision,g,n,epoch,now()?])?!=1{
                return Err(StoreError::Conflict)}
            Ok(())}
        ).await
    }
    pub async fn complete_subscription_refresh(
        &self,
        l: SubscriptionRefresh,
        bundle: TokenBundle,
    ) -> Result<()> {
        self.call(move|db|{
            let b=&l.binding;
            let result=authorize(&db.conn,&db.provider_key,&l.authority,b);
            if result.is_err(){
                refresh_failure(&db.conn,&l,"rotation_unknown")?;
                return Err(StoreError::Unauthorized)}
            if owned(&db.conn,b)?!="active"{
                return Err(StoreError::Conflict)}
            let current:Option<Vec<u8>>=db.conn.query_row("SELECT cipher FROM chatgpt_sessions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND generation=?4",params![b.tenant,b.account,b.id,l.generation],|r|r.get(0)).optional()?.flatten();
            let old=sealer(&db.provider_key).open(&current.ok_or(StoreError::Conflict)?,&context(&db.conn,b,Some(l.generation))?).map_err(|_|StoreError::Corrupt)?;
            if old.identity()!=bundle.identity(){
                refresh_failure(&db.conn,&l,"rotation_unknown")?;
                return Err(StoreError::Conflict)}
            let next=l.generation.checked_add(1).ok_or(StoreError::Corrupt)?;
            let cipher=sealer(&db.provider_key).seal(&bundle,&context(&db.conn,b,Some(next))?).map_err(|_|StoreError::Corrupt)?;
            let enabled=bundle.plan_grant()==moosuri_connectors::chatgpt_oauth::PlanGrant::Enabled;
            let tx=db.conn.transaction()?;
            if tx.execute("UPDATE chatgpt_sessions SET generation=?7,cipher=?8,expires_at=?9,plan_enabled=?10,refresh_state='idle',refresh_nonce=NULL,refresh_until=NULL WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4 AND generation=?5 AND refresh_nonce=?6 AND refresh_state='dispatched'",params![b.tenant,b.account,b.id,b.revision,l.generation,l.nonce,next,cipher,bundle.expires_at_unix() as i64,enabled])?!=1{
                return Err(StoreError::Conflict)}
            if !enabled{
                tx.execute("UPDATE chatgpt_bindings SET status='plan_disabled' WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![b.tenant,b.account,b.id])?;
            }
            tx.commit()?;
            Ok(())}
        ).await
    }
    pub async fn fail_subscription_refresh(
        &self,
        l: SubscriptionRefresh,
        temporary: bool,
    ) -> Result<()> {
        self.call(move |db| {
            refresh_failure(
                &db.conn,
                &l,
                if temporary {
                    "idle"
                } else {
                    "rotation_unknown"
                },
            )
        })
        .await
    }
    pub async fn record_subscription_probe(
        &self,
        u: &UserContext,
        b: SubscriptionRef,
    ) -> Result<ModelChoice> {
        let u = u.clone();
        self.call(move|db|{
            authorize(&db.conn,&db.provider_key,&SubscriptionAuthority::User(u.clone()),&b)?;
            if owned(&db.conn,&b)?!="active"{
                return Err(StoreError::Conflict)}
            db.conn.execute("INSERT INTO chatgpt_model_probes(tenant_id,account_id,binding_id,revision,model,effort,verified_at) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT DO UPDATE SET verified_at=excluded.verified_at",params![b.tenant,b.account,b.id,b.revision,b.model,b.effort.as_deref().unwrap_or(""),now()?])?;
            verified(&db.conn,&u,&ModelSelection{
                source:ModelSource::PersonalChatGpt,provider_id:b.id,provider_revision:b.revision,model:b.model,effort:b.effort}
            )}
        ).await
    }
    /// Synthetic-only expiry control for an owner-authenticated loopback test.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    pub async fn expire_subscription_fixture(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move|db| {
            let u=validate(&db.conn,&u,true,false)?;
            let b=SubscriptionRef{tenant:u.tenant.0,account:u.account.id,id,revision,model:"fixture-model".into(),effort:None};
            if owned(&db.conn,&b)?!="active" {return Err(StoreError::Conflict)}
            db.conn.execute("UPDATE chatgpt_sessions SET expires_at=0 WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4",params![b.tenant,b.account,b.id,b.revision])?;
            Ok(())
        }).await
    }
    /// Owner-only synthetic tool proof admission. Catalog presence is insufficient.
    pub async fn subscription_tool_probe_binding(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
        model: String,
        effort: Option<String>,
    ) -> Result<(providers::ProviderBinding, String)> {
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            let reference = SubscriptionRef {
                tenant: u.tenant.0.clone(),
                account: u.account.id.clone(),
                id: id.clone(),
                revision,
                model: model.clone(),
                effort: effort.clone(),
            };
            // Match ordinary subscription routes: an unowned/unknown binding is
            // unavailable before examining capability metadata or credentials.
            if owned(&db.conn, &reference)? != "active" {
                return Err(StoreError::Conflict);
            }
            let choice = verified(
                &db.conn,
                &u,
                &ModelSelection {
                    source: ModelSource::PersonalChatGpt,
                    provider_id: id,
                    provider_revision: revision,
                    model,
                    effort,
                },
            )?;
            let b = binding(&db.conn, &u.tenant.0, &u.account.id, &choice)?;
            let nonce = random_token()?;
            Ok((b, format!("proof-{}", &nonce[..16])))
        })
        .await
    }
    /// Called only after the trusted server has verified both complete tool rounds.
    pub async fn record_subscription_tool_probe(
        &self,
        u: &UserContext,
        b: SubscriptionRef,
    ) -> Result<ModelChoice> {
        let u = u.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize(&tx, &db.provider_key, &SubscriptionAuthority::User(u.clone()), &b)?;
            let selection = ModelSelection { source: ModelSource::PersonalChatGpt, provider_id: b.id.clone(), provider_revision: b.revision, model: b.model.clone(), effort: b.effort.clone() };
            verified(&tx, &u, &selection)?;
            let changed = tx.execute("UPDATE chatgpt_model_probes SET function_tools=1 WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4 AND model=?5 AND effort=?6", params![b.tenant,b.account,b.id,b.revision,b.model,b.effort.as_deref().unwrap_or("")])?;
            if changed != 1 { return Err(StoreError::Conflict); }
            let result = verified(&tx, &u, &selection)?;
            tx.commit()?;
            Ok(result)
        }).await
    }
    pub async fn begin_subscription_disconnect(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<Option<TokenBundle>> {
        let u = u.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&u,true,false)?;
            let b=SubscriptionRef{
                tenant:u.tenant.0,account:u.account.id,id,revision,model:String::new(),effort:None}
            ;
            owned(&db.conn,&b)?;
            let row:Option<(i64,Option<Vec<u8>>)>=db.conn.query_row("SELECT generation,cipher FROM chatgpt_sessions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4",params![b.tenant,b.account,b.id,b.revision],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            db.conn.execute("UPDATE chatgpt_bindings SET status='revoking',remote_revoke='unconfirmed' WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND current_revision=?4",params![b.tenant,b.account,b.id,b.revision])?;
            let bundle=row.and_then(|(g,c)|c.map(|v|(g,v))).map(|(g,c)|sealer(&db.provider_key).open(&c,&context(&db.conn,&b,Some(g))?).map_err(|_|StoreError::Corrupt)).transpose();
            db.conn.execute("DELETE FROM chatgpt_sessions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3",params![b.tenant,b.account,b.id])?;
            bundle}
        ).await
    }
    pub async fn finish_subscription_disconnect(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
        confirmed: bool,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move|db|{
            db.conn.execute("UPDATE chatgpt_bindings SET status='disconnected',remote_revoke=?5 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND current_revision=?4 AND status='revoking'",params![u.tenant.0,u.account.id,id,revision,if confirmed{
                "confirmed"}
            else{
                "unconfirmed"}
            ])?;
            Ok(())}
        ).await
    }
}
fn validate_attempt(
    c: &Connection,
    u: &UserContext,
    a: &str,
    b: &SubscriptionRef,
    phase: &str,
) -> Result<()> {
    validate(c, u, true, false)?;
    if owned(c, b)? != "connecting" {
        return Err(StoreError::Conflict);
    }
    let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM chatgpt_auth_attempts WHERE id=?1 AND tenant_id=?2 AND account_id=?3 AND binding_id=?4 AND revision=?5 AND session_digest=?6 AND auth_epoch=?7 AND phase=?8 AND expires_at>?9)",params![a,b.tenant,b.account,b.id,b.revision,u.digest.as_slice(),u.epoch,phase,now()?],|r|r.get(0))?;
    if valid {
        Ok(())
    } else {
        Err(StoreError::Conflict)
    }
}
fn refresh_failure(c: &Connection, l: &SubscriptionRefresh, state: &str) -> Result<()> {
    let b = &l.binding;
    let changed=c.execute("UPDATE chatgpt_sessions SET refresh_state=?7,cipher=CASE WHEN ?7='idle' THEN cipher ELSE NULL END,refresh_nonce=NULL,refresh_until=NULL,cooldown_until=?8 WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4 AND generation=?5 AND refresh_nonce=?6 AND refresh_state='dispatched'",params![b.tenant,b.account,b.id,b.revision,l.generation,l.nonce,state,now()?+5])?;
    if changed == 1 && state != "idle" {
        c.execute("UPDATE chatgpt_bindings SET status='needs_reauth' WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND current_revision=?4 AND status='active'",params![b.tenant,b.account,b.id,b.revision])?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "subscriptions/handoff_tests.rs"]
mod handoff_tests;

#[cfg(test)]
#[path = "subscriptions/tools_tests.rs"]
mod tools_tests;
