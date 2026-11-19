//! Browser-only mail observations. Kept outside chat turns, recall, memory and
//! model context. A durable dispatch marker forbids an automatic second read.
use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberGoogleReadIntent {
    pub thread_id: Uuid,
    pub request_id: Uuid,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberGoogleInboxMessage {
    pub id: String,
    pub received_at_ms: i64,
    pub subject: String,
    pub from: String,
    pub date: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberGoogleInbox {
    pub mailbox: String,
    pub messages: Vec<MemberGoogleInboxMessage>,
    pub has_more: bool,
    pub scope: String,
}
impl MemberGoogleInbox {
    pub fn valid(&self) -> bool {
        let literal = |s: &str| s.chars().count() <= 512 && !s.chars().any(char::is_control);
        email_valid(self.mailbox.clone()).is_ok()
            && self.scope == "one_inbox_page_metadata_only"
            && self.messages.len() <= 5
            && self.messages.iter().all(|m| {
                !m.id.is_empty()
                    && m.id.len() <= 256
                    && m.id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
                    && m.received_at_ms >= 0
                    && literal(&m.subject)
                    && literal(&m.from)
                    && literal(&m.date)
            })
            && self
                .messages
                .iter()
                .map(|m| &m.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == self.messages.len()
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberGoogleReadSource {
    pub connection_id: Uuid,
    pub epoch: i64,
    pub app_revision: Uuid,
    pub mailbox: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemberGoogleThreadRead {
    pub thread_id: Uuid,
    pub request_id: Uuid,
    pub state: String,
    pub created_at: i64,
    pub source: Option<MemberGoogleReadSource>,
    pub result: Option<MemberGoogleInbox>,
}
impl MemberGoogleThreadRead {
    pub fn valid(&self) -> bool {
        !self.thread_id.is_nil()
            && !self.request_id.is_nil()
            && self.created_at >= 0
            && [
                "awaiting_consent",
                "ready",
                "dispatched",
                "completed",
                "unknown",
                "withdrawn",
            ]
            .contains(&self.state.as_str())
            && self.source.as_ref().is_none_or(|s| {
                !s.connection_id.is_nil()
                    && s.epoch > 0
                    && !s.app_revision.is_nil()
                    && email_valid(s.mailbox.clone()).is_ok()
            })
            && if self.state == "completed" {
                self.result.as_ref().is_some_and(|r| {
                    r.valid()
                        && self
                            .source
                            .as_ref()
                            .is_some_and(|s| s.mailbox.eq_ignore_ascii_case(&r.mailbox))
                })
            } else {
                self.result.is_none()
            }
    }
}
pub enum MemberGoogleReadAdmission {
    Fresh(MemberGoogleReadLease),
    Existing(MemberGoogleThreadRead),
}
pub struct MemberGoogleReadLease {
    user: UserContext,
    owner: VolumeOwner,
    app: Uuid,
    intent: MemberGoogleReadIntent,
    id: Uuid,
    epoch: i64,
}

struct Saved {
    intent: MemberGoogleReadIntent,
    owner: VolumeOwner,
    app: Uuid,
    id: Option<Uuid>,
    epoch: Option<i64>,
    phase: String,
    created: i64,
    cipher: Option<Vec<u8>>,
}
fn saved(c: &Connection, u: &UserContext, id: Uuid) -> Result<Saved> {
    c.query_row("SELECT thread_id,volume_id,generation,app_revision,connection_id,source_epoch,phase,created_at,cipher FROM member_google_thread_reads WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![u.tenant.0,u.account.id,id.to_string()],|r|Ok(Saved {intent:MemberGoogleReadIntent{thread_id:parse_id(r.get(0)?)?,request_id:id},owner:VolumeOwner{account:parse_id(u.account.id.clone())?,tenant:parse_id(u.tenant.0.clone())?,volume:parse_id(r.get(1)?)?,generation:r.get::<_,i64>(2)? as u64},app:parse_id(r.get(3)?)?,id:r.get::<_,Option<String>>(4)?.map(parse_id).transpose()?,epoch:r.get(5)?,phase:r.get(6)?,created:r.get(7)?,cipher:r.get(8)?})).optional()?.ok_or(StoreError::NotFound)
}
fn same_source(s: &Saved, r: &Row, u: &UserContext) -> bool {
    matches(r, s.owner, s.app)
        && r.status == "active"
        && r.auth_epoch == u.epoch
        && r.id == s.id
        && Some(r.epoch) == s.epoch
}
fn aad(s: &Saved) -> Result<Vec<u8>> {
    Ok(format!(
        "moosuri/member-google/thread-read/v1/{}/{}/{}/{}/{}/{}/{}/{}/{}",
        s.owner.account,
        s.owner.tenant,
        s.owner.volume,
        s.owner.generation,
        s.app,
        s.intent.thread_id,
        s.intent.request_id,
        s.id.ok_or(StoreError::Corrupt)?,
        s.epoch.ok_or(StoreError::Corrupt)?
    )
    .into_bytes())
}
fn view(
    c: &Connection,
    key: &[u8; 32],
    u: &UserContext,
    o: VolumeOwner,
    id: Uuid,
) -> Result<MemberGoogleThreadRead> {
    let mut s = saved(c, u, id)?;
    if s.owner.volume != o.volume || s.owner.generation != o.generation {
        return Err(StoreError::Conflict);
    }
    let current = row(c, u, Service::Gmail)?;
    let source =
        current
            .as_ref()
            .filter(|r| same_source(&s, r, u))
            .map(|r| MemberGoogleReadSource {
                connection_id: r.id.unwrap(),
                epoch: r.epoch,
                app_revision: r.app,
                mailbox: r.mailbox.clone().unwrap_or_default(),
            });
    if source.is_none() && matches!(s.phase.as_str(), "ready" | "dispatched" | "completed") {
        c.execute("UPDATE member_google_thread_reads SET phase='withdrawn',cipher=NULL,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![u.tenant.0,u.account.id,id.to_string()])?;
        s.phase = "withdrawn".into();
        s.cipher = None;
    }
    let result = if s.phase == "completed" {
        let bytes = providers::decrypt(
            key,
            s.cipher.as_deref().ok_or(StoreError::Corrupt)?,
            &aad(&s)?,
        )?;
        let result: MemberGoogleInbox =
            serde_json::from_slice(&bytes).map_err(|_| StoreError::Corrupt)?;
        if !result.valid()
            || source
                .as_ref()
                .is_none_or(|v| !v.mailbox.eq_ignore_ascii_case(&result.mailbox))
        {
            return Err(StoreError::Corrupt);
        }
        Some(result)
    } else {
        None
    };
    Ok(MemberGoogleThreadRead {
        thread_id: s.intent.thread_id,
        request_id: id,
        state: s.phase,
        created_at: s.created,
        source,
        result,
    })
}
pub(super) fn withdraw(c: &Connection, u: &UserContext) -> Result<()> {
    c.execute("UPDATE member_google_thread_reads SET phase='withdrawn',cipher=NULL,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND phase!='withdrawn'",params![u.tenant.0,u.account.id])?;
    Ok(())
}
pub(super) fn connected(
    c: &Connection,
    u: &UserContext,
    o: VolumeOwner,
    app: Uuid,
    flow: &str,
    id: Uuid,
    epoch: i64,
) -> Result<()> {
    c.execute("UPDATE member_google_thread_reads SET phase='withdrawn',cipher=NULL,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND NOT(phase='awaiting_consent' AND flow_id=?3 AND auth_epoch=?4 AND session_digest=?5 AND volume_id=?6 AND generation=?7 AND app_revision=?8)",params![u.tenant.0,u.account.id,flow,u.epoch,u.digest.as_slice(),o.volume.to_string(),o.generation as i64,app.to_string()])?;
    c.execute("UPDATE member_google_thread_reads SET connection_id=?4,source_epoch=?5,phase='ready',updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND flow_id=?3 AND phase='awaiting_consent'",params![u.tenant.0,u.account.id,flow,id.to_string(),epoch])?;
    Ok(())
}
fn intent(c: &Connection, u: &UserContext, v: &MemberGoogleReadIntent) -> Result<()> {
    if v.thread_id.is_nil() || v.request_id.is_nil() {
        return Err(StoreError::InvalidInput);
    }
    crate::chat::thread_exists(c, u, &v.thread_id.to_string())
}
impl Store {
    pub async fn bind_member_google_read_oauth(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        app: Uuid,
        state: String,
        v: MemberGoogleReadIntent,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=authority(&tx,&u,o)?;intent(&tx,&u,&v)?;
            let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM member_google_oauth m JOIN mail_oauth_sessions s USING(state_hash) WHERE m.service='gmail' AND m.state_hash=?1 AND m.tenant_id=?2 AND m.account_id=?3 AND m.volume_id=?4 AND m.generation=?5 AND m.app_revision=?6 AND s.session_digest=?7 AND s.auth_epoch=?8 AND s.claimed=0 AND s.expires_at>unixepoch())",params![hash(&state),u.tenant.0,u.account.id,o.volume.to_string(),o.generation as i64,app.to_string(),u.digest.as_slice(),u.epoch],|r|r.get(0))?;
            if !valid{return Err(StoreError::Conflict);}
            if tx.query_row("SELECT count(*) FROM member_google_thread_reads WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|r.get::<_,i64>(0))?>=1024{return Err(StoreError::Busy);}
            tx.execute("INSERT INTO member_google_thread_reads(tenant_id,account_id,request_id,thread_id,volume_id,generation,auth_epoch,session_digest,app_revision,flow_id,phase,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'awaiting_consent',unixepoch(),unixepoch())",params![u.tenant.0,u.account.id,v.request_id.to_string(),v.thread_id.to_string(),o.volume.to_string(),o.generation as i64,u.epoch,u.digest.as_slice(),app.to_string(),Store::mail_oauth_flow_id(&state)])?;
            tx.commit()?;Ok(())
        }).await
    }
    pub async fn member_google_read_continuation(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        flow: String,
    ) -> Result<Option<MemberGoogleReadIntent>> {
        let u = u.clone();
        self.call(move|db|{let u=authority(&db.conn,&u,o)?;let id:Option<String>=db.conn.query_row("SELECT request_id FROM member_google_thread_reads WHERE tenant_id=?1 AND account_id=?2 AND flow_id=?3 AND session_digest=?4 AND auth_epoch=?5 AND phase='ready'",params![u.tenant.0,u.account.id,flow,u.digest.as_slice(),u.epoch],|r|r.get(0)).optional()?;let Some(id)=id else{return Ok(None);};let s=saved(&db.conn,&u,parse_id(id).map_err(|_|StoreError::Corrupt)?)?;crate::chat::thread_exists(&db.conn,&u,&s.intent.thread_id.to_string())?;Ok(Some(s.intent))}).await
    }
    pub async fn begin_member_google_thread_read(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        app: Uuid,
        v: MemberGoogleReadIntent,
        id: Uuid,
        epoch: i64,
    ) -> Result<MemberGoogleReadAdmission> {
        let u = u.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=authority(&tx,&u,o)?;intent(&tx,&u,&v)?;
            let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM member_google_thread_reads WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3)",params![u.tenant.0,u.account.id,v.request_id.to_string()],|r|r.get(0))?;
            if exists {let s=saved(&tx,&u,v.request_id)?;if s.intent.thread_id!=v.thread_id||s.owner.volume!=o.volume||s.owner.generation!=o.generation||s.app!=app||s.id!=Some(id)||s.epoch!=Some(epoch){return Err(StoreError::Conflict);}let same:bool=tx.query_row("SELECT session_digest=?4 AND auth_epoch=?5 FROM member_google_thread_reads WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![u.tenant.0,u.account.id,v.request_id.to_string(),u.digest.as_slice(),u.epoch],|r|r.get(0))?;if !same{return Err(StoreError::Conflict);}if s.phase!="ready" {let v=view(&tx,&db.provider_key,&u,o,v.request_id)?;tx.commit()?;return Ok(MemberGoogleReadAdmission::Existing(v));}}
            let r=live(&tx,&u,o,app,Service::Gmail,true)?;if r.id!=Some(id)||r.epoch!=epoch{return Err(StoreError::Conflict);}
            if !exists {if tx.query_row("SELECT count(*) FROM member_google_thread_reads WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|r.get::<_,i64>(0))?>=1024{return Err(StoreError::Busy);}tx.execute("INSERT INTO member_google_thread_reads(tenant_id,account_id,request_id,thread_id,volume_id,generation,auth_epoch,session_digest,app_revision,connection_id,source_epoch,phase,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,'ready',unixepoch(),unixepoch())",params![u.tenant.0,u.account.id,v.request_id.to_string(),v.thread_id.to_string(),o.volume.to_string(),o.generation as i64,u.epoch,u.digest.as_slice(),app.to_string(),id.to_string(),epoch])?;}
            tx.execute("UPDATE member_google_thread_reads SET phase='dispatched',updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3 AND phase='ready'",params![u.tenant.0,u.account.id,v.request_id.to_string()])?;
            tx.commit()?;Ok(MemberGoogleReadAdmission::Fresh(MemberGoogleReadLease{user:u,owner:o,app,intent:v,id,epoch}))
        }).await
    }
    pub async fn settle_member_google_thread_read(
        &self,
        lease: MemberGoogleReadLease,
        result: Option<MemberGoogleInbox>,
    ) -> Result<MemberGoogleThreadRead> {
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=authority(&tx,&lease.user,lease.owner)?;intent(&tx,&u,&lease.intent)?;
            let s=saved(&tx,&u,lease.intent.request_id)?;let r=row(&tx,&u,Service::Gmail)?.ok_or(StoreError::Conflict)?;
            if s.phase!="dispatched"||!same_source(&s,&r,&u)||s.app!=lease.app||s.id!=Some(lease.id)||s.epoch!=Some(lease.epoch){return Err(StoreError::Conflict);}
            let (phase,cipher)=if let Some(result)=result {
                if !result.valid()||r.mailbox.as_ref().is_none_or(|m|!m.eq_ignore_ascii_case(&result.mailbox)){return Err(StoreError::InvalidInput);}
                let bytes=Zeroizing::new(serde_json::to_vec(&result).map_err(|_|StoreError::InvalidInput)?);if bytes.len()>32768{return Err(StoreError::InvalidInput);}
                ("completed",Some(providers::encrypt(&db.provider_key,&bytes,&aad(&s)?)?))
            }else{("unknown",None)};
            tx.execute("UPDATE member_google_thread_reads SET phase=?4,cipher=?5,updated_at=unixepoch() WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3 AND phase='dispatched'",params![u.tenant.0,u.account.id,lease.intent.request_id.to_string(),phase,cipher])?;
            tx.execute("UPDATE threads SET updated_at=max(updated_at,unixepoch()) WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,lease.intent.thread_id.to_string()])?;
            let result=view(&tx,&db.provider_key,&u,lease.owner,lease.intent.request_id)?;tx.commit()?;Ok(result)
        }).await
    }
    pub async fn member_google_thread_history(
        &self,
        u: &UserContext,
        o: VolumeOwner,
        thread: Uuid,
    ) -> Result<Vec<MemberGoogleThreadRead>> {
        let u = u.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=authority(&tx,&u,o)?;crate::chat::thread_exists(&tx,&u,&thread.to_string())?;
            let ids=tx.prepare("SELECT request_id FROM member_google_thread_reads WHERE tenant_id=?1 AND account_id=?2 AND thread_id=?3 ORDER BY created_at DESC,request_id DESC LIMIT 20")?.query_map(params![u.tenant.0,u.account.id,thread.to_string()],|r|parse_id(r.get(0)?))?.collect::<std::result::Result<Vec<_>,_>>()?;
            let values=ids.into_iter().map(|id|view(&tx,&db.provider_key,&u,o,id)).collect::<Result<Vec<_>>>()?;tx.commit()?;Ok(values)
        }).await
    }
}
