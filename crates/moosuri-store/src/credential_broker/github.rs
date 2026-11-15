//! Typed GitHub metadata requests. No generic URL or model-selected item/secret.
use super::*;
#[derive(serde::Serialize)]
pub struct GitHubJob {
    pub request_id: String,
    pub repository: GitHubRepository,
    pub status: String,
    pub projection: Option<Projection>,
}
#[derive(serde::Serialize)]
pub struct ScopedJob {
    pub run_id: String,
    pub expires_at: i64,
    pub request_id: String,
    pub resource: Resource,
    pub status: String,
    pub projection: Option<Projection>,
}
fn job(conn: &Connection, tenant: &str, id: &str) -> Result<GitHubJob> {
    let j = scoped_job(conn, tenant, id)?;
    let Resource::GitHub(repository) = j.resource else {
        return Err(StoreError::InvalidInput);
    };
    Ok(GitHubJob {
        request_id: j.request_id,
        repository,
        status: j.status,
        projection: j.projection,
    })
}
pub(super) fn scoped_job(conn: &Connection, tenant: &str, id: &str) -> Result<ScopedJob> {
    let (repo,run,expires,request):(String,String,i64,Option<String>)=conn.query_row("SELECT repository,run_id,expires_at,request FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?.ok_or(StoreError::NotFound)?;
    let row:Option<(String,Option<String>)>=conn.query_row("SELECT commitment,receipt FROM credential_broker_commitments WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let mut projection = None;
    let cancelled:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM credential_broker_invocations WHERE tenant_id=?1 AND run_id=?2 AND cancelled=1)",params![tenant,run],|r|r.get(0))?;
    let current:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM credential_broker_jobs j JOIN accounts a ON a.tenant_id=j.tenant_id AND a.id=j.account_id WHERE j.tenant_id=?1 AND j.request_id=?2 AND j.auth_epoch=a.auth_epoch AND a.status='active' AND a.recovery_required=0 AND a.must_change_password=0)",params![tenant,id],|r|r.get(0))?;
    let status = if let Some((_, receipt)) = row {
        if let Some(receipt) = receipt {
            let r: SignedReceipt = decode(&receipt)?;
            match r.receipt.outcome {
                Outcome::ProjectedSuccess => {
                    if current && !cancelled && device_current(conn, &r)? {
                        projection = Some(r.receipt.projection);
                    }
                    "complete"
                }
                Outcome::NotDispatched => "not_dispatched",
                Outcome::Rejected => "credential_rejected",
                Outcome::Unknown => "outcome_unknown",
            }
        } else {
            "in_flight_or_unknown"
        }
    } else if !current {
        "authority_changed"
    } else if cancelled {
        "not_dispatched"
    } else if expires <= now()? {
        "expired"
    } else if request.is_some() {
        "waiting_for_unlock"
    } else {
        "waiting_for_credential"
    };
    Ok(ScopedJob {
        run_id: run,
        expires_at: expires,
        request_id: id.into(),
        resource: decode(&repo)?,
        status: status.into(),
        projection,
    })
}
pub(super) fn device_current(conn: &Connection, receipt: &SignedReceipt) -> Result<bool> {
    let request = &receipt.receipt.commitment.request;
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM credential_broker_devices d JOIN credential_broker_bindings b ON b.device_id=d.id WHERE d.id=?1 AND d.status='active' AND b.binding_id=?2 AND b.active=1)",params![request.device.id.to_string(),request.binding.id.to_string()],|r|r.get(0))?)
}
/// Only a stored, authenticated, successful owner's preview can supply a selector.
/// Native enforcement separately pins the hidden message ID and credential identity.
pub(super) fn selection_origin(
    conn: &Connection,
    tenant: &str,
    account: &str,
    selection: Uuid,
    preview_id: Option<&str>,
) -> Result<SignedReceipt> {
    let rows=conn.prepare("SELECT j.request_id,c.receipt,i.cancelled,j.auth_epoch,a.auth_epoch FROM credential_broker_jobs j JOIN accounts a ON a.tenant_id=j.tenant_id AND a.id=j.account_id JOIN credential_broker_commitments c ON c.tenant_id=j.tenant_id AND c.run_id=j.run_id JOIN credential_broker_invocations i ON i.tenant_id=j.tenant_id AND i.run_id=j.run_id WHERE j.tenant_id=?1 AND j.account_id=?2 AND json_extract(j.repository,'$.google_inbox')='preview_v1' AND c.receipt IS NOT NULL AND a.status='active' AND a.recovery_required=0 AND a.must_change_password=0 LIMIT 1024")?.query_map(params![tenant,account],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,bool>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, raw, cancelled, old, current) in rows {
        if cancelled || old != current || preview_id.is_some_and(|p| p != id) {
            continue;
        }
        let receipt: SignedReceipt = decode(&raw)?;
        let request = &receipt.receipt.commitment.request;
        if request.expires_at <= now()?
            || receipt.receipt.outcome != Outcome::ProjectedSuccess
            || request.operation != Operation::GoogleInboxPreview
            || !device_current(conn, &receipt)?
        {
            continue;
        }
        let binding: Option<String>=conn.query_row("SELECT binding FROM credential_broker_bindings WHERE device_id=?1 AND binding_id=?2 AND active=1",params![request.device.id.to_string(),request.binding.id.to_string()],|r|r.get(0)).optional()?;
        if binding.map(|s| decode::<Binding>(&s)).transpose()?.as_ref() != Some(&request.binding) {
            continue;
        }
        if let Projection::GoogleInboxPreview { messages, .. } = &receipt.receipt.projection
            && messages.iter().any(|m| m.selection == Some(selection))
        {
            return Ok(receipt);
        }
    }
    Err(StoreError::Forbidden)
}
impl Store {
    pub async fn request_github_metadata(
        &self,
        user: &UserContext,
        id: String,
        repository: GitHubRepository,
    ) -> Result<GitHubJob> {
        if Uuid::parse_str(&id).is_err() || !repository.valid() {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&user,true,false)?;
            let old:Option<String>=db.conn.query_row("SELECT repository FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2",params![u.tenant.0,id],|r|r.get(0)).optional()?;
            let repo=encode(&repository)?;
            if let Some(old)=old {if old!=repo{return Err(StoreError::Conflict)}}else{
                let n:i64=db.conn.query_row("SELECT count(*) FROM credential_broker_jobs WHERE tenant_id=?1",[&u.tenant.0],|r|r.get(0))?;if n>=1024{return Err(StoreError::Busy)}
                db.conn.execute("INSERT INTO credential_broker_jobs(tenant_id,account_id,request_id,run_id,call_id,invocation_nonce,repository,auth_epoch,expires_at,request) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",params![u.tenant.0,u.account.id,id,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),repo,u.epoch,now()?+3600])?;
            }
            job(&db.conn,&u.tenant.0,&id)
        }).await
    }
    pub async fn github_metadata_jobs(&self, user: &UserContext) -> Result<Vec<GitHubJob>> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;
            let ids=db.conn.prepare("SELECT request_id FROM credential_broker_jobs WHERE tenant_id=?1 AND json_extract(repository,'$.google_inbox') IS NULL AND json_extract(repository,'$.google_body') IS NULL AND json_extract(repository,'$.google_model_read') IS NULL AND json_extract(repository,'$.gitea_site') IS NULL ORDER BY rowid DESC LIMIT 30")?.query_map([&u.tenant.0],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            ids.iter().map(|i|job(&db.conn,&u.tenant.0,i)).collect()
        }).await
    }
    pub async fn request_gitea_issues(
        &self,
        user: &UserContext,
        id: String,
        resource: GiteaRepository,
    ) -> Result<ScopedJob> {
        if !Uuid::parse_str(&id).is_ok_and(|u| !u.is_nil()) || !resource.valid() {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;let scope=encode(&Resource::Gitea(resource))?;
        let old:Option<String>=db.conn.query_row("SELECT repository FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2",params![u.tenant.0,id],|r|r.get(0)).optional()?;
        if let Some(old)=old{if old!=scope{return Err(StoreError::Conflict)}}else{let n:i64=db.conn.query_row("SELECT count(*) FROM credential_broker_jobs WHERE tenant_id=?1",[&u.tenant.0],|r|r.get(0))?;if n>=1024{return Err(StoreError::Busy)}
        db.conn.execute("INSERT INTO credential_broker_jobs(tenant_id,account_id,request_id,run_id,call_id,invocation_nonce,repository,auth_epoch,expires_at,request) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",params![u.tenant.0,u.account.id,id,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),scope,u.epoch,now()?+3600])?;}
        scoped_job(&db.conn,&u.tenant.0,&id)}).await
    }
    pub async fn gitea_issues_jobs(&self, user: &UserContext) -> Result<Vec<ScopedJob>> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;let ids=db.conn.prepare("SELECT request_id FROM credential_broker_jobs WHERE tenant_id=?1 AND account_id=?2 AND json_extract(repository,'$.gitea_site') IS NOT NULL ORDER BY rowid DESC LIMIT 30")?.query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;ids.iter().map(|id|scoped_job(&db.conn,&u.tenant.0,id)).collect()}).await
    }
    pub async fn gitea_connections(&self, user: &UserContext) -> Result<Vec<GiteaRepository>> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;let rows=db.conn.prepare("SELECT r.repository FROM credential_broker_repositories r JOIN credential_broker_devices d ON d.id=r.device_id JOIN credential_broker_bindings b USING(device_id,binding_id) WHERE d.tenant_id=?1 AND d.account_id=?2 AND d.status='active' AND b.active=1 AND json_extract(b.binding,'$.expires_at')>unixepoch() AND json_extract(r.repository,'$.gitea_site') IS NOT NULL")?.query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;rows.into_iter().map(|r|match decode::<Resource>(&r)?{Resource::Gitea(g)=>Ok(g),_=>Err(StoreError::Corrupt)}).collect()}).await
    }
    pub async fn request_google_inbox(&self, user: &UserContext, id: String) -> Result<ScopedJob> {
        if Uuid::parse_str(&id).is_err() {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&user,true,false)?;let resource=encode(&Resource::google_inbox())?;
            let old:Option<String>=db.conn.query_row("SELECT repository FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2",params![u.tenant.0,id],|r|r.get(0)).optional()?;
            if let Some(old)=old{if old!=resource{return Err(StoreError::Conflict)}}else{
                let n:i64=db.conn.query_row("SELECT count(*) FROM credential_broker_jobs WHERE tenant_id=?1",[&u.tenant.0],|r|r.get(0))?;if n>=1024{return Err(StoreError::Busy)}
                db.conn.execute("INSERT INTO credential_broker_jobs(tenant_id,account_id,request_id,run_id,call_id,invocation_nonce,repository,auth_epoch,expires_at,request) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",params![u.tenant.0,u.account.id,id,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),resource,u.epoch,now()?+3600])?;
            }scoped_job(&db.conn,&u.tenant.0,&id)
        }).await
    }
    pub async fn google_inbox_jobs(&self, user: &UserContext) -> Result<Vec<ScopedJob>> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;
            let ids=db.conn.prepare("SELECT request_id FROM credential_broker_jobs WHERE tenant_id=?1 AND json_extract(repository,'$.google_inbox')='counts_v1' ORDER BY rowid DESC LIMIT 30")?.query_map([&u.tenant.0],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            ids.iter().map(|id|scoped_job(&db.conn,&u.tenant.0,id)).collect()
        }).await
    }
    pub async fn request_google_preview(
        &self,
        user: &UserContext,
        id: String,
    ) -> Result<ScopedJob> {
        if Uuid::parse_str(&id).is_err() {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&user,true,false)?;let resource=encode(&Resource::google_preview())?;
            let old:Option<String>=db.conn.query_row("SELECT repository FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2",params![u.tenant.0,id],|r|r.get(0)).optional()?;
            if let Some(old)=old{if old!=resource{return Err(StoreError::Conflict)}}else{
                let n:i64=db.conn.query_row("SELECT count(*) FROM credential_broker_jobs WHERE tenant_id=?1",[&u.tenant.0],|r|r.get(0))?;if n>=1024{return Err(StoreError::Busy)}
                db.conn.execute("INSERT INTO credential_broker_jobs(tenant_id,account_id,request_id,run_id,call_id,invocation_nonce,repository,auth_epoch,expires_at,request) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",params![u.tenant.0,u.account.id,id,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),resource,u.epoch,now()?+3600])?;
            }scoped_job(&db.conn,&u.tenant.0,&id)
        }).await
    }
    pub async fn google_preview_jobs(&self, user: &UserContext) -> Result<Vec<ScopedJob>> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;
            let ids=db.conn.prepare("SELECT request_id FROM credential_broker_jobs WHERE tenant_id=?1 AND json_extract(repository,'$.google_inbox')='preview_v1' ORDER BY rowid DESC LIMIT 30")?.query_map([&u.tenant.0],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            ids.iter().map(|id|scoped_job(&db.conn,&u.tenant.0,id)).collect()
        }).await
    }
    pub async fn request_google_body(
        &self,
        user: &UserContext,
        id: String,
        preview_id: String,
        selection: Uuid,
    ) -> Result<ScopedJob> {
        if Uuid::parse_str(&id).is_err()
            || Uuid::parse_str(&preview_id).is_err()
            || selection.is_nil()
        {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move|db| {
            let u=validate(&db.conn,&user,true,false)?;
            let resource=encode(&Resource::google_body(selection))?;
            let old:Option<(String,String)>=db.conn.query_row("SELECT repository,account_id FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2",params![u.tenant.0,id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            if let Some((old,account))=old { if account!=u.account.id { return Err(StoreError::Forbidden); }
                if old!=resource { return Err(StoreError::Conflict); } }
            else {
                let origin=selection_origin(&db.conn,&u.tenant.0,&u.account.id,selection,Some(&preview_id))?;
                let n:i64=db.conn.query_row("SELECT count(*) FROM credential_broker_jobs WHERE tenant_id=?1",[&u.tenant.0],|r|r.get(0))?; if n>=1024 { return Err(StoreError::Busy); }
                db.conn.execute("INSERT INTO credential_broker_jobs(tenant_id,account_id,request_id,run_id,call_id,invocation_nonce,repository,auth_epoch,expires_at,request) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL)",params![u.tenant.0,u.account.id,id,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),resource,u.epoch,origin.receipt.commitment.request.expires_at])?;
            }
            scoped_job(&db.conn,&u.tenant.0,&id)
        }).await
    }
    pub async fn google_body_jobs(&self, user: &UserContext) -> Result<Vec<ScopedJob>> {
        let user = user.clone();
        self.call(move|db| { let u=validate(&db.conn,&user,true,false)?;
            let ids=db.conn.prepare("SELECT request_id FROM credential_broker_jobs WHERE tenant_id=?1 AND account_id=?2 AND json_extract(repository,'$.google_body') IS NOT NULL ORDER BY rowid DESC LIMIT 30")?.query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            ids.iter().map(|id|scoped_job(&db.conn,&u.tenant.0,id)).collect()
        }).await
    }
    pub async fn broker_devices(&self, user: &UserContext) -> Result<Vec<(String, String)>> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;
            Ok(db.conn.prepare("SELECT d.id,CASE WHEN d.status!='active' THEN d.status WHEN EXISTS(SELECT 1 FROM sessions s JOIN accounts a ON a.id=s.account_id AND a.tenant_id=s.tenant_id WHERE s.digest=d.session_digest AND s.expires_at>unixepoch() AND s.auth_epoch=a.auth_epoch AND a.status='active' AND a.recovery_required=0) THEN 'active' ELSE 'enrollment_expired' END FROM credential_broker_devices d WHERE d.account_id=?1 AND d.tenant_id=?2")?.query_map(params![u.account.id,u.tenant.0],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        }).await
    }
}
pub(super) fn poll(
    conn: &Connection,
    p: &PairChallenge,
    session: &[u8],
    status: &str,
) -> Result<Reply> {
    if status != "active" {
        return Ok(Reply::Waiting {
            requests: vec![],
            revocation_pending: true,
        });
    }
    let u = active(conn, p, session)?;
    let rows=conn.prepare("SELECT request_id,repository,run_id,call_id,invocation_nonce,expires_at,step FROM credential_broker_jobs WHERE tenant_id=?1 AND account_id=?2 AND auth_epoch=?3 AND expires_at>?4 AND request IS NULL LIMIT 32")?.query_map(params![u.tenant.0,u.account.id,u.epoch,now()?],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,i64>(5)?,r.get::<_,u32>(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, repo, run, call, nonce, expiry, step) in rows {
        if !crate::credential_chat::dispatch_current(conn, &u.tenant.0, &run, step, &call, &nonce)?
        {
            continue;
        }
        let target = encode(&decode::<Resource>(&repo)?.scope())?;
        let mut bindings=conn.prepare("SELECT r.device_id,b.binding FROM credential_broker_repositories r JOIN credential_broker_bindings b USING(device_id,binding_id) JOIN credential_broker_devices d ON d.id=r.device_id JOIN sessions live ON live.digest=d.session_digest WHERE live.expires_at>unixepoch() AND live.auth_epoch=?4 AND d.account_id=?1 AND d.tenant_id=?2 AND d.status='active' AND b.active=1 AND json_extract(b.binding,'$.expires_at')>unixepoch() AND r.repository=?3")?.query_map(params![u.account.id,u.tenant.0,target,u.epoch],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let resource: Resource = decode(&repo)?;
        if matches!(resource, Resource::GoogleModelRead(_)) {
            bindings.retain(|(device, raw)| {
                device == &p.device.id.to_string()
                    && decode::<Binding>(raw).is_ok_and(|binding| {
                        super::native_model_mail::admit(conn, &u, &id, &p.device, &binding)
                            .ok()
                            .flatten()
                            .is_some()
                    })
            });
        }
        if let Resource::GoogleBody(selected) = &resource {
            let Ok(origin) =
                selection_origin(conn, &u.tenant.0, &u.account.id, selected.google_body, None)
            else {
                continue;
            };
            if origin.receipt.commitment.request.device.id != p.device.id {
                continue;
            }
        }
        if bindings.len() != 1 || bindings[0].0 != p.device.id.to_string() {
            continue;
        }
        let b: Binding = decode(&bindings[0].1)?;
        if b.expires_at <= now()? {
            continue;
        }
        let resource: Resource = decode(&repo)?;
        if !resource.valid() {
            return Err(StoreError::Corrupt);
        }
        let request = Request {
            device: p.device.clone(),
            invocation: Invocation {
                run: run.parse().map_err(|_| StoreError::Corrupt)?,
                step,
                call: call.parse().map_err(|_| StoreError::Corrupt)?,
                nonce: nonce.parse().map_err(|_| StoreError::Corrupt)?,
            },
            binding: b.clone(),
            operation: resource.operation(),
            repository: resource.repository().cloned(),
            digest: Sha256::digest(resource.bytes()).into(),
            expires_at: expiry.min(b.expires_at),
        };
        let mut request = request;
        if matches!(request.operation, Operation::GoogleMailModelRead { .. }) {
            let Some(digest) = super::native_model_mail::admit(conn, &u, &id, &p.device, &b)?
            else {
                continue;
            };
            request.digest = digest;
        }
        let json = encode(&request)?;
        conn.execute(
            "INSERT INTO credential_broker_invocations VALUES(?1,?2,?6,?3,?4,?5,0)",
            params![u.tenant.0, run, call, nonce, json, step],
        )?;
        conn.execute("UPDATE credential_broker_jobs SET request=?1 WHERE tenant_id=?2 AND request_id=?3 AND request IS NULL",params![json,u.tenant.0,id])?;
    }
    let candidates=conn.prepare("SELECT i.request FROM credential_broker_invocations i WHERE i.tenant_id=?1 AND i.cancelled=0 AND NOT EXISTS(SELECT 1 FROM credential_broker_commitments c WHERE c.tenant_id=i.tenant_id AND c.run_id=i.run_id) LIMIT 64")?.query_map([&u.tenant.0],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?.into_iter().map(|s|decode::<Request>(&s)).collect::<Result<Vec<_>>>()?.into_iter().filter(|r|r.device==p.device && r.expires_at>now().unwrap_or(i64::MAX)).collect::<Vec<_>>();
    let mut requests = Vec::new();
    for r in candidates {
        if matches!(
            r.operation,
            Operation::GoogleMailSend { .. } | Operation::GoogleMailLookup { .. }
        ) {
            continue;
        }
        if crate::credential_chat::dispatch_current(
            conn,
            &u.tenant.0,
            &r.invocation.run.to_string(),
            r.invocation.step,
            &r.invocation.call.to_string(),
            &r.invocation.nonce.to_string(),
        )? {
            requests.push(r);
        }
    }
    Ok(Reply::Waiting {
        requests,
        revocation_pending: false,
    })
}

/// Only proven uncommitted, uncancelled work may move to a refreshed version of
/// the SAME device/repository grant. Invocation/effect identity never changes.
pub(super) fn refresh_uncommitted(
    conn: &Connection,
    device: &Device,
    expected: &Binding,
    binding: &Binding,
    repository: &GitHubRepository,
) -> Result<()> {
    refresh_resource(
        conn,
        device,
        expected,
        binding,
        &Resource::GitHub(repository.clone()),
    )
}
pub(super) fn refresh_resource(
    conn: &Connection,
    device: &Device,
    expected: &Binding,
    binding: &Binding,
    resource: &Resource,
) -> Result<()> {
    let tenant = device.owner.tenant.to_string();
    let rows=conn.prepare("SELECT i.request FROM credential_broker_invocations i WHERE i.tenant_id=?1 AND i.cancelled=0 AND NOT EXISTS(SELECT 1 FROM credential_broker_commitments c WHERE c.tenant_id=i.tenant_id AND c.run_id=i.run_id AND c.step=i.step AND c.call_id=i.call_id AND c.invocation_nonce=i.invocation_nonce)")?.query_map([&tenant],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for text in rows {
        let mut request: Request = decode(&text)?;
        if request.device != *device
            || request.binding != *expected
            || (!matches!(
                (request.operation, resource.operation()),
                (
                    Operation::GoogleInboxBody { .. },
                    Operation::GoogleInboxBody { .. }
                ) | (
                    Operation::GoogleMailModelRead { .. },
                    Operation::GoogleMailModelRead { .. }
                )
            ) && request.operation != resource.operation())
            || request.repository.as_ref() != resource.repository()
        {
            continue;
        }
        request.binding = binding.clone();
        let json = encode(&request)?;
        let i = &request.invocation;
        conn.execute("UPDATE credential_broker_invocations SET request=?1 WHERE tenant_id=?2 AND run_id=?3 AND step=?4 AND call_id=?5 AND invocation_nonce=?6 AND request=?7 AND cancelled=0",params![json,tenant,i.run.to_string(),i.step,i.call.to_string(),i.nonce.to_string(),text])?;
        conn.execute("UPDATE credential_broker_jobs SET request=?1 WHERE tenant_id=?2 AND run_id=?3 AND call_id=?4 AND invocation_nonce=?5 AND request=?6",params![json,tenant,i.run.to_string(),i.call.to_string(),i.nonce.to_string(),text])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory};
    use moosuri_credential_broker::{Engine, auth::SigningKey, github::GitHubAdapter};
    use moosuri_vault_core::{
        Continue, ItemFields, OsEntropy, PendingVault, RecoveryKey, SecretText, StorageReceipt,
        UnlockedArchive, inventory_digest,
    };
    use moosuri_vault_protocol::broker::{
        Binding as BrokerBinding, Command, Request as BrokerRequest,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    async fn signed(s: &Store, e: &Engine, command: Command) -> Result<Reply> {
        let challenge = s.broker_auth_challenge(e.device().id).await?;
        let input = e
            .sign_command(&challenge, "POST", DEVICE_PATH, command, now()?)
            .map_err(|_| StoreError::Forbidden)?;
        s.broker_command(input).await
    }
    fn archive(device: &Device) -> (Vec<u8>, Uuid) {
        let mut entropy = OsEntropy;
        let password = SecretText::new("Synthetic archive password2026".into()).unwrap();
        let mut pending = PendingVault::prepare(
            moosuri_vault_core::Binding {
                owner_id: device.owner.account,
                tenant_id: device.owner.tenant,
                vault_id: Uuid::new_v4(),
            },
            &password,
            &mut entropy,
        )
        .unwrap();
        let recovery = RecoveryKey::parse(&pending.recovery().expose_encoded()).unwrap();
        pending.verify_recovery(&recovery).unwrap();
        let (id, digest) = pending.candidate().unwrap();
        pending
            .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
            .unwrap();
        let mut active = pending.activate().unwrap();
        let fields:ItemFields=serde_json::from_value(serde_json::json!({"kind":"api_key","title":"Synthetic GitHub token","username":"","url":"https://api.github.com","secret":"SYNTHETIC-TOKEN-CANARY-NEVER-OUTPUT","notes":"","favorite":false,"expires_at":"","totp":""})).unwrap();
        let item = active
            .seal_item(Uuid::new_v4(), 1, &fields, &mut entropy)
            .unwrap();
        let id = item.id;
        let inventory = inventory_digest(std::slice::from_ref(&item)).unwrap();
        (
            active
                .export_archive(&[item], inventory, &Continue)
                .unwrap(),
            id,
        )
    }
    async fn assigned(s: &Store, e: &Engine) -> BrokerRequest {
        let Reply::Waiting { mut requests, .. } = signed(s, e, Command::Poll).await.unwrap() else {
            panic!("not waiting")
        };
        assert_eq!(requests.len(), 1);
        requests.remove(0)
    }
    async fn enter(
        s: &Store,
        e: &mut Engine,
        v: &mut UnlockedArchive,
        r: BrokerRequest,
        adapter: &GitHubAdapter,
    ) -> SignedReceipt {
        let invocation = r.invocation.clone();
        e.prepare_github(v, r, now().unwrap()).unwrap();
        let Reply::Commitment { commitment } = signed(
            s,
            e,
            Command::Commit {
                invocation: invocation.clone(),
            },
        )
        .await
        .unwrap() else {
            panic!("not committed")
        };
        e.accept_commitment(*commitment).unwrap();
        e.execute_github(v, &invocation, adapter).await.unwrap()
    }
    #[tokio::test]
    async fn github_actual_adapter_wait_unlock_reuse_canary_and_lost_ack_preserve_original_identity()
     {
        let _serial = TEST_LOCK.lock().await;
        let dir = directory();
        let s = Store::open(dir.path()).unwrap();
        let u = admin(&s).await.user;
        let repo = GitHubRepository {
            owner: "synthetic-owner".into(),
            repository: "synthetic-repo".into(),
        };
        let first = Uuid::new_v4().to_string();
        assert_eq!(
            s.request_github_metadata(&u, first.clone(), repo.clone())
                .await
                .unwrap()
                .status,
            "waiting_for_credential"
        );
        let key = zeroize::Zeroizing::new(SigningKey::generate_pkcs8().unwrap());
        let signer = SigningKey::from_pkcs8(&key).unwrap();
        let challenge = s
            .broker_pair_challenge(
                &u,
                "https://assistant.example.test".into(),
                signer.public_key(),
            )
            .await
            .unwrap();
        s.broker_pair(PairProof {
            challenge: challenge.nonce,
            signature: signer.sign(&pairing_bytes(&challenge).unwrap()),
        })
        .await
        .unwrap();
        let local = directory();
        let root = local.path().join("broker");
        let mut engine =
            Engine::enroll(&root, challenge.device.clone(), challenge.origin, &key).unwrap();
        let (bytes, item) = archive(engine.device());
        let password = SecretText::new("Synthetic archive password2026".into()).unwrap();
        let mut vault = UnlockedArchive::unlock(&bytes, &password).unwrap();
        let binding: BrokerBinding = engine
            .archive_grant(&mut vault, item, repo.clone(), now().unwrap() + 600)
            .unwrap();
        signed(
            &s,
            &engine,
            Command::RegisterRepository {
                binding: binding.clone(),
                repository: repo.clone(),
            },
        )
        .await
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let shared = count.clone();
        let server = tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut data = [0; 16384];
                let n = socket.read(&mut data).await.unwrap();
                let headers = String::from_utf8_lossy(&data[..n]);
                assert!(headers.starts_with("GET /repos/synthetic-owner/synthetic-repo HTTP/1.1"));
                assert!(
                    headers
                        .to_lowercase()
                        .contains("authorization: bearer synthetic-token-canary-never-output")
                );
                shared.fetch_add(1, Ordering::SeqCst);
                let body = r#"{"id":42,"private":true,"archived":false,"stargazers_count":7,"open_issues_count":2,"token":"SYNTHETIC-TOKEN-CANARY-NEVER-OUTPUT","description":"SYNTHETIC-TOKEN-CANARY-NEVER-OUTPUT"}"#;
                let wire = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                socket.write_all(wire.as_bytes()).await.unwrap();
            }
        });
        let adapter = GitHubAdapter::synthetic_loopback(address).unwrap();
        let request = assigned(&s, &engine).await;
        let invocation = request.invocation.clone();
        let receipt = enter(&s, &mut engine, &mut vault, request, &adapter).await;
        assert!(!serde_json::to_string(&receipt).unwrap().contains("CANARY"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // Simulate successful provider execution + lost assistant settlement ACK and restart.
        drop(engine);
        let mut engine = Engine::open_local(&root).unwrap();
        let retained = engine.receipt(&invocation).unwrap().unwrap();
        assert!(
            engine
                .execute_github(&mut vault, &invocation, &adapter)
                .await
                .is_err()
        );
        signed(
            &s,
            &engine,
            Command::Settle {
                receipt: Box::new(retained.clone()),
            },
        )
        .await
        .unwrap();
        signed(
            &s,
            &engine,
            Command::Settle {
                receipt: Box::new(retained),
            },
        )
        .await
        .unwrap();
        assert_eq!(
            s.github_metadata_jobs(&u).await.unwrap()[0].status,
            "complete"
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        // Same exact scope after local lock/unlock reuses binding, without another selection.
        drop(vault);
        let second = Uuid::new_v4().to_string();
        s.request_github_metadata(&u, second, repo).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let mut vault = UnlockedArchive::unlock(&bytes, &password).unwrap();
        let request = assigned(&s, &engine).await;
        assert_eq!(request.binding.id, binding.id);
        let receipt = enter(&s, &mut engine, &mut vault, request, &adapter).await;
        signed(
            &s,
            &engine,
            Command::Settle {
                receipt: Box::new(receipt),
            },
        )
        .await
        .unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 2);
        // A valid writer commitment response held across the local deadline must
        // not reuse pre-await poll time to authorize provider entry.
        let expiry_id = Uuid::new_v4().to_string();
        let repo = GitHubRepository {
            owner: "synthetic-owner".into(),
            repository: "synthetic-repo".into(),
        };
        s.request_github_metadata(&u, expiry_id, repo)
            .await
            .unwrap();
        let mut expiring = assigned(&s, &engine).await;
        expiring.expires_at = now().unwrap() + 2;
        let stored = expiring.clone();
        s.call(move|db|{db.conn.execute("UPDATE credential_broker_invocations SET request=?1 WHERE tenant_id=?2 AND run_id=?3",params![encode(&stored)?,stored.device.owner.tenant.to_string(),stored.invocation.run.to_string()])?;Ok(())}).await.unwrap();
        engine
            .prepare_github(&vault, expiring.clone(), now().unwrap())
            .unwrap();
        let held = async {
            let committed = signed(
                &s,
                &engine,
                Command::Commit {
                    invocation: expiring.invocation.clone(),
                },
            )
            .await
            .unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            committed
        };
        let Reply::Commitment { commitment } = held.await else {
            panic!("not committed")
        };
        engine.accept_commitment(*commitment).unwrap();
        assert!(
            engine
                .execute_github(&mut vault, &expiring.invocation, &adapter)
                .await
                .is_err()
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "expired held commitment must perform zero new HTTP entries"
        );
        let receipt = engine.settle_without_retry(&expiring.invocation).unwrap();
        assert!(receipt.receipt.outcome == Outcome::NotDispatched);
        signed(
            &s,
            &engine,
            Command::Settle {
                receipt: Box::new(receipt),
            },
        )
        .await
        .unwrap();
        let third = Uuid::new_v4().to_string();
        let repo = GitHubRepository {
            owner: "synthetic-owner".into(),
            repository: "synthetic-repo".into(),
        };
        s.request_github_metadata(&u, third, repo).await.unwrap();
        let request = assigned(&s, &engine).await;
        let mut forged = request.clone();
        forged.repository.as_mut().unwrap().repository = "different-repo".into();
        assert!(
            engine
                .prepare_github(&vault, forged, now().unwrap())
                .is_err()
        );
        engine
            .prepare_github(&vault, request.clone(), now().unwrap())
            .unwrap();
        s.logout(&u).await.unwrap();
        assert!(
            signed(
                &s,
                &engine,
                Command::Commit {
                    invocation: request.invocation
                }
            )
            .await
            .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 2);
        let tenant = u.tenant.0.clone();
        s.call(move |db| {
            for table in [
                "credential_broker_jobs",
                "credential_broker_invocations",
                "credential_broker_commitments",
            ] {
                let fields = db
                    .conn
                    .prepare(&format!("SELECT * FROM {table} WHERE tenant_id=?1"))?
                    .query_map([&tenant], |r| {
                        let mut text = String::new();
                        for i in 0..r.as_ref().column_count() {
                            if let Ok(v) = r.get::<_, String>(i) {
                                text.push_str(&v);
                            }
                        }
                        Ok(text)
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                assert!(!fields.join("").contains("CANARY"));
            }
            Ok(())
        })
        .await
        .unwrap();
        server.abort();
    }
    #[tokio::test]
    async fn google_archive_binding_refresh_lost_ack_keeps_committed_identity_and_expiry() {
        use moosuri_credential_broker::auth::authenticated_bytes;
        async fn issue(
            s: &Store,
            device: &Device,
            key: &SigningKey,
            command: Command,
        ) -> Result<Reply> {
            let challenge = s.broker_auth_challenge(device.id).await?;
            s.broker_command(SignedCommand {
                challenge: challenge.nonce,
                signature: key
                    .sign(&authenticated_bytes(&challenge, "POST", DEVICE_PATH, &command).unwrap()),
                command,
            })
            .await
        }
        let _serial = TEST_LOCK.lock().await;
        let dir = directory();
        let s = Store::open(dir.path()).unwrap();
        let user = admin(&s).await.user;
        let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
        let pair = s
            .broker_pair_challenge(
                &user,
                "https://assistant.example.test".into(),
                key.public_key(),
            )
            .await
            .unwrap();
        let device = s
            .broker_pair(PairProof {
                challenge: pair.nonce,
                signature: key.sign(&pairing_bytes(&pair).unwrap()),
            })
            .await
            .unwrap();
        let old = moosuri_credential_broker::policy::LocalGrant::select_google(
            device.clone(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            moosuri_credential_broker::google::GoogleIdentity {
                subject: "synthetic-subject".into(),
                email: "synthetic@example.test".into(),
            },
            1,
            1,
            now().unwrap() + 600,
        )
        .unwrap()
        .binding;
        issue(
            &s,
            &device,
            &key,
            Command::RegisterGoogleInbox {
                binding: old.clone(),
            },
        )
        .await
        .unwrap();
        let committed_id = Uuid::new_v4().to_string();
        s.request_google_inbox(&user, committed_id.clone())
            .await
            .unwrap();
        let Reply::Waiting { mut requests, .. } =
            issue(&s, &device, &key, Command::Poll).await.unwrap()
        else {
            panic!("waiting")
        };
        let committed_request = requests.remove(0);
        let Reply::Commitment { commitment } = issue(
            &s,
            &device,
            &key,
            Command::CommitCurrent {
                invocation: committed_request.invocation.clone(),
                request_digest: moosuri_credential_broker::auth::hash(&request_bytes(
                    &committed_request,
                )),
            },
        )
        .await
        .unwrap() else {
            panic!("commit")
        };
        let exact = *commitment;
        let waiting_id = Uuid::new_v4().to_string();
        s.request_google_inbox(&user, waiting_id).await.unwrap();
        let Reply::Waiting { mut requests, .. } =
            issue(&s, &device, &key, Command::Poll).await.unwrap()
        else {
            panic!("waiting")
        };
        assert_eq!(requests.len(), 1);
        let waiting = requests.remove(0);
        let mut revised = old.clone();
        revised.epoch += 1;
        revised.grant_version += 1;
        revised.item_epoch = 2;
        revised.grant_hash = [77; 32];
        let refresh = Command::RefreshGoogleInbox {
            expected: old.clone(),
            binding: revised.clone(),
        };
        assert!(matches!(
            issue(&s, &device, &key, refresh.clone()).await.unwrap(),
            Reply::Registered
        ));
        // Registration committed; its reply is lost before the native pending CAS clears.
        drop(s);
        let s = Store::open(dir.path()).unwrap();
        assert!(matches!(
            issue(&s, &device, &key, refresh).await.unwrap(),
            Reply::Registered
        ));
        let mut rotated = revised.clone();
        rotated.epoch += 1;
        rotated.grant_version += 1;
        rotated.item_epoch = 3;
        rotated.key_epoch = 2;
        rotated.grant_hash = [78; 32];
        issue(
            &s,
            &device,
            &key,
            Command::RefreshGoogleInbox {
                expected: revised,
                binding: rotated.clone(),
            },
        )
        .await
        .unwrap();
        let Reply::Waiting { requests, .. } =
            issue(&s, &device, &key, Command::Poll).await.unwrap()
        else {
            panic!("waiting")
        };
        assert_eq!(requests.len(), 1);
        assert!(requests[0].invocation == waiting.invocation);
        assert!(requests[0].binding == rotated);
        assert_eq!(requests[0].expires_at, waiting.expires_at);
        let Reply::Commitment { commitment } = issue(
            &s,
            &device,
            &key,
            Command::Recover {
                invocation: committed_request.invocation.clone(),
            },
        )
        .await
        .unwrap() else {
            panic!("recover")
        };
        assert!(*commitment == exact);
        assert_eq!(rotated.expires_at, old.expires_at);
        let receipt = Receipt {
            commitment: exact,
            outcome: Outcome::Unknown,
            projection: Projection::None,
            projection_digest: moosuri_credential_broker::auth::projection_digest(Projection::None),
        };
        let signed = SignedReceipt {
            signature: key.sign(&receipt_bytes(&receipt)),
            receipt,
        };
        for _ in 0..2 {
            issue(
                &s,
                &device,
                &key,
                Command::Settle {
                    receipt: Box::new(signed.clone()),
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(
            s.google_inbox_jobs(&user)
                .await
                .unwrap()
                .into_iter()
                .find(|j| j.request_id == committed_id)
                .unwrap()
                .status,
            "outcome_unknown"
        );
        let tenant = user.tenant.0.clone();
        s.call(move |db| {
            let count: i64 = db.conn.query_row(
                "SELECT count(*) FROM credential_broker_commitments WHERE tenant_id=?1",
                [tenant],
                |r| r.get(0),
            )?;
            assert_eq!(count, 1);
            Ok(())
        })
        .await
        .unwrap();
    }
}

#[cfg(test)]
mod body_tests {
    use super::*;
    use crate::tests::{TEST_LOCK, admin, directory};
    use moosuri_credential_broker::{auth::SigningKey, google::GoogleIdentity, policy::LocalGrant};
    async fn issue(
        store: &Store,
        key: &SigningKey,
        device: &Device,
        command: Command,
    ) -> Result<Reply> {
        let challenge = store.broker_auth_challenge(device.id).await?;
        let signature = key.sign(
            &moosuri_credential_broker::auth::authenticated_bytes(
                &challenge,
                "POST",
                DEVICE_PATH,
                &command,
            )
            .unwrap(),
        );
        store
            .broker_command(SignedCommand {
                challenge: challenge.nonce,
                command,
                signature,
            })
            .await
    }
    fn signed(key: &SigningKey, commitment: Commitment, projection: Projection) -> SignedReceipt {
        let receipt = Receipt {
            commitment,
            outcome: Outcome::ProjectedSuccess,
            projection_digest: moosuri_credential_broker::auth::projection_digest(
                projection.clone(),
            ),
            projection,
        };
        SignedReceipt {
            signature: key.sign(&receipt_bytes(&receipt)),
            receipt,
        }
    }
    #[tokio::test]
    async fn body_owner_selection_separate_grant_original_expiry_and_revoked_late_result() {
        let _serial = TEST_LOCK.lock().await;
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = admin(&store).await.user;
        let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
        let pair = store
            .broker_pair_challenge(
                &user,
                "https://assistant.example.test".into(),
                key.public_key(),
            )
            .await
            .unwrap();
        let device = store
            .broker_pair(PairProof {
                challenge: pair.nonce,
                signature: key.sign(&pairing_bytes(&pair).unwrap()),
            })
            .await
            .unwrap();
        let item = Uuid::new_v4();
        let vault = Uuid::new_v4();
        let identity = GoogleIdentity {
            subject: "synthetic".into(),
            email: "synthetic@example.test".into(),
        };
        let preview = LocalGrant::select_google_preview(
            device.clone(),
            item,
            vault,
            identity.clone(),
            1,
            1,
            now().unwrap() + 600,
        )
        .unwrap()
        .binding;
        issue(
            &store,
            &key,
            &device,
            Command::RegisterGooglePreview {
                binding: preview.clone(),
            },
        )
        .await
        .unwrap();
        let preview_id = Uuid::new_v4().to_string();
        store
            .request_google_preview(&user, preview_id.clone())
            .await
            .unwrap();
        let Reply::Waiting { mut requests, .. } =
            issue(&store, &key, &device, Command::Poll).await.unwrap()
        else {
            panic!("waiting")
        };
        assert_eq!(requests.len(), 1);
        let request = requests.remove(0);
        let Reply::Commitment { commitment } = issue(
            &store,
            &key,
            &device,
            Command::CommitCurrent {
                invocation: request.invocation.clone(),
                request_digest: moosuri_credential_broker::auth::hash(&request_bytes(&request)),
            },
        )
        .await
        .unwrap() else {
            panic!("commit")
        };
        let selection = Uuid::new_v4();
        let receipt = signed(
            &key,
            *commitment,
            Projection::GoogleInboxPreview {
                messages: vec![GoogleMessagePreview {
                    selection: Some(selection),
                    subject: "Synthetic".into(),
                    from: "Sender".into(),
                    date: "".into(),
                    snippet: "preview".into(),
                    truncated: false,
                }],
                unavailable: 0,
                more_available: false,
            },
        );
        issue(
            &store,
            &key,
            &device,
            Command::Settle {
                receipt: Box::new(receipt),
            },
        )
        .await
        .unwrap();
        assert!(
            store
                .request_google_body(
                    &user,
                    Uuid::new_v4().to_string(),
                    preview_id.clone(),
                    Uuid::new_v4()
                )
                .await
                .is_err()
        );
        assert!(
            store
                .request_google_body(
                    &user,
                    Uuid::new_v4().to_string(),
                    Uuid::new_v4().to_string(),
                    selection
                )
                .await
                .is_err()
        );
        let body_id = Uuid::new_v4().to_string();
        let waiting = store
            .request_google_body(&user, body_id.clone(), preview_id.clone(), selection)
            .await
            .unwrap();
        assert_eq!(waiting.status, "waiting_for_credential");
        assert_eq!(waiting.expires_at, request.expires_at);
        let Reply::Waiting { requests, .. } =
            issue(&store, &key, &device, Command::Poll).await.unwrap()
        else {
            panic!("waiting")
        };
        assert!(requests.is_empty(), "preview grant cannot escalate");
        let body = LocalGrant::select_google_body(
            device.clone(),
            item,
            vault,
            identity,
            1,
            1,
            now().unwrap() + 600,
        )
        .unwrap()
        .binding;
        issue(
            &store,
            &key,
            &device,
            Command::RegisterGoogleBody {
                binding: body.clone(),
            },
        )
        .await
        .unwrap();
        let Reply::Waiting { mut requests, .. } =
            issue(&store, &key, &device, Command::Poll).await.unwrap()
        else {
            panic!("waiting")
        };
        assert_eq!(requests.len(), 1);
        let request = requests.remove(0);
        assert!(request.operation == Operation::GoogleInboxBody { selection });
        assert!(request.binding == body);
        assert_eq!(
            store
                .request_google_body(&user, body_id.clone(), preview_id.clone(), selection)
                .await
                .unwrap()
                .request_id,
            body_id
        );
        let Reply::Commitment { commitment } = issue(
            &store,
            &key,
            &device,
            Command::CommitCurrent {
                invocation: request.invocation.clone(),
                request_digest: moosuri_credential_broker::auth::hash(&request_bytes(&request)),
            },
        )
        .await
        .unwrap() else {
            panic!("commit")
        };
        store.broker_revoke(&user, device.id).await.unwrap();
        let receipt = signed(
            &key,
            *commitment,
            Projection::GoogleInboxBody {
                text: "SYNTHETIC PRIVATE BODY".into(),
                truncated: false,
                unavailable: false,
            },
        );
        for _ in 0..2 {
            issue(
                &store,
                &key,
                &device,
                Command::Settle {
                    receipt: Box::new(receipt.clone()),
                },
            )
            .await
            .unwrap();
        }
        let jobs = store.google_body_jobs(&user).await.unwrap();
        assert_eq!(jobs.len(), 1);
        assert!(
            jobs[0].projection.is_none(),
            "late signed settlement stays private after revoke"
        );
        assert!(
            store
                .request_google_body(&user, Uuid::new_v4().to_string(), preview_id, selection)
                .await
                .is_err()
        );
    }
}
