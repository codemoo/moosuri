//! Separately consented native mail. No legacy display output becomes model evidence.
use super::*;
#[derive(Serialize)]
pub struct NativeMailModelJob {
    pub request_id: String,
    pub status: String,
    /// Exact signed device/owner/binding/invocation plus typed bounded content.
    pub receipt: Option<SignedReceipt>,
}
pub(super) fn register(
    c: &Connection,
    pair: &PairChallenge,
    status: &str,
    binding: &Binding,
    background: bool,
    expected: Option<&Binding>,
) -> Result<()> {
    let device = pair.device.id.to_string();
    let scope = Resource::google_model_read(Uuid::nil());
    if let Some(expected) = expected {
        if status != "active"
            || binding.id != expected.id
            || binding.epoch != expected.epoch.checked_add(1).ok_or(StoreError::Forbidden)?
            || binding.grant_version
                != expected
                    .grant_version
                    .checked_add(1)
                    .ok_or(StoreError::Forbidden)?
            || binding.item_epoch < expected.item_epoch
            || binding.key_epoch < expected.key_epoch
            || binding.expires_at != expected.expires_at
            || binding.expires_at <= now()?
            || binding.grant_hash == [0; 32]
            || binding.grant_hash == expected.grant_hash
        {
            return Err(StoreError::Forbidden);
        }
        let (saved,resource,old_background):(String,String,bool)=c.query_row("SELECT b.binding,r.repository,m.background FROM credential_broker_bindings b JOIN credential_broker_repositories r USING(device_id,binding_id) JOIN credential_model_mail_grants m USING(device_id,binding_id) WHERE b.device_id=?1 AND b.binding_id=?2 AND b.active=1",params![device,binding.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(StoreError::NotFound)?;
        if decode::<Resource>(&resource)? != scope || old_background != background {
            return Err(StoreError::Forbidden);
        }
        let current: Binding = decode(&saved)?;
        if current != *binding {
            if current != *expected {
                return Err(StoreError::Conflict);
            }
            c.execute("UPDATE credential_broker_bindings SET binding=?1 WHERE device_id=?2 AND binding_id=?3 AND binding=?4",params![encode(binding)?,device,binding.id.to_string(),saved])?;
            github::refresh_resource(c, &pair.device, expected, binding, &scope)?;
        }
    } else {
        register_google(c, &device, status, binding, &scope)?;
        let old:Option<bool>=c.query_row("SELECT background FROM credential_model_mail_grants WHERE device_id=?1 AND binding_id=?2",params![device,binding.id.to_string()],|r|r.get(0)).optional()?;
        if old.is_some_and(|old| old != background) {
            return Err(StoreError::Conflict);
        }
        c.execute(
            "INSERT OR IGNORE INTO credential_model_mail_grants VALUES(?1,?2,?3)",
            params![device, binding.id.to_string(), background],
        )?;
    }
    Ok(())
}
fn selected_payload(
    c: &Connection,
    u: &UserContext,
    id: &str,
) -> Result<(String, String, MailModelRequest, i64)> {
    let row:(String,String,String,i64)=c.query_row("SELECT device_id,binding_id,payload,expires_at FROM credential_model_mail_payloads WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![u.tenant.0,u.account.id,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(StoreError::Forbidden)?;
    let payload: MailModelRequest = decode(&row.2)?;
    if !payload.valid() {
        return Err(StoreError::Corrupt);
    }
    Ok((row.0, row.1, payload, row.3))
}
fn current_binding(
    c: &Connection,
    u: &UserContext,
    device: &str,
    binding: &str,
    payload: &MailModelRequest,
) -> Result<Binding> {
    let (encoded,background):(String,bool)=c.query_row("SELECT b.binding,m.background FROM credential_broker_bindings b JOIN credential_broker_devices d ON d.id=b.device_id JOIN credential_broker_repositories r USING(device_id,binding_id) JOIN credential_model_mail_grants m USING(device_id,binding_id) JOIN sessions s ON s.digest=d.session_digest WHERE b.device_id=?1 AND b.binding_id=?2 AND b.active=1 AND d.status='active' AND d.tenant_id=?3 AND d.account_id=?4 AND s.expires_at>?5 AND s.auth_epoch=?6 AND r.repository=?7",params![device,binding,u.tenant.0,u.account.id,now()?,u.epoch,encode(&Resource::google_model_read(Uuid::nil()))?],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::Forbidden)?;
    let b: Binding = decode(&encoded)?;
    if b.expires_at <= now()? || payload.background && !background {
        return Err(StoreError::Forbidden);
    }
    Ok(b)
}
/// Poll admission pins the requested device/grant; it never fails over.
pub(super) fn admit(
    c: &Connection,
    u: &UserContext,
    id: &str,
    device: &Device,
    binding: &Binding,
) -> Result<Option<[u8; 32]>> {
    let (d, b, payload, expiry) = selected_payload(c, u, id)?;
    if d != device.id.to_string() || b != binding.id.to_string() {
        return Ok(None);
    }
    if expiry <= now()? || current_binding(c, u, &d, &b, &payload)? != *binding {
        return Ok(None);
    }
    Ok(Some(moosuri_credential_broker::auth::hash(
        &payload.bytes(),
    )))
}
pub(super) fn payload(
    c: &Connection,
    pair: &PairChallenge,
    u: &UserContext,
    r: &Request,
) -> Result<Reply> {
    let Operation::GoogleMailModelRead { selection } = r.operation else {
        return Err(StoreError::Forbidden);
    };
    if r.device != pair.device
        || r.device.owner != vault_authority::owner(c, u)?
        || r.expires_at <= now()?
    {
        return Err(StoreError::Forbidden);
    }
    let (device, binding, payload, expiry) = selected_payload(c, u, &selection.to_string())?;
    if device != r.device.id.to_string()
        || binding != r.binding.id.to_string()
        || expiry < r.expires_at
        || current_binding(c, u, &device, &binding, &payload)? != r.binding
        || moosuri_credential_broker::auth::hash(&payload.bytes()) != r.digest
        || !crate::credential_chat::dispatch_current(
            c,
            &u.tenant.0,
            &r.invocation.run.to_string(),
            r.invocation.step,
            &r.invocation.call.to_string(),
            &r.invocation.nonce.to_string(),
        )?
    {
        return Err(StoreError::Forbidden);
    }
    Ok(Reply::ModelMailPayload {
        payload: Box::new(payload),
    })
}
fn selector_origin(
    c: &Connection,
    u: &UserContext,
    device: &str,
    binding: &str,
    _selection: Uuid,
    expected: &Binding,
    request: &MailModelRequest,
) -> Result<i64> {
    let receipts=c.prepare("SELECT x.receipt FROM credential_broker_jobs j JOIN credential_broker_commitments x ON x.tenant_id=j.tenant_id AND x.run_id=j.run_id JOIN credential_broker_invocations i ON i.tenant_id=j.tenant_id AND i.run_id=j.run_id WHERE j.tenant_id=?1 AND j.account_id=?2 AND j.auth_epoch=?3 AND json_extract(j.repository,'$.google_model_read') IS NOT NULL AND x.receipt IS NOT NULL AND i.cancelled=0 LIMIT 1024")?.query_map(params![u.tenant.0,u.account.id,u.epoch],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for raw in receipts {
        let signed: SignedReceipt = decode(&raw)?;
        let r = &signed.receipt.commitment.request;
        if r.device.id.to_string() != device
            || r.binding.id.to_string() != binding
            || r.binding != *expected
            || r.expires_at <= now()?
            || signed.receipt.outcome != Outcome::ProjectedSuccess
            || !github::device_current(c, &signed)?
        {
            continue;
        }
        let Projection::GoogleMailModelRead { result } = &signed.receipt.projection else {
            continue;
        };
        let Operation::GoogleMailModelRead {
            selection: origin_id,
        } = r.operation
        else {
            continue;
        };
        let origin:Option<String>=c.query_row("SELECT payload FROM credential_model_mail_payloads WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3 AND device_id=?4 AND binding_id=?5",params![u.tenant.0,u.account.id,origin_id.to_string(),device,binding],|row|row.get(0)).optional()?;
        let Some(origin) = origin else { continue };
        let origin: MailModelRequest = decode(&origin)?;
        if request.accepts_selector(&origin, result) {
            return Ok(r.expires_at);
        }
    }
    Err(StoreError::Forbidden)
}
impl Store {
    /// Backend admission only. The model runner must additionally require its
    /// pinned, actually probed native-mail tool profile before calling this API.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_native_model_mail(
        &self,
        user: &UserContext,
        id: String,
        device: Uuid,
        binding: Uuid,
        request: MailModelRequest,
        expires_at: i64,
    ) -> Result<NativeMailModelJob> {
        if !Uuid::parse_str(&id).is_ok_and(|id| !id.is_nil())
            || device.is_nil()
            || binding.is_nil()
            || !request.valid()
        {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&user,true,false)?;let b=current_binding(&tx,&u,&device.to_string(),&binding.to_string(),&request)?;
            let mut expiry=expires_at.min(b.expires_at).min(now()?+86400);if expiry<=now()? {return Err(StoreError::InvalidInput);}
            let selected=match &request.read {MailModelRead::Search {cursor,..}=>*cursor,MailModelRead::Message {selection}|MailModelRead::Thread {selection}|MailModelRead::Attachment {selection,..}=>Some(*selection)};
            if let Some(selected)=selected {expiry=expiry.min(selector_origin(&tx,&u,&device.to_string(),&binding.to_string(),selected,&b,&request)?);}
            let old:Option<(String,String,String)>=tx.query_row("SELECT device_id,binding_id,payload FROM credential_model_mail_payloads WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![u.tenant.0,u.account.id,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let payload=encode(&request)?;if let Some(old)=old {if old!=(device.to_string(),binding.to_string(),payload){return Err(StoreError::Conflict);}}
            else {let n:i64=tx.query_row("SELECT count(*) FROM credential_broker_jobs WHERE tenant_id=?1",[&u.tenant.0],|r|r.get(0))?;if n>=1024{return Err(StoreError::Busy);}
                tx.execute("INSERT INTO credential_broker_jobs(tenant_id,account_id,request_id,run_id,call_id,invocation_nonce,repository,auth_epoch,expires_at,request) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",params![u.tenant.0,u.account.id,id,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),encode(&Resource::google_model_read(id.parse().map_err(|_|StoreError::InvalidInput)?))?,u.epoch,expiry])?;
                tx.execute("INSERT INTO credential_model_mail_payloads VALUES(?1,?2,?3,?4,?5,?6,?7)",params![u.tenant.0,u.account.id,id,device.to_string(),binding.to_string(),payload,expiry])?;
            }let job=model_job(&tx,&u,&id)?;tx.commit()?;Ok(job)
        }).await
    }
    pub async fn native_model_mail_job(
        &self,
        user: &UserContext,
        id: String,
    ) -> Result<NativeMailModelJob> {
        let user = user.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &user, true, false)?;
            model_job(&db.conn, &u, &id)
        })
        .await
    }
}
fn model_job(c: &Connection, u: &UserContext, id: &str) -> Result<NativeMailModelJob> {
    let (device, binding, payload, _) = selected_payload(c, u, id)?;
    let job = github::scoped_job(c, &u.tenant.0, id)?;
    let receipt = if matches!(job.status.as_str(), "complete" | "credential_rejected") {
        let raw: Option<String> = c.query_row("SELECT x.receipt FROM credential_broker_jobs j JOIN credential_broker_commitments x ON x.tenant_id=j.tenant_id AND x.run_id=j.run_id JOIN credential_broker_invocations i ON i.tenant_id=j.tenant_id AND i.run_id=j.run_id WHERE j.tenant_id=?1 AND j.account_id=?2 AND j.auth_epoch=?4 AND j.request_id=?3 AND i.cancelled=0", params![u.tenant.0,u.account.id,id,u.epoch], |r| r.get(0)).optional()?.flatten();
        let signed: Option<SignedReceipt> = raw.map(|s| decode(&s)).transpose()?;
        signed.filter(|s| current_binding(c,u,&device,&binding,&payload).is_ok_and(|b| b == s.receipt.commitment.request.binding) && matches!(&s.receipt.projection, Projection::GoogleMailModelRead { result } if result.valid()))
    } else {
        None
    };
    Ok(NativeMailModelJob {
        request_id: id.into(),
        status: job.status,
        receipt,
    })
}
#[cfg(test)]
mod tests;
