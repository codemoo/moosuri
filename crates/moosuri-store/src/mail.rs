//! Owner-scoped server-managed Gmail credentials and immutable mail approvals.
//! Crypto/MIME work is local; network IO is never performed by the DB writer.
use super::*;

use moosuri_connectors::gmail::{FrozenMail, MailContent, ReplyContext, SecretString, TokenSet};

use moosuri_core::*;

mod external;

pub use external::{ExternalToolPermit, ExternalToolStart};

fn json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}

fn parse<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}

fn random() -> Result<String> {
    let mut bytes = [0; 32];

    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| StoreError::Unavailable)?;

    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn hash(s: &str) -> Vec<u8> {
    Sha256::digest(s.as_bytes()).to_vec()
}

fn aad(user: &UserContext, id: &str, revision: i64, purpose: &str) -> Result<Vec<u8>> {
    serde_json::to_vec(&(
        "moosuri/gmail/v1",
        purpose,
        &user.tenant.0,
        &user.account.id,
        id,
        revision,
    ))
    .map_err(|_| StoreError::InvalidInput)
}

fn oauth_aad(state: &[u8], purpose: &str) -> Vec<u8> {
    let mut a = format!("moosuri/gmail/oauth/{purpose}/v1").into_bytes();

    a.extend_from_slice(state);

    a
}

fn digest_string(digest: [u8; 32]) -> String {
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

fn digest_bytes(s: &str) -> Result<[u8; 32]> {
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(StoreError::Corrupt);
    }

    let mut out = [0; 32];

    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| StoreError::Corrupt)?;
    }

    Ok(out)
}

pub struct MailOAuthStart {
    pub state: String,
    pub verifier: Zeroizing<String>,
}

pub struct MailOAuthExchange {
    pub code: SecretString,
    pub verifier: SecretString,
    user: UserContext,
    state: Vec<u8>,
}

/// Trusted application carrier. Never Debug/Serialize or pass to the model.
pub struct MailBinding {
    pub id: String,
    pub epoch: i64,
    pub revision: i64,
    pub email: String,
    pub expires_at: i64,
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    user: UserContext,
    scopes: Vec<String>,
    dispatch: Option<(String, Vec<u8>)>,
}

impl MailBinding {
    pub fn user(&self) -> &UserContext {
        &self.user
    }
}

type DispatchRow = (
    String,
    String,
    String,
    String,
    i64,
    Vec<u8>,
    String,
    Option<String>,
    i64,
    i64,
);

pub struct MailDispatch {
    pub action_id: String,
    pub connection_id: String,
    pub frozen: FrozenMail,
    pub user: UserContext,
    nonce: Vec<u8>,
}

pub(crate) fn connection(
    conn: &Connection,
    user: &UserContext,
    id: &str,
) -> Result<MailConnection> {
    conn.query_row("SELECT id,email,epoch,status FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![user.tenant.0,user.account.id,id],|r|Ok(MailConnection{
        id:r.get(0)?,email:r.get(1)?,epoch:r.get(2)?,status:r.get(3)?}
    )).optional()?.ok_or(StoreError::NotFound)
}

