//! Read leases are independent of the irreversible send commitment.
use super::*;
use moosuri_connectors::gmail::{READ_SCOPE, SentObservation, reconciliation_fingerprint};

#[derive(Clone)]
pub struct MailReconcilePermit {
    tenant: String,
    account: String,
    action: String,
    nonce: Vec<u8>,
    connection_id: String,
    message_id: String,
    fingerprint: Option<String>,
    reply_thread: Option<String>,
    app: Option<GoogleAppPin>,
}
impl MailReconcilePermit {
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }
    pub fn message_id(&self) -> &str {
        &self.message_id
    }
    pub fn fingerprint(&self) -> Option<&str> {
        self.fingerprint.as_deref()
    }
    pub fn reply_thread(&self) -> Option<&str> {
        self.reply_thread.as_deref()
    }
}
/// No browser-session identity and no send authority. Never Debug/Serialize.
pub struct MailReconcileCredentials {
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    pub expires_at: i64,
    revision: i64,
    epoch: i64,
    scopes: Vec<String>,
    app: GoogleAppPin,
}
fn token_aad(p: &MailReconcilePermit, revision: i64) -> Result<Vec<u8>> {
    serde_json::to_vec(&(
        "moosuri/gmail/v1",
        "tokens",
        &p.tenant,
        &p.account,
        &p.connection_id,
        revision,
    ))
    .map_err(|_| StoreError::Corrupt)
}
pub(super) fn freeze(
    conn: &Connection,
    user: &UserContext,
    id: &str,
    nonce: &[u8],
    frozen: &FrozenMail,
) -> Result<()> {
    let (revision,digest,connection_id,connection_epoch,message_id):(i64,String,String,i64,String)=conn.query_row(
        "SELECT revision,digest,connection_id,connection_epoch,message_id FROM mail_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",
        params![user.tenant.0,user.account.id,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let fingerprint = reconciliation_fingerprint(frozen.raw_mime(), &message_id)
        .map_err(|_| StoreError::Corrupt)?;
    let time = now()?;
    conn.execute("INSERT INTO mail_send_evidence(tenant_id,action_id,account_id,revision,digest,connection_id,connection_epoch,auth_epoch,dispatch_nonce,message_id,reply_thread,committed_at,oracle_version,fingerprint) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,1,?13)",params![user.tenant.0,id,user.account.id,revision,digest,connection_id,connection_epoch,user.epoch,nonce,message_id,frozen.thread_id(),time,fingerprint])?;
    conn.execute(
        "INSERT INTO mail_reconciliations(tenant_id,action_id,next_due) VALUES(?1,?2,?3)",
        params![user.tenant.0, id, time],
    )?;
    Ok(())
}
pub(super) fn status(
    conn: &Connection,
    user: &UserContext,
    id: &str,
) -> Result<Option<MailReconciliation>> {
    Ok(conn.query_row("SELECT CASE WHEN a.state='sent' AND r.provider_message IS NOT NULL AND (r.provider_message!=a.provider_message OR r.provider_thread!=a.provider_thread) THEN 'receipt_conflict' ELSE r.status END,r.last_checked_at,r.candidate_count,r.automatic_attempts,r.manual_after,r.last_result FROM mail_reconciliations r JOIN mail_actions a ON a.tenant_id=r.tenant_id AND a.id=r.action_id WHERE r.tenant_id=?1 AND r.action_id=?2 AND a.account_id=?3",params![user.tenant.0,id,user.account.id],|r|Ok(MailReconciliation{status:r.get(0)?,last_checked_at:r.get(1)?,candidate_count:r.get(2)?,automatic_attempts:r.get(3)?,manual_after:r.get(4)?,last_result:r.get(5)?})).optional()?)
}
// All authority pins are reloaded; a permit is only an unforgeable lease carrier.
fn guard(conn: &Connection, p: &MailReconcilePermit) -> Result<()> {
    let current=conn.query_row("SELECT EXISTS(SELECT 1 FROM mail_send_evidence e JOIN mail_actions a ON a.tenant_id=e.tenant_id AND a.id=e.action_id JOIN accounts u ON u.tenant_id=e.tenant_id AND u.id=e.account_id JOIN mail_connections c ON c.tenant_id=e.tenant_id AND c.account_id=e.account_id AND c.id=e.connection_id JOIN mail_reconciliations r ON r.tenant_id=e.tenant_id AND r.action_id=e.action_id WHERE e.tenant_id=?1 AND e.action_id=?2 AND e.account_id=?3 AND e.connection_id=?4 AND e.withdrawn=0 AND e.dispatch_nonce=a.dispatch_nonce AND e.revision=a.revision AND e.digest=a.digest AND e.auth_epoch=u.auth_epoch AND a.state='outcome_unknown' AND u.status='active' AND u.recovery_required=0 AND u.must_change_password=0 AND (u.temporary_expires_at IS NULL OR u.temporary_expires_at>?6) AND c.status='active' AND c.epoch=e.connection_epoch AND r.quarantined=0 AND r.lease_nonce=?5 AND r.lease_until>?6)",params![p.tenant,p.action,p.account,p.connection_id,p.nonce,now()?],|r|r.get::<_,bool>(0))?;
    if !current {
        return Err(StoreError::Conflict);
    }
    if let Some(app) = &p.app {
        google_setup::guard_mail_app(conn, &p.tenant, &p.connection_id, app)?;
    }
    let scopes: String = conn.query_row(
        "SELECT scopes FROM mail_connections WHERE tenant_id=?1 AND id=?2",
        params![p.tenant, p.connection_id],
        |r| r.get(0),
    )?;
    if !parse::<Vec<String>>(&scopes)?
        .iter()
        .any(|s| s == READ_SCOPE)
    {
        return Err(StoreError::Forbidden);
    }
    Ok(())
}
fn claim(
    conn: &Connection,
    tenant: String,
    id: String,
    manual: bool,
) -> Result<MailReconcilePermit> {
    let (account,connection_id,message_id,fingerprint,reply_thread,attempts,committed,manual_after):(String,String,String,Option<String>,Option<String>,i64,i64,i64)=conn.query_row("SELECT e.account_id,e.connection_id,e.message_id,e.fingerprint,e.reply_thread,r.automatic_attempts,e.committed_at,r.manual_after FROM mail_send_evidence e JOIN mail_reconciliations r ON r.tenant_id=e.tenant_id AND r.action_id=e.action_id WHERE e.tenant_id=?1 AND e.action_id=?2",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)))?;
    let time = now()?;
    if manual && manual_after > time {
        return Err(StoreError::Busy);
    }
    if !manual && attempts >= 4 {
        return Err(StoreError::Conflict);
    }
    let nonce = hash(&random()?);
    if conn.execute("UPDATE mail_reconciliations SET lease_nonce=?3,lease_until=?4 WHERE tenant_id=?1 AND action_id=?2 AND (lease_until IS NULL OR lease_until<=?5) AND quarantined=0",params![tenant,id,nonce,time+90,time])?!=1 {return Err(StoreError::Busy)}
    let p = MailReconcilePermit {
        tenant,
        account,
        action: id,
        nonce,
        connection_id,
        message_id,
        fingerprint,
        reply_thread,
        app: None,
    };
    guard(conn, &p)?;
    let next = if manual {
        None
    } else {
        [
            Some(committed + 15),
            Some(committed + 120),
            Some(committed + 600),
            None,
        ][attempts as usize]
    };
    if manual {
        conn.execute(
            "UPDATE mail_reconciliations SET manual_after=?3 WHERE tenant_id=?1 AND action_id=?2",
            params![p.tenant, p.action, time + 60],
        )?;
    } else {
        conn.execute("UPDATE mail_reconciliations SET automatic_attempts=automatic_attempts+1,next_due=?3 WHERE tenant_id=?1 AND action_id=?2",params![p.tenant,p.action,next])?;
    }
    Ok(p)
}
impl Store {
    /// Pin one current application configuration for the whole read lease, including its final CAS.
    pub async fn pin_mail_reconciliation(
        &self,
        mut p: MailReconcilePermit,
        app: GoogleAppPin,
    ) -> Result<MailReconcilePermit> {
        self.call(move |db| {
            guard(&db.conn, &p)?;
            google_setup::guard_mail_app(&db.conn, &p.tenant, &p.connection_id, &app)?;
            p.app = Some(app);
            Ok(p)
        })
        .await
    }
    /// User-triggered read attempt: authenticated, owner-scoped and rate limited.
    pub async fn claim_mail_reconciliation(
        &self,
        user: &UserContext,
        id: String,
    ) -> Result<MailReconcilePermit> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            action(&db.conn, &user, &id)?;
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let p = claim(&tx, user.tenant.0, id, true)?;
            tx.commit()?;
            Ok(p)
        })
        .await
    }
    pub async fn next_mail_reconciliation(&self) -> Result<Option<MailReconcilePermit>> {
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let ids=tx.prepare("SELECT r.tenant_id,r.action_id FROM mail_reconciliations r JOIN mail_actions a ON a.tenant_id=r.tenant_id AND a.id=r.action_id WHERE r.quarantined=0 AND r.next_due<=?1 AND r.automatic_attempts<4 AND (r.lease_until IS NULL OR r.lease_until<=?1) AND a.state='outcome_unknown' ORDER BY r.next_due LIMIT 16")?.query_map([now()?],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
            for (tenant,id) in ids {
                // Savepoint rolls back a rejected lease while allowing other owners.
                tx.execute_batch("SAVEPOINT mail_lookup")?;
                match claim(&tx,tenant.clone(),id.clone(),false) {
                    Ok(p)=>{tx.execute_batch("RELEASE mail_lookup")?;tx.commit()?;return Ok(Some(p))},
                    Err(StoreError::Conflict|StoreError::Forbidden)=>{
                        tx.execute_batch("ROLLBACK TO mail_lookup; RELEASE mail_lookup")?;
                        // This original automatic authority is no longer valid.
                        // Retire only its due pointer so at most this bounded
                        // batch can delay another owner. Never revive it on
                        // restart or spend a network-attempt budget scanning it.
                        // Manual checks still require their unchanged guards.
                        tx.execute("UPDATE mail_reconciliations SET next_due=NULL WHERE tenant_id=?1 AND action_id=?2 AND quarantined=0",params![tenant,id])?;
                    },
                    Err(StoreError::Busy)=>{
                        // An occupied/transient lease is not revoked authority.
                        // Roll it back without retiring due work or its budget.
                        tx.execute_batch("ROLLBACK TO mail_lookup; RELEASE mail_lookup")?;
                    },
                    Err(e)=>return Err(e),
                }
            }
            tx.commit()?;Ok(None)
        }).await
    }
    pub async fn mail_reconcile_credentials(
        &self,
        p: &MailReconcilePermit,
        app: GoogleAppPin,
    ) -> Result<MailReconcileCredentials> {
        let p = p.clone();
        self.call(move|db|{
            guard(&db.conn,&p)?;
            google_setup::guard_mail_app(&db.conn,&p.tenant,&p.connection_id,&app)?;
            let (revision,cipher,expires_at,epoch,scopes):(i64,Vec<u8>,i64,i64,String)=db.conn.query_row("SELECT credential_revision,credential,expires_at,epoch,scopes FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![p.tenant,p.account,p.connection_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            let plain=providers::decrypt(&db.provider_key,&cipher,&token_aad(&p,revision)?)?;
            let (access,refresh):(String,String)=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;
            Ok(MailReconcileCredentials{access_token:SecretString::new(access).map_err(|_|StoreError::Corrupt)?,refresh_token:SecretString::new(refresh).map_err(|_|StoreError::Corrupt)?,expires_at,revision,epoch,scopes:parse(&scopes)?,app})
        }).await
    }
    pub async fn save_mail_reconcile_refresh(
        &self,
        p: &MailReconcilePermit,
        old: &MailReconcileCredentials,
        tokens: TokenSet,
    ) -> Result<()> {
        let p = p.clone();
        let revision = old.revision;
        let epoch = old.epoch;
        let app = old.app.clone();
        let scopes = tokens.scopes.unwrap_or_else(|| old.scopes.clone());
        let refresh = tokens
            .refresh_token
            .map(|r| r.expose().to_owned())
            .unwrap_or_else(|| old.refresh_token.expose().into());
        let plain = Zeroizing::new(
            serde_json::to_vec(&(tokens.access_token.expose(), refresh))
                .map_err(|_| StoreError::InvalidInput)?,
        );
        self.call(move|db|{
            guard(&db.conn,&p)?;google_setup::guard_mail_app(&db.conn,&p.tenant,&p.connection_id,&app)?;
            if !scopes.iter().any(|s|s==READ_SCOPE) {return Err(StoreError::Forbidden)}
            let cipher=providers::encrypt(&db.provider_key,&plain,&token_aad(&p,revision+1)?)?;
            if db.conn.execute("UPDATE mail_connections SET credential=?4,credential_revision=credential_revision+1,expires_at=?5,scopes=?6 WHERE tenant_id=?1 AND id=?2 AND epoch=?3 AND credential_revision=?7 AND status='active'",params![p.tenant,p.connection_id,epoch,cipher,now()?+tokens.expires_in as i64,json(&scopes)?,revision])?!=1 {return Err(StoreError::Conflict)}
            Ok(())
        }).await
    }
    pub async fn finish_mail_reconciliation(
        &self,
        p: &MailReconcilePermit,
        observation: Option<SentObservation>,
    ) -> Result<()> {
        let p = p.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            guard(&tx,&p)?;
            let (status,count,message,thread)=match observation {
                Some(SentObservation::Matching{message_id,thread_id})=>("matching_sent_observed",1,Some(message_id),Some(thread_id)),
                Some(SentObservation::Unresolved{status,candidates})=>(status,candidates,None,None),
                None=>("lookup_unavailable",0,None,None),
            };
            // Keep the first exact-match observation and at most one conflicting
            // provider identity. Later absence cannot erase observed evidence.
            tx.execute("UPDATE mail_reconciliations SET status='lookup_conflict',conflict_message=COALESCE(conflict_message,?4),conflict_thread=COALESCE(conflict_thread,?5) WHERE tenant_id=?1 AND action_id=?2 AND lease_nonce=?3 AND provider_message IS NOT NULL AND ?4 IS NOT NULL AND (provider_message!=?4 OR provider_thread!=?5)",params![p.tenant,p.action,p.nonce,message,thread])?;
            // Observed matches are sticky; subsequent indexing absence cannot erase one.
            tx.execute("UPDATE mail_reconciliations SET status=CASE WHEN provider_message IS NULL THEN ?4 ELSE status END,last_checked_at=?5,last_result=?4,candidate_count=?6,provider_message=COALESCE(provider_message,?7),provider_thread=COALESCE(provider_thread,?8),lease_nonce=NULL,lease_until=NULL WHERE tenant_id=?1 AND action_id=?2 AND lease_nonce=?3",params![p.tenant,p.action,p.nonce,status,now()?,count,message,thread])?;
            tx.execute("UPDATE mail_actions SET version=version+1 WHERE tenant_id=?1 AND id=?2",params![p.tenant,p.action])?;
            tx.commit()?;Ok(())
        }).await
    }
}
