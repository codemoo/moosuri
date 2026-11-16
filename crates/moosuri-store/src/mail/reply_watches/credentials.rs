//! A watch lease grants one connection read, never a login session or send right.
use super::*;
pub struct MailReplyCredentials {
    pub access: SecretString,
    pub refresh: SecretString,
    pub revision: i64,
    pub expires_at: i64,
    pub mailbox: String,
}
fn aad(lease: &MailReplyLease, revision: i64) -> Result<Vec<u8>> {
    serde_json::to_vec(&(
        "moosuri/gmail/v1",
        "tokens",
        &lease.tenant,
        &lease.account,
        &lease.connection_id,
        revision,
    ))
    .map_err(|_| StoreError::Corrupt)
}
fn app(c: &Connection, lease: &MailReplyLease, pin: &GoogleAppPin) -> Result<()> {
    guard(c, lease)?;
    google_setup::guard_mail_app(c, &lease.tenant, &lease.connection_id, pin)?;
    let matches:bool=c.query_row("SELECT app_revision IS ?3 AND client_id=?4 FROM mail_reply_watches WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.id,pin.revision,pin.client_id],|r|r.get(0))?;
    if !matches {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
fn load(c: &Connection, key: &[u8; 32], lease: &MailReplyLease) -> Result<MailReplyCredentials> {
    let (revision,expires,cipher,mailbox,scopes):(i64,i64,Vec<u8>,String,String)=c.query_row("SELECT credential_revision,expires_at,credential,email,scopes FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![lease.tenant,lease.account,lease.connection_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let scopes: Vec<String> = parse(&scopes)?;
    if moosuri_connectors::gmail::MINIMAL_SCOPES
        .iter()
        .any(|s| !scopes.iter().any(|v| v == s))
    {
        return Err(StoreError::Forbidden);
    }
    let plain = providers::decrypt(key, &cipher, &aad(lease, revision)?)?;
    let (access, refresh): (String, String) =
        serde_json::from_slice(&plain).map_err(|_| StoreError::Corrupt)?;
    Ok(MailReplyCredentials {
        access: SecretString::new(access).map_err(|_| StoreError::Corrupt)?,
        refresh: SecretString::new(refresh).map_err(|_| StoreError::Corrupt)?,
        revision,
        expires_at: expires,
        mailbox,
    })
}
impl Store {
    pub async fn mail_reply_credentials(
        &self,
        lease: &MailReplyLease,
        pin: GoogleAppPin,
    ) -> Result<MailReplyCredentials> {
        let lease = lease.clone();
        self.call(move |db| {
            app(&db.conn, &lease, &pin)?;
            load(&db.conn, &db.provider_key, &lease)
        })
        .await
    }
    pub async fn refresh_mail_reply_credentials(
        &self,
        lease: &MailReplyLease,
        pin: GoogleAppPin,
        revision: i64,
        tokens: TokenSet,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;app(&tx,&lease,&pin)?;
            let old=load(&tx,&db.provider_key,&lease)?;if old.revision!=revision{return Err(StoreError::Conflict)}
            let scopes=if let Some(scopes)=tokens.scopes{scopes}else{parse(&tx.query_row("SELECT scopes FROM mail_connections WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.connection_id],|r|r.get::<_,String>(0))?)?};
            if moosuri_connectors::gmail::MINIMAL_SCOPES.iter().any(|s|!scopes.iter().any(|v|v==s)){return Err(StoreError::Forbidden)}
            let plain=Zeroizing::new(serde_json::to_vec(&(tokens.access_token.expose(),tokens.refresh_token.as_ref().map(|v|v.expose()).unwrap_or(old.refresh.expose()))).map_err(|_|StoreError::InvalidInput)?);
            let cipher=providers::encrypt(&db.provider_key,&plain,&aad(&lease,revision+1)?)?;
            let expires=now()?.checked_add(i64::try_from(tokens.expires_in).map_err(|_|StoreError::InvalidInput)?).ok_or(StoreError::InvalidInput)?;
            tx.execute("UPDATE mail_connections SET credential=?4,credential_revision=credential_revision+1,expires_at=?5,scopes=?6 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND credential_revision=?7",params![lease.tenant,lease.account,lease.connection_id,cipher,expires,json(&scopes)?,revision])?;
            tx.commit()?;Ok(())
        }).await
    }
}