fn binding(conn: &Connection, key: &[u8; 32], user: &UserContext, id: &str) -> Result<MailBinding> {
    let c = connection(conn, user, id)?;

    if c.status != "active" {
        return Err(StoreError::Conflict);
    }

    let (revision,cipher,expires,scopes):(i64,Vec<u8>,i64,String)=conn.query_row("SELECT credential_revision,credential,expires_at,scopes FROM mail_connections WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;

    let plain = providers::decrypt(key, &cipher, &aad(user, id, revision, "tokens")?)?;

    let (access, refresh): (String, String) =
        serde_json::from_slice(&plain).map_err(|_| StoreError::Corrupt)?;

    Ok(MailBinding {
        id: id.into(),
        epoch: c.epoch,
        revision,
        email: c.email,
        expires_at: expires,
        access_token: SecretString::new(access).map_err(|_| StoreError::Corrupt)?,
        refresh_token: SecretString::new(refresh).map_err(|_| StoreError::Corrupt)?,
        user: user.clone(),
        scopes: parse(&scopes)?,
        dispatch: None,
    })
}

fn action(conn: &Connection, user: &UserContext, id: &str) -> Result<MailAction> {
    conn.query_row("SELECT id,revision,connection_id,content,digest,state,expires_at,message_id,provider_message,provider_thread,origin_thread,version FROM mail_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![user.tenant.0,user.account.id,id],|r|Ok((r.get::<_,String>(0)?,r.get(1)?,r.get(2)?,r.get::<_,Option<String>>(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?,r.get(11)?))).optional()?.ok_or(StoreError::NotFound).and_then(|r|Ok(MailAction{
        id:r.0,version:r.11,revision:r.1,connection_id:r.2,content:r.3.map(|s|parse(&s)).transpose()?,digest:r.4,state:r.5,expires_at:r.6,message_id:r.7,provider_message_id:r.8,provider_thread_id:r.9,origin_thread_id:r.10}
    ))
}

pub(crate) fn draft(
    conn: &Connection,
    user: &UserContext,
    mut input: MailDraftInput,
    origin_run: Option<&str>,
    replace: Option<(String, i64)>,
) -> Result<MailAction> {
    let mut retained_origin = origin_run.map(str::to_owned);

    let c = connection(conn, user, &input.connection_id)?;

    if c.status != "active" || c.email != input.content.from {
        return Err(StoreError::Conflict);
    }

    if let Some(thread) = &input.origin_thread_id
        && !conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND account_id=?2 AND id=?3)",
            params![user.tenant.0, user.account.id, thread],
            |r| r.get::<_, bool>(0),
        )?
    {
        return Err(StoreError::NotFound);
    }

    let (id, revision) = if let Some((id, revision)) = replace {
        let old = action(conn, user, &id)?;

        if retained_origin.is_none() {
            retained_origin = conn.query_row(
                "SELECT origin_run FROM mail_actions WHERE tenant_id=?1 AND id=?2",
                params![user.tenant.0, id],
                |r| r.get::<_, Option<String>>(0),
            )?;
        }

        if old.origin_thread_id.is_some() {
            input.origin_thread_id = old.origin_thread_id.clone();
        }

        if old.revision != revision
            || !matches!(old.state.as_str(), "pending_approval" | "approved")
        {
            return Err(StoreError::Conflict);
        }

        (id, revision.checked_add(1).ok_or(StoreError::Corrupt)?)
    } else {
        (Uuid::new_v4().to_string(), 1)
    };

    let message_id = format!("{}@moosuri.local", Uuid::new_v4());

    let body = MailContent {
        from: input.content.from.clone(),
        to: input.content.to.clone(),
        cc: input.content.cc.clone(),
        bcc: input.content.bcc.clone(),
        subject: input.content.subject.clone(),
        text: input.content.text.clone(),
        reply: input.content.reply.as_ref().map(|r| ReplyContext {
            thread_id: r.thread_id.clone(),
            in_reply_to: r.in_reply_to.clone(),
            references: r.references.clone(),
        }),
    };

    let frozen = FrozenMail::prepare(&body, &message_id).map_err(|_| StoreError::InvalidInput)?;

    let expires = now()? + 3600;

    conn.execute("INSERT INTO mail_actions(tenant_id,account_id,id,revision,connection_id,connection_epoch,auth_epoch,content,raw,digest,message_id,reply_thread,origin_thread,origin_run,state,expires_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'pending_approval',?15) ON CONFLICT(tenant_id,id) DO UPDATE SET version=mail_actions.version+1,revision=excluded.revision,connection_id=excluded.connection_id,connection_epoch=excluded.connection_epoch,auth_epoch=excluded.auth_epoch,content=excluded.content,raw=excluded.raw,digest=excluded.digest,message_id=excluded.message_id,reply_thread=excluded.reply_thread,origin_thread=excluded.origin_thread,origin_run=excluded.origin_run,state='pending_approval',expires_at=excluded.expires_at,dispatch_nonce=NULL,provider_message=NULL,provider_thread=NULL",params![user.tenant.0,user.account.id,id,revision,c.id,c.epoch,user.epoch,json(&input.content)?,frozen.raw_mime(),digest_string(frozen.digest()),message_id,frozen.thread_id(),input.origin_thread_id,retained_origin,expires])?;

    action(conn, user, &id)
}

