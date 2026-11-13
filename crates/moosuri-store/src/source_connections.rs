//! Scoped source ingestion. Provider IO never runs on the SQLite writer.
//! Neither producer grants nor scheduler capabilities can read original sources.
use super::*;
use moosuri_connectors::gmail::{SecretString, TokenSet};
use moosuri_core::*;
use serde::{Serialize, de::DeserializeOwned};

const LEASE_SECONDS: i64 = 300;
const RECEIPT_LIMIT: i64 = 1024;
const RUN_LIMIT: i64 = 64;
type ResetCredentialRow = (i64, Option<Vec<u8>>, Option<i64>, Option<Vec<u8>>, String);

/// Ingestion/status only. Not serializable and never a session/UserContext.
#[derive(Clone)]
pub struct SourceIngestCapability {
    tenant: String,
    account: String,
    connection: String,
    auth_epoch: i64,
    config_revision: i64,
    credential_epoch: i64,
    token_hash: Option<[u8; 32]>,
}
/// A bearer secret returned once. Never Debug or serialize this carrier.
pub struct SourceTokenGrant {
    pub token: String,
    pub expires_at: i64,
}
#[derive(Clone)]
pub struct SourceRunLease {
    cap: SourceIngestCapability,
    id: String,
    generation: i64,
    nonce: Vec<u8>,
}
impl SourceRunLease {
    pub fn run_id(&self) -> &str {
        &self.id
    }
    pub fn generation(&self) -> i64 {
        self.generation
    }
}
/// Private runtime carrier. Checkpoints and credentials never enter safe DTOs.
pub struct SourceRuntimeBinding {
    pub connection: SourceConnection,
    pub checkpoint: Option<String>,
    pub slack_token: Option<SecretString>,
}
pub struct SourceGmailCredentials {
    pub app: Option<GoogleAppPin>,
    pub mail_connection_id: String,
    pub mail_epoch: i64,
    pub revision: i64,
    pub expires_at: i64,
    pub access_token: SecretString,
    pub refresh_token: SecretString,
}
fn json<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn parse<T: DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
fn kind(v: SourceConnectionKind) -> &'static str {
    match v {
        SourceConnectionKind::CustomHttp => "custom_http",
        SourceConnectionKind::Gmail => "gmail",
        SourceConnectionKind::Slack => "slack",
    }
}
fn parse_kind(v: &str) -> Result<SourceConnectionKind> {
    match v {
        "custom_http" => Ok(SourceConnectionKind::CustomHttp),
        "gmail" => Ok(SourceConnectionKind::Gmail),
        "slack" => Ok(SourceConnectionKind::Slack),
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
fn checkpoint(v: &Option<String>) -> Result<()> {
    if v.as_ref()
        .is_some_and(|s| s.len() > 16384 || s.is_empty() || s.chars().any(char::is_control))
    {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn connection(conn: &Connection, tenant: &str, id: &str) -> Result<SourceConnection> {
    let row=conn.query_row("SELECT c.id,c.collection_id,c.name,c.kind,c.scope,c.remote_account,c.mail_connection_id,c.config_revision,c.credential_epoch,c.state,c.sync_enabled,c.poll_interval,k.cursor,c.last_heartbeat,c.last_commit,c.last_completed,c.next_due,c.error,((c.token_hash IS NOT NULL AND c.token_expires>unixepoch()) OR c.credential IS NOT NULL OR (c.kind='gmail' AND EXISTS(SELECT 1 FROM mail_connections m WHERE m.tenant_id=c.tenant_id AND m.id=c.mail_connection_id AND m.epoch=c.mail_epoch AND m.status='active'))),k.connected,c.adapter_version,k.deleted FROM source_connections c JOIN knowledge_collections k ON k.tenant_id=c.tenant_id AND k.id=c.collection_id WHERE c.tenant_id=?1 AND c.id=?2",params![tenant,id],|r|Ok((SourceConnection {
        id:r.get(0)?,collection_id:r.get(1)?,name:r.get(2)?,kind:SourceConnectionKind::CustomHttp,adapter_version:r.get(20)?,scope:r.get(4)?,remote_account:r.get(5)?,mail_connection_id:r.get(6)?,config_revision:r.get(7)?,credential_epoch:r.get(8)?,state:r.get(9)?,sync_enabled:r.get(10)?,poll_interval_seconds:r.get(11)?,local_sequence:r.get(12)?,last_heartbeat_at:r.get(13)?,last_commit_at:r.get(14)?,last_completed_at:r.get(15)?,next_due_at:r.get(16)?,error:r.get(17)?,credential_present:r.get(18)?
    },r.get::<_,String>(3)?,r.get::<_,bool>(19)?,r.get::<_,bool>(21)?))).optional()?.ok_or(StoreError::NotFound)?;
    let (mut c, k, connected, deleted) = row;
    c.kind = parse_kind(&k)?;
    if !connected {
        c.state = if deleted { "deleted" } else { "disconnected" }.into();
        c.credential_present = false;
    }
    let account_valid:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM source_connections c JOIN accounts a ON a.tenant_id=c.tenant_id AND a.id=c.account_id WHERE c.tenant_id=?1 AND c.id=?2 AND a.auth_epoch=c.auth_epoch AND a.recovery_required=0 AND a.must_change_password=0)",params![tenant,id],|r|r.get(0))?;
    if !account_valid {
        c.credential_present = false;
        if c.state == "active" {
            c.state = "needs_reconnect".into();
        }
    }
    if c.kind == SourceConnectionKind::Gmail && c.state == "active" && !c.credential_present {
        c.state = "needs_reconnect".into();
    }
    Ok(c)
}
fn owned(conn: &Connection, user: &UserContext, id: &str) -> Result<SourceConnection> {
    if !conn.query_row("SELECT EXISTS(SELECT 1 FROM source_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3)",params![user.tenant.0,user.account.id,id],|r|r.get::<_,bool>(0))? {return Err(StoreError::NotFound)}
    connection(conn, &user.tenant.0, id)
}
fn capability(
    conn: &Connection,
    tenant: String,
    account: String,
    id: String,
    token_hash: Option<[u8; 32]>,
) -> Result<SourceIngestCapability> {
    let (auth_epoch,config_revision,credential_epoch)=conn.query_row("SELECT auth_epoch,config_revision,credential_epoch FROM source_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![tenant,account,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let cap = SourceIngestCapability {
        tenant,
        account,
        connection: id,
        auth_epoch,
        config_revision,
        credential_epoch,
        token_hash,
    };
    guard(conn, &cap)?;
    Ok(cap)
}
fn guard(conn: &Connection, cap: &SourceIngestCapability) -> Result<SourceConnection> {
    let time = now()?;
    let allowed=conn.query_row("SELECT EXISTS(SELECT 1 FROM source_connections c JOIN accounts a ON a.tenant_id=c.tenant_id AND a.id=c.account_id JOIN knowledge_collections k ON k.tenant_id=c.tenant_id AND k.id=c.collection_id WHERE c.tenant_id=?1 AND c.account_id=?2 AND c.id=?3 AND c.auth_epoch=?4 AND a.auth_epoch=c.auth_epoch AND a.recovery_required=0 AND a.must_change_password=0 AND (a.temporary_expires_at IS NULL OR a.temporary_expires_at>?7) AND c.config_revision=?5 AND c.credential_epoch=?6 AND c.state='active' AND k.connected=1 AND k.deleted=0 AND ((?8 IS NOT NULL AND c.kind='custom_http' AND c.token_hash=?8 AND c.token_expires>?7) OR (?8 IS NULL AND c.kind!='custom_http' AND c.sync_enabled=1 AND ((c.kind='slack' AND c.credential IS NOT NULL) OR (c.kind='gmail' AND EXISTS(SELECT 1 FROM mail_connections m WHERE m.tenant_id=c.tenant_id AND m.account_id=c.account_id AND m.id=c.mail_connection_id AND m.epoch=c.mail_epoch AND m.status='active'))))))",params![cap.tenant,cap.account,cap.connection,cap.auth_epoch,cap.config_revision,cap.credential_epoch,time,cap.token_hash.as_ref().map(|v|v.as_slice())],|r|r.get::<_,bool>(0))?;
    if !allowed {
        return Err(StoreError::Unauthorized);
    }
    connection(conn, &cap.tenant, &cap.connection)
}
fn lease_guard(
    conn: &Connection,
    lease: &SourceRunLease,
    allow_finished: bool,
) -> Result<SourceConnection> {
    let c = guard(conn, &lease.cap)?;
    let allowed=conn.query_row("SELECT EXISTS(SELECT 1 FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2 AND id=?3 AND generation=?4 AND nonce=?5 AND config_revision=?6 AND credential_epoch=?7 AND ((state='running' AND lease_until>?8) OR (?9 AND state!='running')))",params![lease.cap.tenant,lease.cap.connection,lease.id,lease.generation,lease.nonce,lease.cap.config_revision,lease.cap.credential_epoch,now()?,allow_finished],|r|r.get::<_,bool>(0))?;
    if !allowed {
        return Err(StoreError::Conflict);
    }
    Ok(c)
}
fn run_row(r: &rusqlite::Row<'_>, producer_report: bool) -> rusqlite::Result<SourceSyncRun> {
    Ok(SourceSyncRun {
        id: r.get(0)?,
        generation: r.get(1)?,
        state: r.get(2)?,
        started_at: r.get(3)?,
        heartbeat_at: r.get(4)?,
        finished_at: r.get(5)?,
        error: r.get(6)?,
        producer_report,
    })
}
fn active_run(conn: &Connection, cap: &SourceIngestCapability) -> Result<Option<SourceSyncRun>> {
    Ok(conn.query_row("SELECT id,generation,state,started_at,heartbeat_at,finished_at,error FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2 AND state='running'",params![cap.tenant,cap.connection],|r|run_row(r,cap.token_hash.is_some())).optional()?)
}
fn unknown_runs(conn: &Connection, tenant: &str, id: &str) -> Result<()> {
    conn.execute("UPDATE source_sync_runs SET state='unknown',error='lease_invalidated',finished_at=?3 WHERE tenant_id=?1 AND connection_id=?2 AND state='running'",params![tenant,id,now()?])?;
    Ok(())
}
pub(crate) fn restart(conn: &Connection) -> Result<()> {
    let time = now()?;
    conn.execute("UPDATE source_connections SET next_due=MAX(next_due,?1+60),error='outcome_unknown' WHERE EXISTS(SELECT 1 FROM source_sync_runs r WHERE r.tenant_id=source_connections.tenant_id AND r.connection_id=source_connections.id AND r.state='running')",[time])?;
    conn.execute("UPDATE source_sync_runs SET state='unknown',error='outcome_unknown',finished_at=?1 WHERE state='running'",[time])?;
    Ok(())
}
pub(crate) fn revoke_for_journal(conn: &Connection, tenant: &str, id: &str) -> Result<()> {
    conn.execute("UPDATE source_connections SET credential_epoch=credential_epoch+1,token_hash=NULL,token_expires=NULL,credential=NULL,state=CASE WHEN kind='gmail' THEN 'needs_reconnect' ELSE state END WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;
    unknown_runs(conn, tenant, id)
}
pub(crate) fn provenance(
    conn: &Connection,
    tenant: &str,
    id: &str,
    revision: i64,
) -> Result<Option<SourceProvenance>> {
    let row=conn.query_row("SELECT p.connection_id,c.name,c.kind,c.scope,s.external_id,v.version,p.request_id,p.run_id,p.source_revision,p.received_at,p.thread_id,p.parent_id FROM source_provenance p JOIN source_connections c ON c.tenant_id=p.tenant_id AND c.id=p.connection_id JOIN knowledge_sources s ON s.tenant_id=p.tenant_id AND s.id=p.source_id JOIN knowledge_source_versions v ON v.tenant_id=p.tenant_id AND v.source_id=p.source_id AND v.revision=p.source_revision WHERE p.tenant_id=?1 AND p.source_id=?2 AND p.source_revision=?3",params![tenant,id,revision],|r|Ok((SourceProvenance{connection_id:r.get(0)?,connection_name:r.get(1)?,kind:SourceConnectionKind::CustomHttp,scope:r.get(3)?,external_id:r.get(4)?,external_version:r.get(5)?,request_id:r.get(6)?,run_id:r.get(7)?,source_revision:r.get(8)?,received_at:r.get(9)?,thread_id:r.get(10)?,parent_id:r.get(11)?},r.get::<_,String>(2)?))).optional()?;
    row.map(|(mut p, k)| {
        p.kind = parse_kind(&k)?;
        Ok(p)
    })
    .transpose()
}
/// Source revisions pin their compact receipts. Unreferenced delivery history
/// remains bounded independently, so a quiet source keeps its original trace
/// after many no-change polls. Runs contain only fixed codes, epochs and times;
/// preserve those rows for retained receipts/provenance plus the recent run view.
/// The source/revision admission limits also bound this pinned metadata.
fn prune_source_history(
    conn: &Connection,
    tenant: &str,
    connection: &str,
    receipt_limit: i64,
    run_limit: i64,
) -> Result<()> {
    conn.execute(
        "DELETE FROM source_batch_receipts
         WHERE tenant_id=?1 AND connection_id=?2
           AND request_id NOT IN (
             SELECT request_id FROM source_provenance WHERE tenant_id=?1 AND connection_id=?2)
           AND request_id NOT IN (
             SELECT request_id FROM source_batch_receipts
             WHERE tenant_id=?1 AND connection_id=?2
               AND request_id NOT IN (
                 SELECT request_id FROM source_provenance WHERE tenant_id=?1 AND connection_id=?2)
             ORDER BY sequence DESC LIMIT ?3)",
        params![tenant, connection, receipt_limit],
    )?;
    conn.execute(
        "DELETE FROM source_sync_runs
         WHERE tenant_id=?1 AND connection_id=?2 AND state!='running'
           AND id NOT IN (
             SELECT run_id FROM source_batch_receipts WHERE tenant_id=?1 AND connection_id=?2)
           AND id NOT IN (
             SELECT run_id FROM source_provenance WHERE tenant_id=?1 AND connection_id=?2)
           AND id NOT IN (
             SELECT id FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2
             ORDER BY generation DESC LIMIT ?3)",
        params![tenant, connection, run_limit],
    )?;
    Ok(())
}
fn slack_aad(cap: &SourceIngestCapability) -> Result<Vec<u8>> {
    serde_json::to_vec(&(
        "moosuri/source/slack/v1",
        &cap.tenant,
        &cap.account,
        &cap.connection,
        cap.credential_epoch,
    ))
    .map_err(|_| StoreError::InvalidInput)
}
impl Store {
    pub async fn create_source_connection(
        &self,
        user: &UserContext,
        input: CreateSourceConnection,
    ) -> Result<SourceConnection> {
        let user = user.clone();
        self.call(move|db|{
            let user=validate(&db.conn,&user,true,false)?; label(&input.name,200)?;label(&input.scope,4096)?;label(&input.remote_account,256)?;
            if !(60..=86400).contains(&input.poll_interval_seconds) || (input.kind!=SourceConnectionKind::Gmail && input.mail_connection_id.is_some()) {return Err(StoreError::InvalidInput)}
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mail_epoch=if input.kind==SourceConnectionKind::Gmail {
                let id=input.mail_connection_id.as_ref().ok_or(StoreError::InvalidInput)?;
                let m=mail::connection(&tx,&user,id)?;
                if m.status!="active" || m.email!=input.remote_account {return Err(StoreError::Conflict)} Some(m.epoch)
            } else {None};
            if tx.query_row("SELECT count(*) FROM knowledge_collections WHERE tenant_id=?1",[&user.tenant.0],|r|r.get::<_,i64>(0))?>=32{return Err(StoreError::Conflict)}
            let id=Uuid::new_v4().to_string();let collection_id=Uuid::new_v4().to_string();
            tx.execute("INSERT INTO knowledge_collections VALUES(?1,?2,?3,?4,1,0,1,0)",params![user.tenant.0,collection_id,input.name.trim(),format!("connection:{id}")])?;
            tx.execute("INSERT INTO source_connections(tenant_id,id,account_id,collection_id,kind,name,scope,remote_account,mail_connection_id,mail_epoch,auth_epoch,sync_enabled,poll_interval) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![user.tenant.0,id,user.account.id,collection_id,kind(input.kind),input.name.trim(),input.scope,input.remote_account,input.mail_connection_id,mail_epoch,user.epoch,input.sync_enabled,input.poll_interval_seconds])?;
            let c=connection(&tx,&user.tenant.0,&id)?; tx.commit()?;Ok(c)
        }).await
    }
    pub async fn source_connections(&self, user: &UserContext) -> Result<Vec<SourceConnection>> {
        let user = user.clone();
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;
            let ids=db.conn.prepare("SELECT id FROM source_connections WHERE tenant_id=?1 AND account_id=?2 ORDER BY rowid DESC LIMIT 32")?.query_map(params![user.tenant.0,user.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            ids.iter().map(|id|connection(&db.conn,&user.tenant.0,id)).collect()
        }).await
    }
    pub async fn source_connection(
        &self,
        user: &UserContext,
        id: String,
    ) -> Result<SourceConnection> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            owned(&db.conn, &user, &id)
        })
        .await
    }
    pub async fn configure_source_connection(
        &self,
        user: &UserContext,
        id: String,
        expected_config_revision: i64,
        name: String,
        sync_enabled: bool,
        poll_interval_seconds: u32,
    ) -> Result<SourceConnection> {
        let user = user.clone();
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;let c=owned(&db.conn,&user,&id)?;label(&name,200)?;
            if c.config_revision!=expected_config_revision || matches!(c.state.as_str(),"disconnected"|"deleted") || !(60..=86400).contains(&poll_interval_seconds){return Err(StoreError::Conflict)}
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute("UPDATE source_connections SET name=?3,sync_enabled=?4,poll_interval=?5,config_revision=config_revision+1,next_due=0 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id,name.trim(),sync_enabled,poll_interval_seconds])?;
            unknown_runs(&tx,&user.tenant.0,&id)?;let c=connection(&tx,&user.tenant.0,&id)?;tx.commit()?;Ok(c)
        }).await
    }
    pub async fn pause_source_connection(
        &self,
        user: &UserContext,
        id: String,
        expected_config_revision: i64,
        paused: bool,
    ) -> Result<SourceConnection> {
        let user = user.clone();
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;let c=owned(&db.conn,&user,&id)?;
            if c.config_revision!=expected_config_revision || matches!(c.state.as_str(),"disconnected"|"deleted") || c.state=="needs_reconnect"{return Err(StoreError::Conflict)}
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute("UPDATE source_connections SET state=?3,config_revision=config_revision+1,next_due=0 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id,if paused {"paused"}else{"active"}])?;
            unknown_runs(&tx,&user.tenant.0,&id)?;let c=connection(&tx,&user.tenant.0,&id)?;tx.commit()?;Ok(c)
        }).await
    }
    pub async fn disconnect_source_connection(
        &self,
        user: &UserContext,
        id: String,
        expected_config_revision: i64,
        mode: DisconnectMode,
    ) -> Result<SourceConnection> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let c = owned(&db.conn, &user, &id)?;
            if c.config_revision != expected_config_revision {
                return Err(StoreError::Conflict);
            }
            // Existing collection journal is the durable late-commit fence, including
            // recovery from a pre-disconnect database backup.
            knowledge::destructive::destructive(
                db,
                &user.tenant.0,
                &c.collection_id,
                if mode == DisconnectMode::Keep { 4 } else { 3 },
            )?;
            connection(&db.conn, &user.tenant.0, &id)
        })
        .await
    }
    pub async fn issue_source_token(
        &self,
        user: &UserContext,
        id: String,
        ttl_seconds: i64,
    ) -> Result<SourceTokenGrant> {
        let user = user.clone();
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;let c=owned(&db.conn,&user,&id)?;
            if c.kind!=SourceConnectionKind::CustomHttp || matches!(c.state.as_str(),"disconnected"|"deleted") || !(60..=90*86400).contains(&ttl_seconds){return Err(StoreError::InvalidInput)}
            knowledge::destructive::destructive(db,&user.tenant.0,&id,6)?;
            let token=random_token()?;let expires_at=now()?.checked_add(ttl_seconds).ok_or(StoreError::InvalidInput)?;
            db.conn.execute("UPDATE source_connections SET token_hash=?3,token_expires=?4,auth_epoch=?5 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id,digest(&token).as_slice(),expires_at,user.epoch])?;
            Ok(SourceTokenGrant{token,expires_at})
        }).await
    }
    pub async fn revoke_source_token(&self, user: &UserContext, id: String) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            owned(&db.conn, &user, &id)?;
            knowledge::destructive::destructive(db, &user.tenant.0, &id, 6)
        })
        .await
    }
    pub async fn set_source_slack_token(
        &self,
        user: &UserContext,
        id: String,
        token: SecretString,
    ) -> Result<SourceConnection> {
        let user = user.clone();
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;let c=owned(&db.conn,&user,&id)?;
            if c.kind!=SourceConnectionKind::Slack || matches!(c.state.as_str(),"disconnected"|"deleted"){return Err(StoreError::Conflict)}
            knowledge::destructive::destructive(db,&user.tenant.0,&id,6)?;
            let (a,config,epoch)=db.conn.query_row("SELECT auth_epoch,config_revision,credential_epoch FROM source_connections WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            let cap=SourceIngestCapability{tenant:user.tenant.0.clone(),account:user.account.id.clone(),connection:id.clone(),auth_epoch:a,config_revision:config,credential_epoch:epoch,token_hash:None};
            let cipher=providers::encrypt(&db.provider_key,token.expose().as_bytes(),&slack_aad(&cap)?)?;
            db.conn.execute("UPDATE source_connections SET credential=?3,auth_epoch=?4,state='active',error=NULL,next_due=0 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id,cipher,user.epoch])?;
            connection(&db.conn,&user.tenant.0,&id)
        }).await
    }
    /// Rebind refreshed personal OAuth to the same immutable remote account and
    /// source scope. A different remote account always requires a new connection.
    pub async fn reconnect_source_gmail(
        &self,
        user: &UserContext,
        id: String,
        expected_config_revision: i64,
        mail_connection_id: String,
    ) -> Result<SourceConnection> {
        let user = user.clone();
        self.call(move|db|{
            let user=validate(&db.conn,&user,true,false)?;
            let c=owned(&db.conn,&user,&id)?;
            if c.kind!=SourceConnectionKind::Gmail || matches!(c.state.as_str(),"disconnected"|"deleted") || c.config_revision!=expected_config_revision {return Err(StoreError::Conflict)}
            let mail=mail::connection(&db.conn,&user,&mail_connection_id)?;
            if mail.status!="active" || mail.email!=c.remote_account {return Err(StoreError::Conflict)}
            knowledge::destructive::destructive(db,&user.tenant.0,&id,6)?;
            db.conn.execute("UPDATE source_connections SET mail_connection_id=?3,mail_epoch=?4,config_revision=config_revision+1,auth_epoch=?5,state='active',error=NULL,next_due=0 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id,mail.id,mail.epoch,user.epoch])?;
            connection(&db.conn,&user.tenant.0,&id)
        }).await
    }
    /// Explicitly restart provider traversal in the same immutable scope. Local
    /// sequence, sources and withdrawals are retained; stale pages lose their lease.
    pub async fn reset_source_checkpoint(
        &self,
        user: &UserContext,
        id: String,
        expected_config_revision: i64,
    ) -> Result<SourceConnection> {
        let user = user.clone();
        self.call(move|db|{
            let user=validate(&db.conn,&user,true,false)?;
            let c=owned(&db.conn,&user,&id)?;
            if matches!(c.state.as_str(),"disconnected"|"deleted") || c.config_revision!=expected_config_revision {return Err(StoreError::Conflict)}
            let (auth,hash,expires,cipher,old_state):ResetCredentialRow=db.conn.query_row("SELECT auth_epoch,token_hash,token_expires,credential,state FROM source_connections WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            let mut cap=SourceIngestCapability{tenant:user.tenant.0.clone(),account:user.account.id.clone(),connection:id.clone(),auth_epoch:auth,config_revision:c.config_revision,credential_epoch:c.credential_epoch,token_hash:None};
            let slack_plain=if c.kind==SourceConnectionKind::Slack {cipher.as_ref().map(|bytes|providers::decrypt(&db.provider_key,bytes,&slack_aad(&cap)?)).transpose()?}else{None};
            let restored_state=if old_state=="needs_reconnect" && c.error.as_deref()==Some("checkpoint_invalid") && c.credential_present {"active".to_owned()}else{old_state};
            knowledge::destructive::destructive(db,&user.tenant.0,&id,6)?;
            cap.credential_epoch+=1;
            let cipher=slack_plain.map(|plain|providers::encrypt(&db.provider_key,&plain,&slack_aad(&cap)?)).transpose()?;
            db.conn.execute("UPDATE source_connections SET checkpoint=NULL,config_revision=config_revision+1,token_hash=?3,token_expires=?4,credential=?5,state=?6,error=NULL,next_due=0,failure_count=0 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id,hash,expires,cipher,restored_state])?;
            connection(&db.conn,&user.tenant.0,&id)
        }).await
    }
    pub async fn authenticate_source_token(&self, token: String) -> Result<SourceIngestCapability> {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(StoreError::Unauthorized);
        }
        let hash = digest(&token);
        let _token = Zeroizing::new(token);
        self.call(move |db| {
            let row: Option<(String, String, String)> = db
                .conn
                .query_row(
                    "SELECT tenant_id,account_id,id FROM source_connections WHERE token_hash=?1",
                    [hash.as_slice()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let (t, a, c) = row.ok_or(StoreError::Unauthorized)?;
            capability(&db.conn, t, a, c, Some(hash))
        })
        .await
    }
    pub async fn source_ingest_status(
        &self,
        cap: &SourceIngestCapability,
    ) -> Result<SourceIngestStatus> {
        let cap = cap.clone();
        self.call(move |db| {
            let c = guard(&db.conn, &cap)?;
            let checkpoint = db.conn.query_row(
                "SELECT checkpoint FROM source_connections WHERE tenant_id=?1 AND id=?2",
                params![cap.tenant, cap.connection],
                |r| r.get(0),
            )?;
            Ok(SourceIngestStatus {
                connection_id: c.id,
                config_revision: c.config_revision,
                local_sequence: c.local_sequence,
                checkpoint,
                active_run: active_run(&db.conn, &cap)?,
            })
        })
        .await
    }
    pub async fn begin_source_run(&self, cap: &SourceIngestCapability) -> Result<SourceRunLease> {
        let cap = cap.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;guard(&tx,&cap)?;let time=now()?;
            tx.execute("UPDATE source_sync_runs SET state='unknown',error='lease_expired',finished_at=?3 WHERE tenant_id=?1 AND connection_id=?2 AND state='running' AND lease_until<=?3",params![cap.tenant,cap.connection,time])?;
            if active_run(&tx,&cap)?.is_some(){return Err(StoreError::Conflict)}
            tx.execute("UPDATE source_connections SET run_generation=run_generation+1,last_heartbeat=?3 WHERE tenant_id=?1 AND id=?2",params![cap.tenant,cap.connection,time])?;
            let generation=tx.query_row("SELECT run_generation FROM source_connections WHERE tenant_id=?1 AND id=?2",params![cap.tenant,cap.connection],|r|r.get(0))?;
            let id=Uuid::new_v4().to_string();let nonce=digest(&random_token()?).to_vec();
            tx.execute("INSERT INTO source_sync_runs(tenant_id,connection_id,id,generation,config_revision,credential_epoch,nonce,state,started_at,heartbeat_at,lease_until) VALUES(?1,?2,?3,?4,?5,?6,?7,'running',?8,?8,?9)",params![cap.tenant,cap.connection,id,generation,cap.config_revision,cap.credential_epoch,nonce,time,time+LEASE_SECONDS])?;
            prune_source_history(&tx,&cap.tenant,&cap.connection,RECEIPT_LIMIT,RUN_LIMIT)?;
            tx.commit()?;Ok(SourceRunLease{cap,id,generation,nonce})
        }).await
    }
    pub async fn source_run_lease(
        &self,
        cap: &SourceIngestCapability,
        id: String,
        generation: i64,
    ) -> Result<SourceRunLease> {
        let cap = cap.clone();
        self.call(move|db|{guard(&db.conn,&cap)?;
            let nonce=db.conn.query_row("SELECT nonce FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2 AND id=?3 AND generation=?4",params![cap.tenant,cap.connection,id,generation],|r|r.get(0)).optional()?.ok_or(StoreError::Conflict)?;
            let lease=SourceRunLease{cap,id,generation,nonce};lease_guard(&db.conn,&lease,true)?;Ok(lease)
        }).await
    }
    pub async fn source_heartbeat(&self, lease: &SourceRunLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;lease_guard(&tx,&lease,false)?;let time=now()?;
            tx.execute("UPDATE source_sync_runs SET heartbeat_at=?4,lease_until=?5 WHERE tenant_id=?1 AND connection_id=?2 AND id=?3",params![lease.cap.tenant,lease.cap.connection,lease.id,time,time+LEASE_SECONDS])?;
            tx.execute("UPDATE source_connections SET last_heartbeat=?3 WHERE tenant_id=?1 AND id=?2",params![lease.cap.tenant,lease.cap.connection,time])?;tx.commit()?;Ok(())
        }).await
    }
    pub async fn commit_source_batch(
        &self,
        lease: &SourceRunLease,
        batch: SourceBatch,
    ) -> Result<SourceBatchReceipt> {
        let lease = lease.clone();
        self.call(move|db|{
            if batch.protocol_version!=1 || batch.complete_snapshot || batch.expected_sequence<0 || batch.config_revision!=lease.cap.config_revision || batch.run_id!=lease.id || batch.generation!=lease.generation || Uuid::parse_str(&batch.request_id).is_err() || batch.tombstones.len()>32 {return Err(StoreError::InvalidInput)}
            knowledge::sources::validate_entries(&batch.entries,true)?;checkpoint(&batch.checkpoint_before)?;checkpoint(&batch.checkpoint_after)?;
            let encoded=json(&batch)?;if encoded.len()>512*1024{return Err(StoreError::InvalidInput)}
            let digest: [u8;32]=Sha256::digest(encoded.as_bytes()).into();
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Even an exact replay rechecks the current account, connection, grant
            // and collection. Revocation cannot be bypassed through receipt lookup.
            let c=guard(&tx,&lease.cap)?;
            let previous:Option<(Vec<u8>,String)>=tx.query_row("SELECT digest,receipt FROM source_batch_receipts WHERE tenant_id=?1 AND connection_id=?2 AND request_id=?3",params![lease.cap.tenant,lease.cap.connection,batch.request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            if let Some((old,receipt))=previous {if old!=digest{return Err(StoreError::Conflict)}return parse(&receipt)}
            lease_guard(&tx,&lease,false)?;
            let old_checkpoint:Option<String>=tx.query_row("SELECT checkpoint FROM source_connections WHERE tenant_id=?1 AND id=?2",params![lease.cap.tenant,lease.cap.connection],|r|r.get(0))?;
            if c.local_sequence!=batch.expected_sequence || old_checkpoint!=batch.checkpoint_before{return Err(StoreError::Conflict)}
            let page_count:i64=tx.query_row("SELECT page_count FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2 AND id=?3",params![lease.cap.tenant,lease.cap.connection,lease.id],|r|r.get(0))?;
            if page_count>=256{return Err(StoreError::BudgetExhausted)}
            let names=batch.entries.iter().map(|e|e.external_id.as_str()).collect::<std::collections::BTreeSet<_>>();
            let mut metadata=std::collections::BTreeMap::new();
            if batch.metadata.len()>32 {return Err(StoreError::InvalidInput)}
            for value in &batch.metadata {
                if !names.contains(value.external_id.as_str()) || metadata.insert(value.external_id.as_str(),value).is_some() {return Err(StoreError::InvalidInput)}
                for id in [&value.thread_id,&value.parent_id].into_iter().flatten(){label(id,128)?;}
            }
            let mut absent_deletes=0u32;
            let mut deleted=Vec::new();let mut seen=std::collections::BTreeSet::new();
            for external in &batch.tombstones {
                label(external,128)?;
                if names.contains(external.as_str()) || !seen.insert(external){return Err(StoreError::InvalidInput)}
                // Deletes outside the imported scope are explicit no-ops. They do
                // not create a permanent suppression. Producer page order is fenced
                // by expected sequence/checkpoint; known deleted identities persist.
                let source:Option<(String,bool)>=tx.query_row("SELECT id,deleted FROM knowledge_sources WHERE tenant_id=?1 AND collection_id=?2 AND external_id=?3",params![lease.cap.tenant,c.collection_id,external],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
                let Some((id,already_deleted))=source else {absent_deletes+=1;continue;};
                if !already_deleted {deleted.push(id)}
            }
            drop(names);
            let total=batch.entries.len() as u32;
            let mut live_entries=Vec::new();let mut suppressed=0u32;
            for entry in batch.entries {
                let was_deleted:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM knowledge_sources WHERE tenant_id=?1 AND collection_id=?2 AND external_id=?3 AND deleted=1)",params![lease.cap.tenant,c.collection_id,entry.external_id],|r|r.get(0))?;
                if was_deleted {suppressed+=1;}else{live_entries.push(entry);}
            }
            // Validate every upsert (including version/CAS rules) before writing
            // irreversible deletion intents. The enclosing transaction rolls back
            // all upserts on any validation error.
            let (source_ids,unchanged_entries)=knowledge::sources::commit_entries(&tx,&lease.cap.tenant,&c.collection_id,live_entries,true)?;
            let time=now()?;let sequence=batch.expected_sequence.checked_add(1).ok_or(StoreError::Conflict)?;
            let receipt=SourceBatchReceipt{request_id:batch.request_id.clone(),run_id:lease.id.clone(),sequence,committed_at:time,accepted:total-unchanged_entries-suppressed,unchanged:unchanged_entries+absent_deletes+suppressed,source_ids};
            for source in &receipt.source_ids {
                let external:String=tx.query_row("SELECT external_id FROM knowledge_sources WHERE tenant_id=?1 AND id=?2",params![lease.cap.tenant,source],|r|r.get(0))?;
                let m=metadata.get(external.as_str());
                tx.execute("INSERT OR IGNORE INTO source_provenance SELECT tenant_id,id,revision,?3,?4,?5,?6,?7,?8 FROM knowledge_sources WHERE tenant_id=?1 AND id=?2",params![lease.cap.tenant,source,lease.cap.connection,batch.request_id,lease.id,time,m.and_then(|v|v.thread_id.as_deref()),m.and_then(|v|v.parent_id.as_deref())])?;
            }
            tx.execute("INSERT INTO source_batch_receipts VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![lease.cap.tenant,lease.cap.connection,batch.request_id,lease.id,digest.as_slice(),sequence,time,batch.checkpoint_before,batch.checkpoint_after,json(&receipt)?])?;
            tx.execute("UPDATE source_connections SET checkpoint=?3,last_commit=?4,last_heartbeat=?4 WHERE tenant_id=?1 AND id=?2",params![lease.cap.tenant,lease.cap.connection,batch.checkpoint_after,time])?;
            tx.execute("UPDATE knowledge_collections SET cursor=?3,revision=revision+1 WHERE tenant_id=?1 AND id=?2",params![lease.cap.tenant,c.collection_id,sequence])?;
            tx.execute("INSERT INTO knowledge_state(tenant_id,generation) VALUES(?1,1) ON CONFLICT(tenant_id) DO UPDATE SET generation=generation+1",[&lease.cap.tenant])?;
            tx.execute("UPDATE source_sync_runs SET page_count=page_count+1,heartbeat_at=?4,lease_until=?5 WHERE tenant_id=?1 AND connection_id=?2 AND id=?3",params![lease.cap.tenant,lease.cap.connection,lease.id,time,time+LEASE_SECONDS])?;
            // No fallible input validation remains beyond this boundary. A crash
            // after intent fsync withdraws sources during startup replay even if the
            // page receipt was not committed. Keep the writer poisoned on failure.
            if !deleted.is_empty(){
                let intent=destructive_intents::Effect::DeleteSources{targets:deleted};
                let prepared=db.destructive_journal.prepare(&lease.cap.tenant,intent)?;
                db.poisoned=true;
                let record=db.destructive_journal.append(prepared)?;
                destructive_intents::apply(&tx,&record)?;
            }
            // Tombstones may release provenance pins in this transaction. Prune
            // after their cascades so newly unreferenced history is also bounded.
            prune_source_history(&tx,&lease.cap.tenant,&lease.cap.connection,RECEIPT_LIMIT,RUN_LIMIT)?;
            tx.commit()?;db.poisoned=false;Ok(receipt)
        }).await
    }
    pub async fn finish_source_run(
        &self,
        lease: &SourceRunLease,
        outcome: SourceRunOutcome,
        error: Option<String>,
        retry_after_seconds: Option<u32>,
    ) -> Result<()> {
        if error.as_ref().is_some_and(|s| {
            !matches!(
                s.as_str(),
                "rate_limited"
                    | "network_error"
                    | "provider_unavailable"
                    | "authentication_required"
                    | "invalid_batch"
                    | "partial_coverage"
                    | "outcome_unknown"
                    | "checkpoint_invalid"
                    | "scope_changed"
                    | "budget_exhausted"
            )
        }) {
            return Err(StoreError::InvalidInput);
        }
        let lease = lease.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let c=lease_guard(&tx,&lease,true)?;let time=now()?;
            let state=match outcome{SourceRunOutcome::Completed=>"completed",SourceRunOutcome::Partial=>"partial",SourceRunOutcome::NoChange=>"no_change",SourceRunOutcome::Polled=>"polled",SourceRunOutcome::Failed=>"failed",SourceRunOutcome::Unknown=>"unknown",SourceRunOutcome::NeedsReconnect=>"needs_reconnect"};
            let (current_state,current_error):(String,Option<String>)=tx.query_row("SELECT state,error FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2 AND id=?3",params![lease.cap.tenant,lease.cap.connection,lease.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if current_state!="running" {return if current_state==state && current_error==error {Ok(())} else {Err(StoreError::Conflict)}}
            let completed=matches!(outcome,SourceRunOutcome::Completed|SourceRunOutcome::NoChange);
            let success=completed || outcome==SourceRunOutcome::Polled;
            let continuation=outcome==SourceRunOutcome::Partial && error.is_none();
            // A zero-page success cannot attest remote coverage. A no-change page
            // must still commit its own receipt/checkpoint through commit_source_batch.
            let pages:i64=tx.query_row("SELECT page_count FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2 AND id=?3",params![lease.cap.tenant,lease.cap.connection,lease.id],|r|r.get(0))?;
            if success && pages==0{return Err(StoreError::Conflict)}
            let failures:i64=tx.query_row("SELECT failure_count FROM source_connections WHERE tenant_id=?1 AND id=?2",params![lease.cap.tenant,lease.cap.connection],|r|r.get(0))?;
            let failures=if success || continuation {0}else{(failures+1).min(16)};
            let delay=if success {i64::from(c.poll_interval_seconds)} else if continuation {30} else {i64::from(retry_after_seconds.unwrap_or(0).min(86400)).max((30i64.saturating_mul(1i64<<failures.min(11))).min(86400))};
            tx.execute("UPDATE source_sync_runs SET state=?4,error=?5,finished_at=?6,heartbeat_at=?6 WHERE tenant_id=?1 AND connection_id=?2 AND id=?3",params![lease.cap.tenant,lease.cap.connection,lease.id,state,error,time])?;
            tx.execute("UPDATE source_connections SET error=?3,failure_count=?4,next_due=?5,last_completed=CASE WHEN ?6 THEN ?7 ELSE last_completed END,state=CASE WHEN ?8 THEN 'needs_reconnect' ELSE state END WHERE tenant_id=?1 AND id=?2",params![lease.cap.tenant,lease.cap.connection,error,failures,time+delay,completed,time,outcome==SourceRunOutcome::NeedsReconnect])?;
            tx.commit()?;Ok(())
        }).await
    }
    pub async fn source_sync_runs(
        &self,
        user: &UserContext,
        id: String,
    ) -> Result<Vec<SourceSyncRun>> {
        let user = user.clone();
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;let c=owned(&db.conn,&user,&id)?;
            Ok(db.conn.prepare("SELECT id,generation,state,started_at,heartbeat_at,finished_at,error FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2 ORDER BY generation DESC LIMIT 64")?.query_map(params![user.tenant.0,id],|r|run_row(r,c.kind==SourceConnectionKind::CustomHttp))?.collect::<std::result::Result<Vec<_>,_>>()?)
        }).await
    }
    pub async fn source_batch_receipt(
        &self,
        user: &UserContext,
        id: String,
        request: String,
    ) -> Result<SourceBatchReceipt> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            owned(&db.conn, &user, &id)?;
            receipt(&db.conn, &user.tenant.0, &id, &request)
        })
        .await
    }
    pub async fn source_ingest_receipt(
        &self,
        cap: &SourceIngestCapability,
        request: String,
    ) -> Result<SourceBatchReceipt> {
        let cap = cap.clone();
        self.call(move |db| {
            guard(&db.conn, &cap)?;
            receipt(&db.conn, &cap.tenant, &cap.connection, &request)
        })
        .await
    }
    pub async fn trigger_source_sync(
        &self,
        user: &UserContext,
        id: String,
        expected_config_revision: i64,
    ) -> Result<SourceConnection> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let c = owned(&db.conn, &user, &id)?;
            if c.config_revision != expected_config_revision
                || c.state != "active"
                || !c.sync_enabled
                || c.kind == SourceConnectionKind::CustomHttp
            {
                return Err(StoreError::Conflict);
            }
            db.conn.execute(
                "UPDATE source_connections SET next_due=0 WHERE tenant_id=?1 AND id=?2",
                params![user.tenant.0, id],
            )?;
            connection(&db.conn, &user.tenant.0, &id)
        })
        .await
    }
    /// Returns authority bound to a real current account and an opt-in personal
    /// connection. It never fabricates a user session to bypass authentication.
    pub async fn next_source_connection(&self) -> Result<Option<SourceIngestCapability>> {
        self.call(move|db|{let time=now()?;
            // Expired leases are outcomes unknown, not successful remote work.
            db.conn.execute("UPDATE source_connections SET next_due=MAX(next_due,?1+60),error='outcome_unknown' WHERE EXISTS(SELECT 1 FROM source_sync_runs r WHERE r.tenant_id=source_connections.tenant_id AND r.connection_id=source_connections.id AND r.state='running' AND r.lease_until<=?1)",[time])?;
            db.conn.execute("UPDATE source_sync_runs SET state='unknown',error='lease_expired',finished_at=?1 WHERE state='running' AND lease_until<=?1",[time])?;
            let rows=db.conn.prepare("SELECT c.tenant_id,c.account_id,c.id FROM source_connections c JOIN accounts a ON a.tenant_id=c.tenant_id AND a.id=c.account_id JOIN knowledge_collections k ON k.tenant_id=c.tenant_id AND k.id=c.collection_id WHERE c.kind!='custom_http' AND c.sync_enabled=1 AND c.state='active' AND c.next_due<=?1 AND c.auth_epoch=a.auth_epoch AND a.recovery_required=0 AND a.must_change_password=0 AND (a.temporary_expires_at IS NULL OR a.temporary_expires_at>?1) AND k.connected=1 AND k.deleted=0 AND ((c.kind='slack' AND c.credential IS NOT NULL) OR (c.kind='gmail' AND EXISTS(SELECT 1 FROM mail_connections m WHERE m.tenant_id=c.tenant_id AND m.account_id=c.account_id AND m.id=c.mail_connection_id AND m.epoch=c.mail_epoch AND m.status='active'))) AND NOT EXISTS(SELECT 1 FROM source_sync_runs r WHERE r.tenant_id=c.tenant_id AND r.connection_id=c.id AND r.state='running') ORDER BY c.next_due,c.rowid LIMIT 32")?.query_map([time],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
            for (t,a,id) in rows {match capability(&db.conn,t,a,id,None){Ok(c)=>return Ok(Some(c)),Err(StoreError::Unauthorized)=>continue,Err(e)=>return Err(e)}} Ok(None)
        }).await
    }
    pub async fn source_runtime_binding(
        &self,
        cap: &SourceIngestCapability,
    ) -> Result<SourceRuntimeBinding> {
        let cap = cap.clone();
        self.call(move |db| {
            let c = guard(&db.conn, &cap)?;
            if cap.token_hash.is_some() {
                return Err(StoreError::Forbidden);
            }
            let (checkpoint, cipher): (Option<String>, Option<Vec<u8>>) = db.conn.query_row(
                "SELECT checkpoint,credential FROM source_connections WHERE tenant_id=?1 AND id=?2",
                params![cap.tenant, cap.connection],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let slack_token = if c.kind == SourceConnectionKind::Slack {
                let plain = providers::decrypt(
                    &db.provider_key,
                    &cipher.ok_or(StoreError::Unauthorized)?,
                    &slack_aad(&cap)?,
                )?;
                Some(
                    SecretString::new(
                        String::from_utf8(plain.to_vec()).map_err(|_| StoreError::Corrupt)?,
                    )
                    .map_err(|_| StoreError::Corrupt)?,
                )
            } else {
                None
            };
            Ok(SourceRuntimeBinding {
                connection: c,
                checkpoint,
                slack_token,
            })
        })
        .await
    }
    pub async fn source_mail_credentials(
        &self,
        cap: &SourceIngestCapability,
    ) -> Result<SourceGmailCredentials> {
        self.source_mail_credentials_inner(cap, None).await
    }
    pub async fn source_mail_credentials_with_app(
        &self,
        cap: &SourceIngestCapability,
        pin: GoogleAppPin,
    ) -> Result<SourceGmailCredentials> {
        self.source_mail_credentials_inner(cap, Some(pin)).await
    }
    async fn source_mail_credentials_inner(
        &self,
        cap: &SourceIngestCapability,
        pin: Option<GoogleAppPin>,
    ) -> Result<SourceGmailCredentials> {
        let cap = cap.clone();
        self.call(move|db|{let c=guard(&db.conn,&cap)?;if cap.token_hash.is_some() || c.kind!=SourceConnectionKind::Gmail{return Err(StoreError::Forbidden)}
            let id=c.mail_connection_id.ok_or(StoreError::Corrupt)?;
            if let Some(pin)=&pin {google_setup::guard_mail_app(&db.conn,&cap.tenant,&id,pin)?;}
            let (epoch,revision,cipher,expires):(i64,i64,Vec<u8>,i64)=db.conn.query_row("SELECT epoch,credential_revision,credential,expires_at FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND status='active'",params![cap.tenant,cap.account,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
            let plain=providers::decrypt(&db.provider_key,&cipher,&gmail_aad(&cap,&id,revision)?)?;
            let (access,refresh):(String,String)=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;
            Ok(SourceGmailCredentials{app:pin,mail_connection_id:id,mail_epoch:epoch,revision,expires_at:expires,access_token:SecretString::new(access).map_err(|_|StoreError::Corrupt)?,refresh_token:SecretString::new(refresh).map_err(|_|StoreError::Corrupt)?})
        }).await
    }
    pub async fn save_source_mail_refresh(
        &self,
        cap: &SourceIngestCapability,
        old_revision: i64,
        tokens: TokenSet,
    ) -> Result<()> {
        self.save_source_mail_refresh_inner(cap, old_revision, tokens, None)
            .await
    }
    pub async fn save_source_mail_refresh_with_app(
        &self,
        cap: &SourceIngestCapability,
        old_revision: i64,
        tokens: TokenSet,
        pin: GoogleAppPin,
    ) -> Result<()> {
        self.save_source_mail_refresh_inner(cap, old_revision, tokens, Some(pin))
            .await
    }
    async fn save_source_mail_refresh_inner(
        &self,
        cap: &SourceIngestCapability,
        old_revision: i64,
        tokens: TokenSet,
        pin: Option<GoogleAppPin>,
    ) -> Result<()> {
        let cap = cap.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let c=guard(&tx,&cap)?;
            if cap.token_hash.is_some() || c.kind!=SourceConnectionKind::Gmail{return Err(StoreError::Forbidden)}
            let id=c.mail_connection_id.ok_or(StoreError::Corrupt)?;
            if let Some(pin)=&pin {google_setup::guard_mail_app(&tx,&cap.tenant,&id,pin)?;}
            let (revision,cipher,old_scopes):(i64,Vec<u8>,String)=tx.query_row("SELECT credential_revision,credential,scopes FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND status='active'",params![cap.tenant,cap.account,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
            if revision!=old_revision{return Err(StoreError::Conflict)}
            let plain=providers::decrypt(&db.provider_key,&cipher,&gmail_aad(&cap,&id,revision)?)?;
            let (_old_access,old_refresh):(Zeroizing<String>,Zeroizing<String>)={let (a,b):(String,String)=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;(Zeroizing::new(a),Zeroizing::new(b))};
            let refresh=tokens.refresh_token.as_ref().map(|t|t.expose()).unwrap_or(&old_refresh);
            let scopes=tokens.scopes.unwrap_or(parse(&old_scopes)?);
            if moosuri_connectors::gmail::MINIMAL_SCOPES.iter().any(|s|!scopes.iter().any(|v|v==s)){return Err(StoreError::Forbidden)}
            let new_plain=Zeroizing::new(serde_json::to_vec(&(tokens.access_token.expose(),refresh)).map_err(|_|StoreError::InvalidInput)?);
            let cipher=providers::encrypt(&db.provider_key,&new_plain,&gmail_aad(&cap,&id,revision+1)?)?;
            let expires=now()?.checked_add(i64::try_from(tokens.expires_in).map_err(|_|StoreError::InvalidInput)?).ok_or(StoreError::InvalidInput)?;
            tx.execute("UPDATE mail_connections SET credential=?4,credential_revision=credential_revision+1,expires_at=?5,scopes=?6 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND credential_revision=?7",params![cap.tenant,cap.account,id,cipher,expires,json(&scopes)?,revision])?;tx.commit()?;Ok(())
        }).await
    }
}
fn gmail_aad(cap: &SourceIngestCapability, id: &str, revision: i64) -> Result<Vec<u8>> {
    serde_json::to_vec(&(
        "moosuri/gmail/v1",
        "tokens",
        &cap.tenant,
        &cap.account,
        id,
        revision,
    ))
    .map_err(|_| StoreError::InvalidInput)
}
fn receipt(conn: &Connection, tenant: &str, id: &str, request: &str) -> Result<SourceBatchReceipt> {
    let text: String=conn.query_row("SELECT receipt FROM source_batch_receipts WHERE tenant_id=?1 AND connection_id=?2 AND request_id=?3",params![tenant,id,request],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound)?;
    parse(&text)
}
#[cfg(test)]
mod tests;
