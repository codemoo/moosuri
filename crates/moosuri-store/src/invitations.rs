//! Explicit admin invitation previews. Payload never enters chat/receipt metadata.
use super::*;
use moosuri_connectors::gmail::{FrozenMail, MailContent};
use moosuri_core::{InvitationDelivery, InvitationPreviewInput};
pub struct InvitationDispatch {
    pub id: String,
    pub frozen: FrozenMail,
    pub sender_pin: String,
    user: UserContext,
}
fn aad(id: &str) -> Vec<u8> {
    format!("moosuri/invitation/v1/{id}").into_bytes()
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(crate) fn restart(c: &Connection) -> Result<()> {
    c.execute("UPDATE invitation_deliveries SET state='delivery_unknown',payload=NULL WHERE state='dispatch_committed'",[])?;
    refresh(c)
}
fn refresh(c: &Connection) -> Result<()> {
    c.execute("UPDATE invitation_deliveries SET state=CASE WHEN EXISTS(SELECT 1 FROM accounts a WHERE a.id=target_id AND a.deleted=0 AND a.must_change_password=0) THEN 'activated' ELSE 'expired' END,payload=NULL WHERE state IN('created','delivery_pending') AND (expires_at<=unixepoch() OR NOT EXISTS(SELECT 1 FROM accounts actor WHERE actor.id=actor_id AND actor.auth_epoch=actor_epoch AND actor.role='admin' AND actor.status='active' AND actor.deleted=0) OR NOT EXISTS(SELECT 1 FROM accounts a WHERE a.id=target_id AND a.deleted=0 AND a.status='active' AND a.auth_epoch=target_epoch AND a.management_revision=target_revision AND a.must_change_password=1 AND a.temporary_expires_at>unixepoch()))",[])?;
    Ok(())
}
fn project(
    c: &Connection,
    key: &[u8; 32],
    actor: &UserContext,
    id: &str,
) -> Result<InvitationDelivery> {
    let (mut v,payload):(InvitationDelivery,Option<Vec<u8>>)=c.query_row("SELECT id,target_id,state,digest,expires_at,from_address,to_address,subject,provider_message,payload FROM invitation_deliveries WHERE id=?1 AND actor_id=?2",params![id,actor.account.id],|r|Ok((InvitationDelivery{id:r.get(0)?,account_id:r.get(1)?,state:r.get(2)?,digest:r.get(3)?,expires_at:r.get(4)?,from:r.get(5)?,to:r.get(6)?,subject:r.get(7)?,provider_message_id:r.get(8)?,text:None},r.get(9)?))).optional()?.ok_or(StoreError::NotFound)?;
    if let Some(p)=payload.filter(|_| c.query_row("SELECT session_digest=?3 AND actor_epoch=?4 FROM invitation_deliveries WHERE id=?1 AND actor_id=?2",params![id,actor.account.id,actor.digest.as_slice(),actor.epoch],|r|r.get::<_,bool>(0)).unwrap_or(false)){let plain=providers::decrypt(key,&p,&aad(id))?;let (text,_,_,_):(String,Vec<u8>,Option<String>,[u8;32])=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;v.text=Some(text);}
    Ok(v)
}
fn authority(c: &Connection, u: &UserContext, id: &str, pin: &str) -> Result<()> {
    validate(c, u, true, true)?;
    let current:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM invitation_deliveries i JOIN accounts a ON a.id=i.target_id WHERE i.id=?1 AND i.actor_id=?2 AND i.actor_epoch=?3 AND i.session_digest=?4 AND i.sender_pin=?5 AND i.expires_at>unixepoch() AND a.deleted=0 AND a.status='active' AND a.recovery_required=0 AND a.must_change_password=1 AND a.auth_epoch=i.target_epoch AND a.management_revision=i.target_revision AND a.temporary_expires_at>unixepoch())",params![id,u.account.id,u.epoch,u.digest.as_slice(),pin],|r|r.get(0))?;
    if !current {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
impl Store {
    pub async fn preview_invitation(
        &self,
        u: &UserContext,
        input: InvitationPreviewInput,
        from: String,
        origin: String,
        pin: String,
    ) -> Result<InvitationDelivery> {
        let u = u.clone();
        let check = u.clone();
        let target = input.account_id.clone();
        let expected = input.expected_revision;
        let checked_hash=self.call(move|db|{validate(&db.conn,&check,true,true)?;
   db.conn.query_row("SELECT password_hash FROM accounts WHERE id=?1 AND management_revision=?2 AND deleted=0 AND must_change_password=1",params![target,expected],|r|r.get::<_,String>(0)).optional()?.ok_or(StoreError::Conflict)
  }).await?;
        if let Some(password) = &input.temporary_password
            && (password.len() > 256
                || !verify_password(Zeroizing::new(password.clone()), checked_hash.clone()).await?)
        {
            return Err(StoreError::InvalidInput);
        }
        self.call(move|db|{
   validate(&db.conn,&u,true,true)?;refresh(&db.conn)?;
   if Uuid::parse_str(&input.request_id).is_err(){return Err(StoreError::InvalidInput)}
   let request_digest=Sha256::digest(serde_json::to_vec(&(&input,&from,&origin,&pin)).map_err(|_|StoreError::InvalidInput)?).to_vec();
   if let Some(prior)=db.conn.query_row("SELECT request_digest FROM invitation_deliveries WHERE id=?1 AND actor_id=?2",params![input.request_id,u.account.id],|r|r.get::<_,Vec<u8>>(0)).optional()? {
    if prior!=request_digest{return Err(StoreError::Conflict)}return project(&db.conn,&db.provider_key,&u,&input.request_id)
   }
   let (name,to,epoch,revision,expiry,hash):(String,String,i64,i64,i64,String)=db.conn.query_row("SELECT name,email,auth_epoch,management_revision,temporary_expires_at,password_hash FROM accounts WHERE id=?1 AND deleted=0 AND status='active' AND recovery_required=0 AND must_change_password=1 AND temporary_expires_at>unixepoch()",[&input.account_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?.ok_or(StoreError::Conflict)?;
   if revision!=input.expected_revision{return Err(StoreError::Conflict)}
   // The one-time UI password must match the current credential. Verification
   // is done before entry (async KDF); this call receives only validated input.
   if hash!=checked_hash{return Err(StoreError::Conflict)}
   if db.conn.query_row("SELECT count(*) FROM invitation_deliveries WHERE actor_id=?1 AND state='delivery_pending'",[&u.account.id],|r|r.get::<_,i64>(0))?>=16{return Err(StoreError::Busy)}
   if db.conn.query_row("SELECT EXISTS(SELECT 1 FROM invitation_deliveries WHERE target_id=?1 AND target_epoch=?2)",params![input.account_id,epoch],|r|r.get::<_,bool>(0))? {return Err(StoreError::Conflict)}
   let expires=expiry.min(now()?+900);
   let mut text=format!("Hello {name},\n\nYour moosuri account is ready.\nEmail: {to}\nSign in: {origin}/app\nTemporary account access expires at UTC {}.\nChoose a new password at first sign-in.\n",chrono::DateTime::from_timestamp(expiry,0).ok_or(StoreError::Corrupt)?.to_rfc3339());
   if let Some(password)=&input.temporary_password{text.push_str(&format!("\nTemporary password: {password}\nKeep this message private.\n"));}
   let subject="Your moosuri invitation".to_string();
   let frozen=FrozenMail::prepare(&MailContent{from:from.clone(),to:vec![to.clone()],cc:vec![],bcc:vec![],subject:subject.clone(),text:text.clone(),reply:None},&format!("{}@moosuri.local",input.request_id)).map_err(|_|StoreError::InvalidInput)?;
   let d=digest(frozen.raw_mime());
   let plain=Zeroizing::new(serde_json::to_vec(&(text,frozen.raw_mime(),frozen.thread_id(),frozen.digest())).map_err(|_|StoreError::InvalidInput)?);
   let cipher=providers::encrypt(&db.provider_key,&plain,&aad(&input.request_id))?;
   db.conn.execute("INSERT INTO invitation_deliveries VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,'delivery_pending',?15,NULL)",params![input.request_id,u.account.id,input.account_id,epoch,revision,u.epoch,u.digest.as_slice(),pin,request_digest,d,from,to,subject,cipher,expires])?;
   project(&db.conn,&db.provider_key,&u,&input.request_id)
  }).await
    }
    pub async fn expire_invitation_deliveries(&self) -> Result<()> {
        self.call(move |db| refresh(&db.conn)).await
    }
    pub async fn invitation_delivery(
        &self,
        u: &UserContext,
        id: String,
    ) -> Result<InvitationDelivery> {
        let u = u.clone();
        self.call(move |db| {
            validate(&db.conn, &u, true, true)?;
            refresh(&db.conn)?;
            project(&db.conn, &db.provider_key, &u, &id)
        })
        .await
    }
    pub async fn invitation_deliveries(&self, u: &UserContext) -> Result<Vec<InvitationDelivery>> {
        let u = u.clone();
        self.call(move|db|{validate(&db.conn,&u,true,true)?;refresh(&db.conn)?;let ids=db.conn.prepare("SELECT id FROM invitation_deliveries WHERE actor_id=?1 ORDER BY rowid DESC LIMIT 50")?.query_map([&u.account.id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;ids.iter().map(|id|{let mut v=project(&db.conn,&db.provider_key,&u,id)?;v.text=None;Ok(v)}).collect()}).await
    }
    pub async fn commit_invitation(
        &self,
        u: &UserContext,
        id: String,
        digest_expected: String,
        pin: String,
    ) -> Result<Option<InvitationDispatch>> {
        let u = u.clone();
        self.call(move|db|{
  refresh(&db.conn)?;authority(&db.conn,&u,&id,&pin)?;
  let (state,d,payload):(String,String,Option<Vec<u8>>)=db.conn.query_row("SELECT state,digest,payload FROM invitation_deliveries WHERE id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
  if d!=digest_expected{return Err(StoreError::Conflict)}
  if state!="delivery_pending"{return Ok(None)}
  let plain=providers::decrypt(&db.provider_key,&payload.ok_or(StoreError::Conflict)?,&aad(&id))?;
  let (_,raw,thread,frozen_digest):(String,Vec<u8>,Option<String>,[u8;32])=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;
  if digest(&raw)!=d{return Err(StoreError::Corrupt)}
  let frozen=FrozenMail::restore(raw,frozen_digest,thread).map_err(|_|StoreError::Corrupt)?;
  db.conn.execute("UPDATE invitation_deliveries SET state='dispatch_committed',payload=NULL WHERE id=?1 AND state='delivery_pending'",[&id])?;
  Ok(Some(InvitationDispatch{id,frozen,sender_pin:pin,user:u}))
 }).await
    }
    pub async fn validate_invitation_dispatch(&self, d: &InvitationDispatch) -> Result<()> {
        let u = d.user.clone();
        let id = d.id.clone();
        let pin = d.sender_pin.clone();
        self.call(move |db| {
            authority(&db.conn, &u, &id, &pin)?;
            let state: String = db.conn.query_row(
                "SELECT state FROM invitation_deliveries WHERE id=?1",
                [id],
                |r| r.get(0),
            )?;
            if state != "dispatch_committed" {
                return Err(StoreError::Conflict);
            }
            Ok(())
        })
        .await
    }
    pub async fn finish_invitation(
        &self,
        d: &InvitationDispatch,
        result: Option<String>,
        definite: bool,
    ) -> Result<()> {
        let id = d.id.clone();
        self.call(move|db|{db.conn.execute("UPDATE invitation_deliveries SET state=?2,provider_message=CASE WHEN EXISTS(SELECT 1 FROM accounts a WHERE a.id IN(target_id,actor_id) AND a.deleted=1) THEN NULL ELSE ?3 END,payload=NULL WHERE id=?1 AND state IN('dispatch_committed','delivery_unknown')",params![id,if result.is_some(){"sent"}else if definite{"failed_definite"}else{"delivery_unknown"},result])?;Ok(())}).await
    }
}

pub(crate) fn invalidate(c: &Connection, id: &str) -> Result<()> {
    c.execute("UPDATE invitation_deliveries SET payload=NULL,state='expired' WHERE (target_id=?1 OR actor_id=?1) AND state IN('created','delivery_pending')",[id])?;
    Ok(())
}
#[cfg(test)]
mod tests;