pub(super) fn purge_run(conn: &Connection, tenant: &str, run: &str) -> Result<()> {
    conn.execute("UPDATE mail_actions SET version=version+1,state=CASE WHEN state IN('pending_approval','approved') THEN 'cancelled' ELSE state END,content=NULL,raw=NULL WHERE tenant_id=?1 AND origin_run=?2",params![tenant,run])?;

    Ok(())
}

pub(super) fn purge_thread(conn: &Connection, tenant: &str, thread: &str) -> Result<()> {
    conn.execute("UPDATE mail_actions SET version=version+1,state=CASE WHEN state IN('pending_approval','approved') THEN 'cancelled' ELSE state END,content=NULL,raw=NULL WHERE tenant_id=?1 AND origin_thread=?2",params![tenant,thread])?;

    Ok(())
}

pub(super) fn pins(conn: &Connection, user: &UserContext) -> Result<Vec<MailConnectionPin>> {
    Ok(conn.prepare("SELECT id,epoch,email FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND status='active' ORDER BY id LIMIT 8")?.query_map(params![user.tenant.0,user.account.id],|r|Ok(MailConnectionPin{
        id:r.get(0)?,epoch:r.get(1)?,email:r.get(2)?}
    ))?.collect::<std::result::Result<Vec<_>,_>>()?)
}

