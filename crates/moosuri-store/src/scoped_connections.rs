//! Nonsecret capability discovery. Device-local grants remain authoritative.
use super::*;
use moosuri_core::ScopedConnectionDescriptor;
impl Store {
    pub async fn scoped_connections(
        &self,
        user: &UserContext,
    ) -> Result<Vec<ScopedConnectionDescriptor>> {
        let user = user.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&user,true,false)?;let time=now()?;
            let rows=db.conn.prepare("SELECT d.id,b.binding_id,b.binding,r.repository,b.active,d.status,coalesce(x.alias,'') FROM credential_broker_devices d JOIN credential_broker_bindings b ON b.device_id=d.id JOIN credential_broker_repositories r USING(device_id,binding_id) JOIN sessions s ON s.digest=d.session_digest LEFT JOIN connection_aliases x ON x.tenant_id=d.tenant_id AND x.account_id=d.account_id AND x.binding_id=b.binding_id WHERE d.tenant_id=?1 AND d.account_id=?2 AND s.auth_epoch=?3 AND s.expires_at>?4 LIMIT 256")?.query_map(params![u.tenant.0,u.account.id,u.epoch,time],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,bool>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut out=vec![];
            for(device,binding,raw,resource,active,state,alias)in rows {
                let pin:moosuri_vault_protocol::broker::Binding=serde_json::from_str(&raw).map_err(|_|StoreError::Corrupt)?;
                let resource:serde_json::Value=serde_json::from_str(&resource).map_err(|_|StoreError::Corrupt)?;
                // The projection is an explicit allowlist, never a dump of local account metadata.
                let(operation,resource)=if resource.get("google_model_read").is_some(){("google.mail.model_read.v1".into(),"mailbox".into())}
                else if resource.get("google_body").is_some(){("google.inbox.body.display.v1".into(),"selected_message".into())}
                else if let Some(v)=resource.get("google_inbox").and_then(|v|v.as_str()) {match v {"counts_v1"=>("google.inbox.counts.display.v1".into(),"inbox".into()),"preview_v1"=>("google.inbox.preview.display.v1".into(),"inbox".into()),_=>continue}}
                else if let Ok(repo)=serde_json::from_value::<moosuri_vault_protocol::broker::GitHubRepository>(resource) {if !repo.valid(){continue}("github.repository.read.v1".into(),format!("{}/{}",repo.owner,repo.repository))}
                else{continue};
                out.push(ScopedConnectionDescriptor{binding_id:binding,device_id:device,operation,resource,schema_version:1,state:if !active||state!="active"{"revoked"}else if pin.expires_at<=time{"expired"}else{"ready"}.into(),expires_at_ms:pin.expires_at.checked_mul(1000).ok_or(StoreError::Corrupt)?,alias:(!alias.is_empty()).then_some(alias)});
            }
            Ok(out)
        }).await
    }
}
