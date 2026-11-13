//! Personal Calendar credentials and durable direct-owner dispatch ledger. No network IO.
use super::*;
use moosuri_connectors::calendar::SCOPES;
use moosuri_connectors::gmail::{SecretString, TokenSet};
use moosuri_core::*;
fn random() -> Result<String> {
    let mut bytes = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| StoreError::Unavailable)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn token_hash(v: &str) -> Vec<u8> {
    Sha256::digest(v.as_bytes()).to_vec()
}
fn json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn parse<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
fn aad(u: &UserContext, id: &str, revision: i64) -> Result<Vec<u8>> {
    Ok(json(&(
        "moosuri/calendar/tokens/v1",
        &u.tenant.0,
        &u.account.id,
        u.epoch,
        id,
        revision,
    ))?
    .into_bytes())
}
fn oauth_aad(state: &[u8], kind: &str) -> Vec<u8> {
    let mut v = format!("moosuri/calendar/oauth/{kind}/v1").into_bytes();
    v.extend_from_slice(state);
    v
}
fn scopes(tokens: &TokenSet, initial: bool) -> Result<()> {
    if (initial && tokens.scopes.is_none())
        || tokens.scopes.as_ref().is_some_and(|s| {
            SCOPES.iter().any(|r| !s.iter().any(|s| s == r))
                || s.iter().any(|s| !SCOPES.contains(&s.as_str()))
        })
    {
        return Err(StoreError::Forbidden);
    }
    Ok(())
}
fn connection(c: &Connection, u: &UserContext, id: &str) -> Result<CalendarConnection> {
    c.query_row("SELECT id,primary_id,title,time_zone,epoch,credential_revision,status,auth_epoch FROM calendar_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,id],|r|{let epoch:i64=r.get(7)?;Ok(CalendarConnection{id:r.get(0)?,primary_id:r.get(1)?,title:r.get(2)?,time_zone:r.get(3)?,epoch:r.get(4)?,revision:r.get(5)?,status:if epoch!=u.epoch{"needs_reconnect".into()}else{r.get(6)?}})}).optional()?.ok_or(StoreError::NotFound)
}
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredTokens {
    access: String,
    refresh: String,
}
/// Trusted carriers only; no Debug/Serialize implementation.
pub struct CalendarBinding {
    pub connection: CalendarConnection,
    pub access: SecretString,
    pub refresh: SecretString,
    pub expires_at: i64,
    user: UserContext,
    pub app: GoogleAppPin,
}
pub struct CalendarOAuthStart {
    pub state: String,
    pub verifier: Zeroizing<String>,
}
pub struct CalendarOAuthExchange {
    pub code: SecretString,
    pub verifier: SecretString,
    user: UserContext,
    state: Vec<u8>,
    app: GoogleAppPin,
}
pub struct CalendarDispatch {
    pub change: CalendarChange,
    pub primary_id: String,
    pub event_id: String,
    pub digest: String,
    user: UserContext,
    nonce: Vec<u8>,
    app: GoogleAppPin,
    settlement_revision: i64,
}
pub enum CalendarAdmission {
    Recorded(CalendarAction),
    Dispatch(CalendarDispatch),
}
type ActionRow = (
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
);
fn action(c: &Connection, u: &UserContext, id: &str) -> Result<CalendarAction> {
    let v:ActionRow=c.query_row("SELECT id,connection_id,state,event_id,before_etag,provider_etag,event,receipt,error,version,auth_epoch FROM calendar_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?))).optional()?.ok_or(StoreError::NotFound)?;
    Ok(CalendarAction{id:v.0,connection_id:v.1,state:v.2,event_id:v.3,before_etag:v.4,provider_etag:v.5,event:if v.10==u.epoch && c.query_row("SELECT EXISTS(SELECT 1 FROM calendar_actions a JOIN calendar_connections c ON c.tenant_id=a.tenant_id AND c.id=a.connection_id WHERE a.tenant_id=?1 AND a.id=?2 AND c.status='active' AND c.auth_epoch=?3 AND c.epoch=a.connection_epoch)",params![u.tenant.0,id,u.epoch],|r|r.get::<_,bool>(0))?{v.6.as_deref().map(parse).transpose()?}else{None},receipt:v.7,error:v.8,version:v.9})
}
pub(crate) fn restart(c: &Connection) -> Result<()> {
    c.execute("UPDATE calendar_actions SET state='outcome_unknown',error='server_restarted',version=version+1 WHERE state='dispatch_committed'",[])?;
    Ok(())
}
pub(crate) fn disconnect(
    c: &Connection,
    t: &str,
    id: &str,
    epoch: i64,
    revision: i64,
) -> Result<()> {
    // The provider account is unknown until OAuth completes. Retire every
    // pending or claimed flow owned by this connection's exact account in the
    // durable disconnect transaction, including an exchange already in flight.
    c.execute("DELETE FROM calendar_oauth_sessions WHERE tenant_id=?1 AND account_id IN (SELECT account_id FROM calendar_connections WHERE tenant_id=?1 AND id=?2)",params![t,id])?;
    c.execute("UPDATE calendar_connections SET status='disconnected',epoch=max(epoch,?3),credential_revision=max(credential_revision,?4),credential=NULL WHERE tenant_id=?1 AND id=?2",params![t,id,epoch,revision])?;
    c.execute("UPDATE calendar_actions SET payload=NULL,event=NULL,version=version+1,state=CASE WHEN state='dispatch_committed' THEN 'outcome_unknown' ELSE state END WHERE tenant_id=?1 AND connection_id=?2",params![t,id])?;
    Ok(())
}
impl Store {
    pub async fn calendar_connections(&self, u: &UserContext) -> Result<Vec<CalendarConnection>> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;let ids=db.conn.prepare("SELECT id FROM calendar_connections WHERE tenant_id=?1 AND account_id=?2 ORDER BY id")?.query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;ids.iter().map(|id|connection(&db.conn,&u,id)).collect()}).await
    }
    pub async fn start_calendar_oauth(
        &self,
        u: &UserContext,
        app: GoogleAppPin,
    ) -> Result<CalendarOAuthStart> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;crate::google_setup::guard_app(&db.conn,&app)?;db.conn.execute("DELETE FROM calendar_oauth_sessions WHERE expires_at<=?1",[now()?])?;let count:i64=db.conn.query_row("SELECT count(*) FROM calendar_oauth_sessions WHERE tenant_id=?1",[&u.tenant.0],|r|r.get(0))?;if count>=8{return Err(StoreError::Busy)}let state=format!("cal.{}",random()?);let verifier=random()?;let digest=token_hash(&state);let cipher=providers::encrypt(&db.provider_key,verifier.as_bytes(),&oauth_aad(&digest,"verifier"))?;db.conn.execute("INSERT INTO calendar_oauth_sessions(state_hash,tenant_id,account_id,session_digest,auth_epoch,expires_at,verifier,app_pin) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![digest,u.tenant.0,u.account.id,u.digest.as_slice(),u.epoch,now()?+600,cipher,json(&app)?])?;Ok(CalendarOAuthStart{state,verifier:Zeroizing::new(verifier)})}).await
    }
    pub async fn capture_calendar_oauth(
        &self,
        state: String,
        code: Option<SecretString>,
    ) -> Result<Option<String>> {
        self.call(move|db|{if state.len()>256||!state.starts_with("cal."){return Err(StoreError::InvalidInput)}let digest=token_hash(&state);let Some(code)=code else{db.conn.execute("DELETE FROM calendar_oauth_sessions WHERE state_hash=?1 AND claimed=0",[digest])?;return Ok(None)};let handle=random()?;let cipher=providers::encrypt(&db.provider_key,code.expose().as_bytes(),&oauth_aad(&digest,"code"))?;if db.conn.execute("UPDATE calendar_oauth_sessions SET code=?2,result_hash=?3 WHERE state_hash=?1 AND expires_at>?4 AND code IS NULL AND claimed=0",params![digest,cipher,token_hash(&handle),now()?])?!=1{return Err(StoreError::Conflict)}Ok(Some(handle))}).await
    }
    pub async fn claim_calendar_oauth(
        &self,
        u: &UserContext,
        handle: String,
        app: GoogleAppPin,
    ) -> Result<CalendarOAuthExchange> {
        let u = u.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&u,true,false)?;crate::google_setup::guard_app(&tx,&app)?;let(state,code,verifier,pin):(Vec<u8>,Vec<u8>,Vec<u8>,String)=tx.query_row("SELECT state_hash,code,verifier,app_pin FROM calendar_oauth_sessions WHERE result_hash=?1 AND tenant_id=?2 AND account_id=?3 AND session_digest=?4 AND auth_epoch=?5 AND claimed=0 AND expires_at>?6 AND code IS NOT NULL",params![token_hash(&handle),u.tenant.0,u.account.id,u.digest.as_slice(),u.epoch,now()?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(StoreError::Conflict)?;if json(&app)?!=pin{return Err(StoreError::Conflict)}let decrypt=|bytes:Vec<u8>,kind:&str|->Result<SecretString>{let plain=providers::decrypt(&db.provider_key,&bytes,&oauth_aad(&state,kind))?;SecretString::new(String::from_utf8(plain.to_vec()).map_err(|_|StoreError::Corrupt)?).map_err(|_|StoreError::Corrupt)};let result=CalendarOAuthExchange{code:decrypt(code,"code")?,verifier:decrypt(verifier,"verifier")?,user:u,state:state.clone(),app};tx.execute("UPDATE calendar_oauth_sessions SET claimed=1,code=NULL,verifier=NULL WHERE state_hash=?1",[state])?;tx.commit()?;Ok(result)}).await
    }
    pub async fn cancel_calendar_oauth(&self, e: &CalendarOAuthExchange) -> Result<()> {
        let state = e.state.clone();
        self.call(move |db| {
            db.conn.execute(
                "DELETE FROM calendar_oauth_sessions WHERE state_hash=?1",
                [state],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn complete_calendar_oauth(
        &self,
        e: CalendarOAuthExchange,
        tokens: TokenSet,
        primary: CalendarInfo,
    ) -> Result<CalendarConnection> {
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&e.user,true,false)?;crate::google_setup::guard_app(&tx,&e.app)?;scopes(&tokens,true)?;if !primary.primary||primary.access_role!="owner"||primary.id.is_empty()||primary.id.len()>1024{return Err(StoreError::Forbidden)}let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM calendar_oauth_sessions WHERE state_hash=?1 AND claimed=1 AND expires_at>?2)",params![e.state,now()?],|r|r.get(0))?;if !exists{return Err(StoreError::Conflict)}let refresh=tokens.refresh_token.ok_or(StoreError::Forbidden)?;let existing:Option<(String,i64,i64)>=tx.query_row("SELECT id,epoch,credential_revision FROM calendar_connections WHERE tenant_id=?1 AND primary_id=?2",params![u.tenant.0,primary.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;let(id,epoch,revision)=existing.map(|(id,e,r)|(id,e+1,r+1)).unwrap_or((Uuid::new_v4().to_string(),1,1));let plain=Zeroizing::new(json(&StoredTokens{access:tokens.access_token.expose().into(),refresh:refresh.expose().into()})?);let cipher=providers::encrypt(&db.provider_key,plain.as_bytes(),&aad(&u,&id,revision)?)?;tx.execute("INSERT INTO calendar_connections VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'active',?10,?11,?12,?13) ON CONFLICT(tenant_id,primary_id) DO UPDATE SET title=excluded.title,time_zone=excluded.time_zone,auth_epoch=excluded.auth_epoch,epoch=excluded.epoch,credential_revision=excluded.credential_revision,status='active',credential=excluded.credential,expires_at=excluded.expires_at,scopes=excluded.scopes,client_id=excluded.client_id",params![u.tenant.0,u.account.id,id,primary.id,primary.title,primary.time_zone,u.epoch,epoch,revision,cipher,now()?+tokens.expires_in as i64,json(&tokens.scopes)?,e.app.client_id])?;tx.execute("DELETE FROM calendar_oauth_sessions WHERE state_hash=?1",[e.state])?;let result=connection(&tx,&u,&id)?;tx.commit()?;Ok(result)}).await
    }
    pub async fn calendar_binding(
        &self,
        u: &UserContext,
        id: String,
        app: GoogleAppPin,
    ) -> Result<CalendarBinding> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;crate::google_setup::guard_app(&db.conn,&app)?;let c=connection(&db.conn,&u,&id)?;if c.status!="active"{return Err(StoreError::Conflict)}let(cipher,expires,client):(Vec<u8>,i64,String)=db.conn.query_row("SELECT credential,expires_at,client_id FROM calendar_connections WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;if client!=app.client_id{return Err(StoreError::Conflict)}let plain=providers::decrypt(&db.provider_key,&cipher,&aad(&u,&id,c.revision)?)?;let tokens:StoredTokens=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;Ok(CalendarBinding{connection:c,access:SecretString::new(tokens.access).map_err(|_|StoreError::Corrupt)?,refresh:SecretString::new(tokens.refresh).map_err(|_|StoreError::Corrupt)?,expires_at:expires,user:u,app})}).await
    }
    pub async fn check_calendar_binding(&self, b: &CalendarBinding) -> Result<()> {
        let u = b.user.clone();
        let c = b.connection.clone();
        let app = b.app.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            crate::google_setup::guard_app(&db.conn, &app)?;
            let current = connection(&db.conn, &u, &c.id)?;
            if current.status != "active"
                || current.epoch != c.epoch
                || current.revision != c.revision
            {
                return Err(StoreError::Conflict);
            }
            Ok(())
        })
        .await
    }
    pub async fn refresh_calendar(&self, b: &CalendarBinding, tokens: TokenSet) -> Result<()> {
        let u = b.user.clone();
        let id = b.connection.id.clone();
        let old = b.connection.clone();
        let app = b.app.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&u,true,false)?;crate::google_setup::guard_app(&tx,&app)?;let current=connection(&tx,&u,&id)?;if current.status!="active"||current.epoch!=old.epoch||current.revision!=old.revision{return Err(StoreError::Conflict)}scopes(&tokens,false)?;let(cipher,client):(Vec<u8>,String)=tx.query_row("SELECT credential,client_id FROM calendar_connections WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id],|r|Ok((r.get(0)?,r.get(1)?)))?;if client!=app.client_id{return Err(StoreError::Conflict)}let plain=providers::decrypt(&db.provider_key,&cipher,&aad(&u,&id,old.revision)?)?;let prior:StoredTokens=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;let next=Zeroizing::new(json(&StoredTokens{access:tokens.access_token.expose().into(),refresh:tokens.refresh_token.as_ref().map(|v|v.expose().to_owned()).unwrap_or(prior.refresh)})?);let cipher=providers::encrypt(&db.provider_key,next.as_bytes(),&aad(&u,&id,old.revision+1)?)?;tx.execute("UPDATE calendar_connections SET credential=?3,credential_revision=credential_revision+1,expires_at=?4 WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id,cipher,now()?+tokens.expires_in as i64])?;tx.commit()?;Ok(())}).await
    }
    pub async fn calendar_needs_reconnect(&self, b: &CalendarBinding) -> Result<()> {
        let u = b.user.clone();
        let c = b.connection.clone();
        let app = b.app.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;crate::google_setup::guard_app(&db.conn,&app)?;db.conn.execute("UPDATE calendar_connections SET status='needs_reconnect',credential=NULL,epoch=epoch+1,credential_revision=credential_revision+1 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND epoch=?4 AND credential_revision=?5",params![u.tenant.0,u.account.id,c.id,c.epoch,c.revision])?;Ok(())}).await
    }
    pub async fn disconnect_calendar(&self, u: &UserContext, id: String) -> Result<()> {
        let u = u.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &u, true, false)?;
            let c = connection(&db.conn, &u, &id)?;
            crate::destructive_intents::commit(
                db,
                &u.tenant.0,
                crate::destructive_intents::Effect::DisconnectCalendar {
                    target: id,
                    epoch: c.epoch + 1,
                    revision: c.revision + 1,
                },
            )
        })
        .await
    }
    pub async fn calendar_actions(&self, u: &UserContext) -> Result<Vec<CalendarAction>> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;db.conn.execute("UPDATE calendar_actions SET state='outcome_unknown',error='dispatch_receipt_missing',version=version+1 WHERE tenant_id=?1 AND account_id=?2 AND state='dispatch_committed' AND created_at<=?3",params![u.tenant.0,u.account.id,now()?-90])?;let ids=db.conn.prepare("SELECT id FROM calendar_actions WHERE tenant_id=?1 AND account_id=?2 ORDER BY created_at DESC,id LIMIT 100")?.query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;ids.iter().map(|id|action(&db.conn,&u,id)).collect()}).await
    }
    pub async fn existing_calendar_change(
        &self,
        u: &UserContext,
        input: CalendarChange,
    ) -> Result<Option<CalendarAction>> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;let digest=format!("{:x}",Sha256::digest(json(&input)?.as_bytes()));let old=db.conn.query_row("SELECT digest FROM calendar_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,input.request_id],|r|r.get::<_,String>(0)).optional()?;match old{Some(old)if old!=digest=>Err(StoreError::Conflict),Some(_)=>action(&db.conn,&u,&input.request_id).map(Some),None=>Ok(None)}}).await
    }
    pub async fn admit_calendar_change(
        &self,
        b: &CalendarBinding,
        change: CalendarChange,
        primary: CalendarInfo,
        prior: Option<CalendarEvent>,
    ) -> Result<CalendarAdmission> {
        let u = b.user.clone();
        let conn = b.connection.clone();
        let app = b.app.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&u,true,false)?;crate::google_setup::guard_app(&tx,&app)?;let payload=json(&change)?;Uuid::parse_str(&change.request_id).map_err(|_|StoreError::InvalidInput)?;let digest=format!("{:x}",Sha256::digest(payload.as_bytes()));if let Some(old)=tx.query_row("SELECT digest FROM calendar_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,change.request_id],|r|r.get::<_,String>(0)).optional()?{if old!=digest{return Err(StoreError::Conflict)}return Ok(CalendarAdmission::Recorded(action(&tx,&u,&change.request_id)?))}
 let current=connection(&tx,&u,&change.connection_id)?;if change.connection_id!=conn.id||change.expected_epoch!=current.epoch||change.expected_revision!=current.revision||conn.epoch!=current.epoch||conn.revision!=current.revision||current.status!="active"||!primary.primary||primary.access_role!="owner"||primary.email_notifications||primary.id!=current.primary_id{return Err(StoreError::Conflict)}let count:i64=tx.query_row("SELECT count(*) FROM calendar_actions WHERE tenant_id=?1 AND state IN('dispatch_committed','outcome_unknown')",[&u.tenant.0],|r|r.get(0))?;if count>=16{return Err(StoreError::Busy)}
 let(kind,event_id,etag)=match &change.mutation{CalendarMutation::Create{fields}=>{moosuri_connectors::calendar::validate_fields(fields).map_err(|_|StoreError::InvalidInput)?;("create",format!("m{}",change.request_id.replace('-',"")),None)},CalendarMutation::Update{event_id,etag,fields}=>{moosuri_connectors::calendar::validate_fields(fields).map_err(|_|StoreError::InvalidInput)?;let p=prior.as_ref().ok_or(StoreError::InvalidInput)?;if !p.writable||p.id!=*event_id||p.etag!=*etag{return Err(StoreError::Conflict)}("update",event_id.clone(),Some(etag.clone()))},CalendarMutation::Delete{event_id,etag}=>{let p=prior.as_ref().ok_or(StoreError::InvalidInput)?;if !p.writable||p.id!=*event_id||p.etag!=*etag{return Err(StoreError::Conflict)}("delete",event_id.clone(),Some(etag.clone()))}};
 moosuri_connectors::calendar::event_id(&event_id).map_err(|_|StoreError::InvalidInput)?;let nonce=token_hash(&random()?);tx.execute("INSERT INTO calendar_actions(tenant_id,account_id,id,connection_id,connection_epoch,credential_revision,auth_epoch,session_digest,app_pin,payload,digest,kind,event_id,before_etag,state,nonce,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'dispatch_committed',?15,?16)",params![u.tenant.0,u.account.id,change.request_id,current.id,current.epoch,current.revision,u.epoch,u.digest.as_slice(),json(&app)?,payload,digest,kind,event_id,etag,nonce,now()?])?;tx.commit()?;Ok(CalendarAdmission::Dispatch(CalendarDispatch{change,primary_id:primary.id,event_id,digest,user:u,nonce,app,settlement_revision:current.revision}))}).await
    }
    pub async fn settle_calendar_change(
        &self,
        d: &CalendarDispatch,
        state: String,
        event: Option<CalendarEvent>,
        receipt: String,
        error: Option<String>,
    ) -> Result<CalendarAction> {
        let u = d.user.clone();
        let id = d.change.request_id.clone();
        let nonce = d.nonce.clone();
        let app = d.app.clone();
        let c = d.change.connection_id.clone();
        let epoch = d.change.expected_epoch;
        let d_revision = d.settlement_revision;
        self.call(move|db|{if !["applied","outcome_unknown","conflict","failed_definite","observed_absent"].contains(&state.as_str())||receipt.len()>4096{return Err(StoreError::InvalidInput)}let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let matches:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM calendar_actions WHERE tenant_id=?1 AND id=?2 AND nonce=?3)",params![u.tenant.0,id,nonce],|r|r.get(0))?;if !matches{return Err(StoreError::Unauthorized)}let eligible=validate(&tx,&u,true,false).is_ok()&&crate::google_setup::guard_app(&tx,&app).is_ok()&&connection(&tx,&u,&c).is_ok_and(|v|v.status=="active"&&v.epoch==epoch&&v.revision==d_revision);let old:String=tx.query_row("SELECT state FROM calendar_actions WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id],|r|r.get(0))?;if matches!(old.as_str(),"dispatch_committed"|"outcome_unknown"){tx.execute("UPDATE calendar_actions SET state=?3,provider_etag=?4,event=?5,receipt=?6,error=?7,version=version+1,payload=CASE WHEN ?8 THEN payload ELSE NULL END WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id,state,event.as_ref().map(|e|&e.etag),if eligible{event.as_ref().map(json).transpose()?}else{None},receipt,error,eligible])?;}let result=if eligible{action(&tx,&u,&id)}else{Err(StoreError::Unauthorized)};tx.commit()?;result}).await
    }
    pub async fn calendar_reconcile(
        &self,
        u: &UserContext,
        id: String,
        app: GoogleAppPin,
    ) -> Result<CalendarDispatch> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;crate::google_setup::guard_app(&db.conn,&app)?;let(payload,digest,event_id,nonce,pin,epoch):(String,String,String,Vec<u8>,String,i64)=db.conn.query_row("SELECT payload,digest,event_id,nonce,app_pin,auth_epoch FROM calendar_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='outcome_unknown' AND payload IS NOT NULL",params![u.tenant.0,u.account.id,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?.ok_or(StoreError::NotFound)?;if epoch!=u.epoch||pin!=json(&app)?{return Err(StoreError::Conflict)}let change:CalendarChange=parse(&payload)?;let c=connection(&db.conn,&u,&change.connection_id)?;if c.status!="active"||c.epoch!=change.expected_epoch{return Err(StoreError::Conflict)}Ok(CalendarDispatch{change,primary_id:c.primary_id,event_id,digest,user:u,nonce,app,settlement_revision:c.revision})}).await
    }
}
#[cfg(test)]
mod tests;