impl Store {
    pub async fn mail_connections(&self, user: &UserContext) -> Result<Vec<MailConnection>> {
        let user = user.clone();

        self.call(move|db|{
            validate(&db.conn,&user,true,false)?;
            let ids=db.conn.prepare("SELECT id FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 ORDER BY id LIMIT 16")?.query_map(params![user.tenant.0,user.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            ids.iter().map(|id|connection(&db.conn,&user,id)).collect()}
        ).await
    }

    pub async fn start_mail_oauth(&self, user: &UserContext) -> Result<MailOAuthStart> {
        let user = user.clone();

        self.call(move|db|{
            validate(&db.conn,&user,true,false)?;
            db.conn.execute("DELETE FROM mail_oauth_sessions WHERE expires_at<?1",[now()?])?;
            if db.conn.query_row("SELECT count(*) FROM mail_oauth_sessions WHERE tenant_id=?1",[&user.tenant.0],|r|r.get::<_,i64>(0))?>=8{
                return Err(StoreError::Busy)}
            let state=random()?;
            let verifier=Zeroizing::new(random()?);
            let digest=hash(&state);
            let cipher=providers::encrypt(&db.provider_key,verifier.as_bytes(),&oauth_aad(&digest,"verifier"))?;
            db.conn.execute("INSERT INTO mail_oauth_sessions(state_hash,tenant_id,account_id,session_digest,auth_epoch,expires_at,verifier) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![digest,user.tenant.0,user.account.id,user.digest.as_slice(),user.epoch,now()?+600,cipher])?;
            Ok(MailOAuthStart{
                state,verifier}
            )}
        ).await
    }

    /// Unauthenticated GET callback authority is the unguessable, unexpired one-use
    /// state. This stores no token and never activates a connection.
    pub async fn capture_mail_oauth(&self, state: String, code: SecretString) -> Result<String> {
        if state.len() != 64 {
            return Err(StoreError::InvalidInput);
        }

        self.call(move|db|{
            let digest=hash(&state);
            let handle=random()?;
            let cipher=providers::encrypt(&db.provider_key,code.expose().as_bytes(),&oauth_aad(&digest,"code"))?;
            if db.conn.execute("UPDATE mail_oauth_sessions SET code=?2,result_hash=?3 WHERE state_hash=?1 AND expires_at>?4 AND code IS NULL AND claimed=0",params![digest,cipher,hash(&handle),now()?])?!=1{
                return Err(StoreError::Conflict)}
            Ok(handle)}
        ).await
    }

    pub async fn claim_mail_oauth(
        &self,
        user: &UserContext,
        handle: String,
    ) -> Result<MailOAuthExchange> {
        let user = user.clone();

        self.call(move|db|{
            validate(&db.conn,&user,true,false)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let(state,code,verifier):(Vec<u8>,Vec<u8>,Vec<u8>)=tx.query_row("SELECT state_hash,code,verifier FROM mail_oauth_sessions WHERE result_hash=?1 AND tenant_id=?2 AND account_id=?3 AND session_digest=?4 AND auth_epoch=?5 AND expires_at>?6 AND claimed=0 AND code IS NOT NULL",params![hash(&handle),user.tenant.0,user.account.id,user.digest.as_slice(),user.epoch,now()?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(StoreError::Conflict)?;
            let decode=|bytes:&[u8],purpose:&str|->Result<SecretString>{
                let plain=providers::decrypt(&db.provider_key,bytes,&oauth_aad(&state,purpose))?;
                SecretString::new(String::from_utf8(plain.to_vec()).map_err(|_|StoreError::Corrupt)?).map_err(|_|StoreError::Corrupt)};
            let result=MailOAuthExchange{
                code:decode(&code,"code")?,verifier:decode(&verifier,"verifier")?,user,state:state.clone()};
            tx.execute("UPDATE mail_oauth_sessions SET claimed=1,code=NULL,verifier=NULL WHERE state_hash=?1",[state])?;
            tx.commit()?;
            Ok(result)}
        ).await
    }

    pub async fn complete_mail_oauth(
        &self,
        exchange: MailOAuthExchange,
        tokens: TokenSet,
        email: String,
    ) -> Result<MailConnection> {
        self.call(move|db|{
            let user=validate(&db.conn,&exchange.user,true,false)?;
            if !db.conn.query_row("SELECT EXISTS(SELECT 1 FROM mail_oauth_sessions WHERE state_hash=?1 AND claimed=1 AND expires_at>?2)",params![exchange.state,now()?],|r|r.get::<_,bool>(0))?{
                return Err(StoreError::Conflict)}
            let scopes=tokens.scopes.unwrap_or_else(||moosuri_connectors::gmail::MINIMAL_SCOPES.iter().map(|s|s.to_string()).collect());
            if moosuri_connectors::gmail::MINIMAL_SCOPES.iter().any(|s|!scopes.iter().any(|v|v==s)){
                return Err(StoreError::Forbidden)}
            let refresh=tokens.refresh_token.ok_or(StoreError::Forbidden)?;
            let id=Uuid::new_v4().to_string();
            let plain=Zeroizing::new(serde_json::to_vec(&(tokens.access_token.expose(),refresh.expose())).map_err(|_|StoreError::InvalidInput)?);
            let cipher=providers::encrypt(&db.provider_key,&plain,&aad(&user,&id,1,"tokens")?)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if tx.query_row("SELECT count(*) FROM mail_connections WHERE tenant_id=?1 AND status='active'",[&user.tenant.0],|r|r.get::<_,i64>(0))?>=8{
                return Err(StoreError::Busy)}
            tx.execute("INSERT INTO mail_connections(tenant_id,account_id,id,email,epoch,credential_revision,status,credential,expires_at,scopes) VALUES(?1,?2,?3,?4,1,1,'active',?5,?6,?7)",params![user.tenant.0,user.account.id,id,email,cipher,now()?+tokens.expires_in as i64,json(&scopes)?])?;
            tx.execute("DELETE FROM mail_oauth_sessions WHERE state_hash=?1",[exchange.state])?;
            let c=connection(&tx,&user,&id)?;
            tx.commit()?;
            Ok(c)}
        ).await
    }

    pub async fn mail_binding(&self, user: &UserContext, id: String) -> Result<MailBinding> {
        let user = user.clone();

        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;

            binding(&db.conn, &db.provider_key, &user, &id)
        })
        .await
    }

    pub async fn save_mail_refresh(&self, old: &MailBinding, tokens: TokenSet) -> Result<()> {
        let user = old.user.clone();

        let dispatch = old.dispatch.clone();

        let id = old.id.clone();

        let epoch = old.epoch;

        let revision = old.revision;

        let refresh = tokens
            .refresh_token
            .map(|s| s.expose().to_owned())
            .unwrap_or_else(|| old.refresh_token.expose().into());

        let scopes = tokens.scopes.unwrap_or_else(|| old.scopes.clone());

        let plain = Zeroizing::new(
            serde_json::to_vec(&(tokens.access_token.expose(), refresh))
                .map_err(|_| StoreError::InvalidInput)?,
        );

        self.call(move|db|{
            if let Some((action,nonce))=dispatch{
                if !db.conn.query_row("SELECT EXISTS(SELECT 1 FROM mail_actions a JOIN accounts u ON u.tenant_id=a.tenant_id AND u.id=a.account_id WHERE a.tenant_id=?1 AND a.id=?2 AND a.dispatch_nonce=?3 AND a.state='dispatch_committed' AND u.auth_epoch=a.auth_epoch AND u.recovery_required=0)",params![user.tenant.0,action,nonce],|r|r.get::<_,bool>(0))?{
                    return Err(StoreError::Conflict)}
            }
            else{
                validate(&db.conn,&user,true,false)?;
            }

            if moosuri_connectors::gmail::MINIMAL_SCOPES.iter().any(|s|!scopes.iter().any(|v|v==s)){
                return Err(StoreError::Forbidden)}
            let cipher=providers::encrypt(&db.provider_key,&plain,&aad(&user,&id,revision+1,"tokens")?)?;
            if db.conn.execute("UPDATE mail_connections SET credential=?4,credential_revision=credential_revision+1,expires_at=?5,scopes=?6 WHERE tenant_id=?1 AND id=?2 AND epoch=?3 AND credential_revision=?7 AND status='active'",params![user.tenant.0,id,epoch,cipher,now()?+tokens.expires_in as i64,json(&scopes)?,revision])?!=1{
                return Err(StoreError::Conflict)}
            Ok(())}
        ).await
    }

    pub async fn mark_mail_needs_reconnect(&self, old: &MailBinding) -> Result<()> {
        let user = old.user.clone();

        let id = old.id.clone();

        let revision = old.revision;

        let epoch = old.epoch;

        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if tx.execute("UPDATE mail_connections SET status='needs_reconnect',epoch=epoch+1,credential_revision=credential_revision+1,credential=NULL WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND credential_revision=?4 AND epoch=?5 AND status='active' AND EXISTS(SELECT 1 FROM accounts WHERE tenant_id=?1 AND id=?2 AND auth_epoch=?6 AND recovery_required=0)",params![user.tenant.0,user.account.id,id,revision,epoch,user.epoch])?!=1{
                return Err(StoreError::Conflict)}
            tx.execute("UPDATE mail_actions SET version=version+1,state='cancelled',content=NULL,raw=NULL WHERE tenant_id=?1 AND connection_id=?2 AND state IN('pending_approval','approved')",params![user.tenant.0,id])?;
            let runs=tx.prepare("SELECT DISTINCT run_id FROM mail_run_dependencies WHERE tenant_id=?1 AND connection_id=?2")?.query_map(params![user.tenant.0,id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            agent_execution::purge_runs(&tx,&user.tenant.0,runs)?;
            tx.commit()?;
            Ok(())}
        ).await
    }

    pub async fn disconnect_mail(&self, user: &UserContext, id: String) -> Result<()> {
        let user = user.clone();

        self.call(move|db|{
            validate(&db.conn,&user,true,false)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            connection(&tx,&user,&id)?;
            tx.execute("UPDATE mail_connections SET status='disconnected',epoch=epoch+1,credential_revision=credential_revision+1,credential=NULL WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id])?;
            tx.execute("UPDATE mail_actions SET version=version+1,state='cancelled',content=NULL,raw=NULL WHERE tenant_id=?1 AND connection_id=?2 AND state IN('pending_approval','approved')",params![user.tenant.0,id])?;

            let runs=tx.prepare("SELECT DISTINCT run_id FROM mail_run_dependencies WHERE tenant_id=?1 AND connection_id=?2")?.query_map(params![user.tenant.0,id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;

            agent_execution::purge_runs(&tx,&user.tenant.0,runs)?;
            tx.commit()?;
            Ok(())}
        ).await
    }

    pub async fn create_mail_draft(
        &self,
        user: &UserContext,
        input: MailDraftInput,
        replace: Option<(String, i64)>,
    ) -> Result<MailAction> {
        let user = user.clone();

        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;

            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;

            let a = draft(&tx, &user, input, None, replace)?;

            tx.commit()?;

            Ok(a)
        })
        .await
    }

    pub async fn mail_actions(&self, user: &UserContext) -> Result<Vec<MailAction>> {
        let user = user.clone();

        self.call(move|db|{
            validate(&db.conn,&user,true,false)?;
            db.conn.execute("UPDATE mail_actions SET version=version+1,state='expired',raw=NULL,content=NULL WHERE tenant_id=?1 AND state IN('pending_approval','approved') AND expires_at<=?2",params![user.tenant.0,now()?])?;
            let ids=db.conn.prepare("SELECT id FROM mail_actions WHERE tenant_id=?1 AND account_id=?2 ORDER BY rowid DESC LIMIT 50")?.query_map(params![user.tenant.0,user.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            ids.iter().map(|id|action(&db.conn,&user,id)).collect()}
        ).await
    }

    pub async fn mail_action(&self, user: &UserContext, id: String) -> Result<MailAction> {
        let user = user.clone();

        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;

            action(&db.conn, &user, &id)
        })
        .await
    }

    pub async fn decide_mail(
        &self,
        user: &UserContext,
        id: String,
        expected: MailApproval,
        approve: bool,
    ) -> Result<MailAction> {
        let user = user.clone();

        self.call(move|db|{
            validate(&db.conn,&user,true,false)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let a=action(&tx,&user,&id)?;
            if a.revision!=expected.revision || a.digest!=expected.digest{
                return Err(StoreError::Conflict)}

            if a.state=="approved" && approve {
                tx.commit()?;
                return Ok(a)}

            if a.state!="pending_approval" || a.expires_at<=now()?{
                return Err(StoreError::Conflict)}
            let c=connection(&tx,&user,&a.connection_id)?;
            if c.status!="active"{
                return Err(StoreError::Conflict)}
            let state=if approve{
                "approved"}
            else{
                "rejected"};
            if tx.execute("UPDATE mail_actions SET version=version+1,state=?4,raw=CASE WHEN ?4='rejected' THEN NULL ELSE raw END,content=CASE WHEN ?4='rejected' THEN NULL ELSE content END WHERE tenant_id=?1 AND id=?2 AND connection_epoch=?3 AND auth_epoch=?5",params![user.tenant.0,id,c.epoch,state,user.epoch])?!=1{
                return Err(StoreError::Conflict)}
            let a=action(&tx,&user,&id)?;
            tx.commit()?;
            Ok(a)}
        ).await
    }

    /// Worker-only adoption: the returned nonce can account late receipts, not
    /// dispatch another action. No approved record is requeued after commitment.
    pub async fn next_mail_dispatch(&self) -> Result<Option<MailDispatch>> {
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute("UPDATE mail_actions SET version=version+1,state='expired',raw=NULL,content=NULL WHERE state='approved' AND expires_at<=?1",[now()?])?;
            let row:Option<DispatchRow>=tx.query_row("SELECT a.tenant_id,a.account_id,a.id,a.connection_id,a.auth_epoch,a.raw,a.digest,a.reply_thread,a.connection_epoch,a.revision FROM mail_actions a WHERE a.state='approved' ORDER BY a.rowid LIMIT 1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?))).optional()?;
            let Some((tenant,account,id,connection_id,epoch,raw,digest,reply,connection_epoch,_))=row else{
                tx.commit()?;
                return Ok(None)};

            // Approval outlives the originating chat session but not the account epoch.
            let current:Option<(AccountSummary,i64)>=tx.query_row("SELECT id,name,email,role,must_change_password,auth_epoch FROM accounts WHERE tenant_id=?1 AND id=?2 AND recovery_required=0 AND must_change_password=0",params![tenant,account],|r|Ok((summary_row(r)?,r.get(5)?))).optional()?;

            let Some((summary,current_epoch))=current else{
                tx.execute("UPDATE mail_actions SET version=version+1,state='cancelled',raw=NULL,content=NULL WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;
                tx.commit()?;
                return Ok(None)};

            let user=UserContext{
                account:summary,tenant:TenantId(tenant.clone()),epoch:current_epoch,restricted:false,digest:[0;
                32]};

            let c=connection(&tx,&user,&connection_id)?;

            if current_epoch!=epoch || c.epoch!=connection_epoch || c.status!="active"{
                tx.execute("UPDATE mail_actions SET version=version+1,state='cancelled',raw=NULL,content=NULL WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;
                tx.commit()?;
                return Ok(None)}

            let nonce=hash(&random()?);
            let frozen=FrozenMail::restore(raw,digest_bytes(&digest)?,reply).map_err(|_|StoreError::Corrupt)?;

            tx.execute("UPDATE mail_actions SET version=version+1,state='dispatch_committed',dispatch_nonce=?3,raw=NULL WHERE tenant_id=?1 AND id=?2 AND state='approved'",params![tenant,id,nonce])?;
            tx.commit()?;
            Ok(Some(MailDispatch{
                action_id:id,connection_id,frozen,user,nonce}
            ))}
        ).await
    }

    /// A dispatch carrier has no session cookie; authority comes solely from the
    /// persisted committed nonce and unchanged account/connection epochs.
    pub async fn mail_dispatch_binding(&self, dispatch: &MailDispatch) -> Result<MailBinding> {
        let user = dispatch.user.clone();

        let id = dispatch.action_id.clone();

        let c = dispatch.connection_id.clone();

        let nonce = dispatch.nonce.clone();

        self.call(move|db|{
            if !db.conn.query_row("SELECT EXISTS(SELECT 1 FROM mail_actions a JOIN accounts u ON u.tenant_id=a.tenant_id AND u.id=a.account_id JOIN mail_connections c ON c.tenant_id=a.tenant_id AND c.id=a.connection_id WHERE a.tenant_id=?1 AND a.id=?2 AND a.dispatch_nonce=?3 AND a.state='dispatch_committed' AND u.auth_epoch=a.auth_epoch AND u.recovery_required=0 AND c.epoch=a.connection_epoch AND c.status='active')",params![user.tenant.0,id,nonce],|r|r.get::<_,bool>(0))?{
                return Err(StoreError::Conflict)}
            let mut b=binding(&db.conn,&db.provider_key,&user,&c)?;
            b.dispatch=Some((id,nonce));
            Ok(b)}
        ).await
    }

    pub async fn finish_mail_dispatch(
        &self,
        dispatch: &MailDispatch,
        receipt: Option<(String, String)>,
        definite: bool,
    ) -> Result<()> {
        let tenant = dispatch.user.tenant.0.clone();

        let id = dispatch.action_id.clone();

        let nonce = dispatch.nonce.clone();

        self.call(move|db|{
            let(state,message,thread)=if let Some((m,t))=receipt{
                ("sent",Some(m),Some(t))}
            else{
                (if definite{
                    "failed_definite"}
                else{
                    "outcome_unknown"}
                ,None,None)};
            if db.conn.execute("UPDATE mail_actions SET version=version+1,state=?4,provider_message=?5,provider_thread=?6,raw=NULL WHERE tenant_id=?1 AND id=?2 AND dispatch_nonce=?3 AND state IN('dispatch_committed','outcome_unknown')",params![tenant,id,nonce,state,message,thread])?!=1{
                return Err(StoreError::Conflict)}
            Ok(())}
        ).await
    }
}

#[cfg(test)]
mod tests;
