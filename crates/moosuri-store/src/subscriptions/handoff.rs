//! Single-writer staging, validation leases and atomic authenticated receipts.
use super::*;
use moosuri_connectors::chatgpt_oauth::handoff::{
    HandoffContext, HandoffExpectation, HandoffKey, HandoffReceipt, VerifiedImportedSession,
};

pub struct HandoffPreparation {
    pub context: HandoffContext,
    pub key: HandoffKey,
    pub expires_at: i64,
    pub client_id: Option<String>,
    pub subject: Option<String>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffStatus {
    pub phase: String,
    pub receipt: Option<HandoffReceipt>,
    pub receipt_mac: Option<Vec<u8>>,
}
pub struct HandoffValidation {
    user: UserContext,
    pub expected: HandoffExpectation,
    pub key: HandoffKey,
    digest: Vec<u8>,
    nonce: String,
}
pub enum HandoffAdmission {
    Imported(Box<HandoffStatus>),
    Validate(Box<HandoffValidation>),
}
struct Row {
    context: HandoffContext,
    digest: Vec<u8>,
    epoch: i64,
    issued: i64,
    expires: i64,
    phase: String,
    key: Option<Vec<u8>>,
    commitment: Option<Vec<u8>>,
    source: Option<String>,
    import_digest: Option<Vec<u8>>,
    lease: Option<String>,
    until: Option<i64>,
    receipt: Option<String>,
    mac: Option<Vec<u8>>,
}
fn row(c: &Connection, u: &UserContext, id: &str) -> Result<Row> {
    c.query_row("SELECT id,runtime_id,target_host,binding_id,expected_revision,staged_revision,session_digest,auth_epoch,issued_at,expires_at,phase,transfer_key,nonce_commitment,source_host,import_digest,lease_nonce,lease_until,receipt,receipt_mac FROM chatgpt_handoffs WHERE id=?1 AND tenant_id=?2 AND account_id=?3",params![id,u.tenant.0,u.account.id],|r|Ok(Row {
        context:HandoffContext{version:1,ticket:r.get(0)?,runtime:r.get(1)?,target_host:r.get(2)?,binding:r.get(3)?,expected_revision:r.get(4)?,staged_revision:r.get(5)?,tenant:u.tenant.0.clone(),account:u.account.id.clone()},
        digest:r.get(6)?,epoch:r.get(7)?,issued:r.get(8)?,expires:r.get(9)?,phase:r.get(10)?,key:r.get(11)?,commitment:r.get(12)?,source:r.get(13)?,import_digest:r.get(14)?,lease:r.get(15)?,until:r.get(16)?,receipt:r.get(17)?,mac:r.get(18)?,
    })).optional()?.ok_or(StoreError::NotFound)
}
fn status(r: &Row) -> Result<HandoffStatus> {
    Ok(HandoffStatus {
        phase: r.phase.clone(),
        receipt: r
            .receipt
            .as_ref()
            .map(|s| serde_json::from_str(s).map_err(|_| StoreError::Corrupt))
            .transpose()?,
        receipt_mac: r.mac.clone(),
    })
}
fn guard(c: &Connection, u: &UserContext, r: &Row) -> Result<()> {
    validate_handoff(c, u)?;
    if r.digest != u.digest
        || r.epoch != u.epoch
        || r.expires <= now()?
        || !matches!(r.phase.as_str(), "prepared" | "committed" | "validating")
    {
        return Err(StoreError::Conflict);
    }
    let (runtime, host): (String, String) = c.query_row(
        "SELECT runtime_id,host_id FROM subscription_runtime WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if runtime != r.context.runtime || host != r.context.target_host {
        return Err(StoreError::Conflict);
    }
    let b = SubscriptionRef {
        tenant: u.tenant.0.clone(),
        account: u.account.id.clone(),
        id: r.context.binding.clone(),
        revision: r.context.expected_revision,
        model: String::new(),
        effort: None,
    };
    owned(c, &b)?;
    Ok(())
}
fn binding_ref(r: &Row) -> SubscriptionRef {
    SubscriptionRef {
        tenant: r.context.tenant.clone(),
        account: r.context.account.clone(),
        id: r.context.binding.clone(),
        revision: r.context.staged_revision,
        model: String::new(),
        effort: None,
    }
}
fn pins(c: &Connection, key: &[u8; 32], r: &Row) -> Result<(Option<String>, Option<String>)> {
    let b = binding_ref(r);
    let (client,identity):(Option<String>,Option<Vec<u8>>)=c.query_row("SELECT client_id,identity FROM chatgpt_binding_revisions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4",params![b.tenant,b.account,b.id,b.revision],|v|Ok((v.get(0)?,v.get(1)?)))?;
    let subject = identity
        .map(|v| {
            sealer(key)
                .open_identity(&v, &context(c, &b, None)?)
                .map(|i| i.subject().to_owned())
                .map_err(|_| StoreError::Corrupt)
        })
        .transpose()?;
    Ok((client, subject))
}
pub(super) fn expire(c: &Connection) -> Result<()> {
    c.execute("UPDATE chatgpt_handoffs SET phase='expired',transfer_key=NULL,lease_nonce=NULL,lease_until=NULL WHERE phase IN('prepared','committed','validating') AND (expires_at<=?1 OR NOT EXISTS(SELECT 1 FROM sessions s JOIN accounts a ON a.tenant_id=s.tenant_id AND a.id=s.account_id WHERE s.digest=chatgpt_handoffs.session_digest AND s.auth_epoch=chatgpt_handoffs.auth_epoch AND s.auth_epoch=a.auth_epoch AND s.expires_at>?1 AND a.status='active'))",[now()?])?;
    Ok(())
}
impl Store {
    pub async fn prepare_chatgpt_handoff(
        &self,
        u: &UserContext,
        existing: Option<(String, i64)>,
        label: String,
    ) -> Result<HandoffPreparation> {
        let u = u.clone();
        self.call(move|db| {
            let u=validate_handoff(&db.conn,&u)?; expire(&db.conn)?;
            let label=name_valid(label,100)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let count:i64=tx.query_row("SELECT count(*) FROM chatgpt_handoffs WHERE tenant_id=?1 AND account_id=?2 AND phase IN('prepared','committed','validating')",params![u.tenant.0,u.account.id],|r|r.get(0))?;
            if count>=4 {return Err(StoreError::Conflict);}
            let (runtime,host):(String,String)=tx.query_row("SELECT runtime_id,host_id FROM subscription_runtime WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let (id,expected)=if let Some((id,rev))=existing {
                owned(&tx,&SubscriptionRef{tenant:u.tenant.0.clone(),account:u.account.id.clone(),id:id.clone(),revision:rev,model:String::new(),effort:None})?;
                if rev<0 {return Err(StoreError::InvalidInput);} (id,rev)
            } else {
                let count:i64=tx.query_row("SELECT count(*) FROM chatgpt_bindings WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|r|r.get(0))?;
                if count>=16 {return Err(StoreError::Conflict);}
                let id=Uuid::new_v4().to_string();
                tx.execute("INSERT INTO chatgpt_bindings(tenant_id,account_id,id,current_revision,status) VALUES(?1,?2,?3,0,'needs_reauth')",params![u.tenant.0,u.account.id,id])?;
                tx.execute("INSERT INTO chatgpt_binding_revisions VALUES(?1,?2,?3,0,?4,NULL,NULL)",params![u.tenant.0,u.account.id,id,label])?;
                (id,0)
            };
            let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chatgpt_handoffs WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND phase IN('prepared','committed','validating'))",params![u.tenant.0,u.account.id,id],|r|r.get(0))?;
            if active {return Err(StoreError::Conflict);}
            let (staged,client,identity)=next_registration_revision(&tx,&db.provider_key,&u,&id)?;
            tx.execute("INSERT INTO chatgpt_binding_revisions VALUES(?1,?2,?3,?4,?5,?6,?7)",params![u.tenant.0,u.account.id,id,staged,label,client,identity])?;
            let ctx=HandoffContext{version:1,ticket:Uuid::new_v4().to_string(),runtime,tenant:u.tenant.0.clone(),account:u.account.id.clone(),target_host:host,binding:id,expected_revision:expected,staged_revision:staged};
            let key=HandoffKey::generate().map_err(|_|StoreError::Unavailable)?;
            let aad=ctx.aad().map_err(|_|StoreError::Corrupt)?;
            let cipher=key.with_private_bytes(|k|providers::encrypt(&db.provider_key,k,&aad))?;
            let at=now()?;let expires=u.absolute_deadline.map_or(at+600,|deadline|deadline.min(at+600));
            tx.execute("INSERT INTO chatgpt_handoffs(id,tenant_id,account_id,session_digest,auth_epoch,runtime_id,target_host,binding_id,expected_revision,staged_revision,transfer_key,issued_at,expires_at,phase) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'prepared')",params![ctx.ticket,ctx.tenant,ctx.account,u.digest.as_slice(),u.epoch,ctx.runtime,ctx.target_host,ctx.binding,expected,staged,cipher,at,expires])?;
            let r=row(&tx,&u,&ctx.ticket)?;
            let (client_id,subject)=pins(&tx,&db.provider_key,&r)?;
            tx.commit()?;
            Ok(HandoffPreparation{context:ctx,key,expires_at:expires,client_id,subject})
        }).await
    }
    pub async fn commit_chatgpt_handoff_nonce(
        &self,
        u: &UserContext,
        id: String,
        commitment: Vec<u8>,
        source_host: String,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move|db| {
            let r=row(&db.conn,&u,&id)?;guard(&db.conn,&u,&r)?;
            if commitment.len()!=32 || IssuedRegistration::from_saved_metadata("metadata-validation".into(),source_host.clone()).is_err() || source_host==r.context.target_host {return Err(StoreError::InvalidInput);}
            if let Some(old)=r.commitment {return if old==commitment && r.source.as_ref()==Some(&source_host) {Ok(())}else{Err(StoreError::Conflict)};}
            if r.phase!="prepared" {return Err(StoreError::Conflict);}
            db.conn.execute("UPDATE chatgpt_handoffs SET nonce_commitment=?2,source_host=?3,phase='committed' WHERE id=?1",params![id,commitment,source_host])?;Ok(())
        }).await
    }
    pub async fn save_chatgpt_handoff_registration(
        &self,
        u: &UserContext,
        id: String,
        client: String,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move|db| {
            let r=row(&db.conn,&u,&id)?;guard(&db.conn,&u,&r)?;
            if r.phase!="committed" || r.import_digest.is_some() {return Err(StoreError::Conflict);}
            IssuedRegistration::from_saved_metadata(client.clone(),r.source.clone().ok_or(StoreError::Conflict)?).map_err(|_|StoreError::InvalidInput)?;
            let (old,_)=pins(&db.conn,&db.provider_key,&r)?;
            if old.as_ref().is_some_and(|v|v!=&client) {return Err(StoreError::Conflict);}
            db.conn.execute("UPDATE chatgpt_binding_revisions SET client_id=?5 WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4",params![r.context.tenant,r.context.account,r.context.binding,r.context.staged_revision,client])?;Ok(())
        }).await
    }
    pub async fn chatgpt_handoff_status(
        &self,
        u: &UserContext,
        id: String,
    ) -> Result<HandoffStatus> {
        let u = u.clone();
        self.call(move |db| {
            validate_handoff(&db.conn, &u)?;
            expire(&db.conn)?;
            let r = row(&db.conn, &u, &id)?;
            // Durable receipt may be read after ordinary owner reauthentication.
            if r.phase != "imported" && r.digest != u.digest {
                return Err(StoreError::NotFound);
            }
            status(&r)
        })
        .await
    }
    pub async fn cancel_chatgpt_handoff(&self, u: &UserContext, id: String) -> Result<()> {
        let u = u.clone();
        self.call(move|db| {
            validate_handoff(&db.conn,&u)?;let r=row(&db.conn,&u,&id)?;
            if r.digest!=u.digest {return Err(StoreError::NotFound);}
            if r.phase=="imported" {return Err(StoreError::Conflict);}
            db.conn.execute("UPDATE chatgpt_handoffs SET phase='cancelled',transfer_key=NULL,lease_nonce=NULL,lease_until=NULL WHERE id=?1",[id])?;Ok(())
        }).await
    }
    pub async fn admit_chatgpt_handoff(
        &self,
        u: &UserContext,
        id: String,
        digest: [u8; 32],
    ) -> Result<HandoffAdmission> {
        let u = u.clone();
        self.call(move|db| {
            validate_handoff(&db.conn,&u)?;let r=row(&db.conn,&u,&id)?;
            if r.phase=="imported" {
                if r.import_digest.as_deref()!=Some(digest.as_slice()) {return Err(StoreError::Conflict);}
                return Ok(HandoffAdmission::Imported(Box::new(status(&r)?)));
            }
            guard(&db.conn,&u,&r)?;
            if !matches!(r.phase.as_str(),"committed"|"validating") || r.import_digest.as_ref().is_some_and(|d|d.as_slice()!=digest) {return Err(StoreError::Conflict);}
            if r.phase=="validating" && r.until.is_some_and(|v|v>now().unwrap_or(i64::MAX)) {return Err(StoreError::Busy);}
            let (client,subject)=pins(&db.conn,&db.provider_key,&r)?;
            let key=providers::decrypt(&db.provider_key,&r.key.clone().ok_or(StoreError::Conflict)?,&r.context.aad().map_err(|_|StoreError::Corrupt)?)?;
            let key=HandoffKey::from_private_bytes(&key).map_err(|_|StoreError::Corrupt)?;
            let commitment:[u8;32]=r.commitment.clone().ok_or(StoreError::Conflict)?.try_into().map_err(|_|StoreError::Corrupt)?;
            let nonce=Uuid::new_v4().to_string();
            db.conn.execute("UPDATE chatgpt_handoffs SET phase='validating',import_digest=?2,lease_nonce=?3,lease_until=?4 WHERE id=?1",params![id,digest.as_slice(),nonce,now()?+45])?;
            Ok(HandoffAdmission::Validate(Box::new(HandoffValidation{user:u,expected:HandoffExpectation{context:r.context,commitment,client_id:client.ok_or(StoreError::Conflict)?,subject,source_host:r.source.ok_or(StoreError::Conflict)?,issued_at:r.issued as u64},key,digest:digest.to_vec(),nonce})))
        }).await
    }
    pub async fn fail_chatgpt_handoff(&self, l: HandoffValidation) -> Result<()> {
        self.call(move|db| {
            db.conn.execute("UPDATE chatgpt_handoffs SET phase='committed',lease_nonce=NULL,lease_until=NULL WHERE id=?1 AND phase='validating' AND lease_nonce=?2",params![l.expected.context.ticket,l.nonce])?;Ok(())
        }).await
    }
    pub async fn complete_chatgpt_handoff(
        &self,
        l: HandoffValidation,
        verified: VerifiedImportedSession,
    ) -> Result<HandoffStatus> {
        self.call(move|db| {
            let r=row(&db.conn,&l.user,&l.expected.context.ticket)?;guard(&db.conn,&l.user,&r)?;
            if r.phase!="validating" || r.lease.as_ref()!=Some(&l.nonce) || r.import_digest.as_ref()!=Some(&l.digest) || r.until.is_none_or(|v|v<=now().unwrap_or(i64::MAX)) {return Err(StoreError::Conflict);}
            let bundle=verified.into_target_bundle(&r.context,&l.digest).map_err(|_|StoreError::Conflict)?;let b=binding_ref(&r);
            let (client,subject)=pins(&db.conn,&db.provider_key,&r)?;
            if client.as_deref()!=Some(bundle.identity().client_id()) || subject.as_ref().is_some_and(|s|s!=bundle.identity().subject()) || bundle.identity().host_id()!=r.context.target_host {return Err(StoreError::Conflict);}
            let cipher=sealer(&db.provider_key).seal(&bundle,&context(&db.conn,&b,Some(1))?).map_err(|_|StoreError::Corrupt)?;
            let identity=sealer(&db.provider_key).seal_identity(bundle.identity(),&context(&db.conn,&b,None)?).map_err(|_|StoreError::Corrupt)?;
            let enabled=bundle.plan_grant()==moosuri_connectors::chatgpt_oauth::PlanGrant::Enabled;
            let receipt=HandoffReceipt{version:1,receipt_id:Uuid::new_v4().to_string(),context:r.context.clone(),generation:1,envelope_digest:l.digest};
            let mac=receipt.authenticate(&l.key).map_err(|_|StoreError::Corrupt)?;
            let encoded=serde_json::to_string(&receipt).map_err(|_|StoreError::Corrupt)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Mark imported before current-pointer trigger; all changes share commit.
            tx.execute("UPDATE chatgpt_handoffs SET phase='imported',transfer_key=NULL,lease_nonce=NULL,lease_until=NULL,receipt=?2,receipt_mac=?3 WHERE id=?1",params![r.context.ticket,encoded,mac])?;
            if tx.execute("UPDATE chatgpt_bindings SET current_revision=?5,status=?6,remote_revoke='not_requested' WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND current_revision=?4",params![b.tenant,b.account,b.id,r.context.expected_revision,b.revision,if enabled{"active"}else{"plan_disabled"}])?!=1 {return Err(StoreError::Conflict);}
            tx.execute("DELETE FROM chatgpt_sessions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3",params![b.tenant,b.account,b.id])?;
            tx.execute("INSERT INTO chatgpt_sessions(tenant_id,account_id,binding_id,revision,generation,cipher,expires_at,plan_enabled) VALUES(?1,?2,?3,?4,1,?5,?6,?7)",params![b.tenant,b.account,b.id,b.revision,cipher,bundle.expires_at_unix() as i64,enabled])?;
            tx.execute("UPDATE chatgpt_binding_revisions SET identity=?5 WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3 AND revision=?4",params![b.tenant,b.account,b.id,b.revision,identity])?;
            tx.commit()?;
            Ok(HandoffStatus{phase:"imported".into(),receipt:Some(receipt),receipt_mac:Some(mac)})
        }).await
    }
}

#[cfg(feature = "test-fixtures")]
impl Store {
    /// Synthetic expiry barrier only; absent from ordinary production builds.
    #[doc(hidden)]
    pub async fn synthetic_expire_chatgpt_handoff(
        &self,
        u: &UserContext,
        id: String,
    ) -> Result<()> {
        let u = u.clone();
        self.call(move |db| {
            validate_handoff(&db.conn, &u)?;
            let r = row(&db.conn, &u, &id)?;
            if r.phase == "imported" {
                return Err(StoreError::Conflict);
            }
            db.conn
                .execute("UPDATE chatgpt_handoffs SET expires_at=0 WHERE id=?1", [id])?;
            Ok(())
        })
        .await
    }
}
