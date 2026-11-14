//! Writer-linearized device authority. No secrets or generic effect minting API.
use super::*;
use moosuri_credential_broker::auth::verify;
use moosuri_vault_protocol::broker::Command;
use moosuri_vault_protocol::broker::*;
use serde::{Serialize, de::DeserializeOwned};
pub mod github;
fn encode<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::Corrupt)
}
fn decode<T: DeserializeOwned>(s: &str) -> Result<T> {
    serde_json::from_str(s).map_err(|_| StoreError::Corrupt)
}
fn check_sig(key: &[u8], bytes: Option<Vec<u8>>, sig: &[u8]) -> Result<()> {
    verify(key, &bytes.ok_or(StoreError::InvalidInput)?, sig).map_err(|_| StoreError::Unauthorized)
}
fn active(conn: &Connection, pair: &PairChallenge, session: &[u8]) -> Result<UserContext> {
    let digest: [u8; 32] = session.try_into().map_err(|_| StoreError::Corrupt)?;
    let u = load_context(conn, digest, false)?;
    let u = validate(conn, &u, true, false)?;
    if vault_authority::owner(conn, &u)? != pair.device.owner {
        return Err(StoreError::Unauthorized);
    }
    Ok(u)
}
pub(crate) fn quarantine(conn: &Connection) -> Result<()> {
    conn.execute_batch("DELETE FROM credential_broker_pairs; DELETE FROM credential_broker_auth; UPDATE credential_broker_devices SET status=CASE WHEN status='active' THEN 'revocation_pending' ELSE status END; UPDATE credential_broker_bindings SET active=0;")?;
    Ok(())
}
fn stored_request(conn: &Connection, tenant: &str, i: &Invocation) -> Result<Request> {
    let s:String=conn.query_row("SELECT request FROM credential_broker_invocations WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND invocation_nonce=?5 AND cancelled=0",params![tenant,i.run.to_string(),i.step,i.call.to_string(),i.nonce.to_string()],|r|r.get(0)).optional()?.ok_or(StoreError::Forbidden)?;
    decode(&s)
}
fn commitment(conn: &Connection, tenant: &str, i: &Invocation) -> Result<Option<Commitment>> {
    let s:Option<String>=conn.query_row("SELECT commitment FROM credential_broker_commitments WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND invocation_nonce=?5",params![tenant,i.run.to_string(),i.step,i.call.to_string(),i.nonce.to_string()],|r|r.get(0)).optional()?;
    s.map(|s| decode(&s)).transpose()
}
fn projection_allowed(outcome: Outcome, p: &Projection, operation: Operation) -> bool {
    match (outcome, p, operation) {
        (Outcome::ProjectedSuccess, Projection::FixedProbeOk, Operation::FixedProbe) => true,
        (
            Outcome::ProjectedSuccess,
            Projection::GitHubRepository { id, .. },
            Operation::GitHubRepositoryRead,
        ) => *id > 0,
        (
            Outcome::ProjectedSuccess,
            Projection::GoogleInbox {
                messages_total,
                messages_unread,
                threads_total,
                threads_unread,
            },
            Operation::GoogleInboxRead,
        ) => {
            messages_unread <= messages_total
                && threads_unread <= threads_total
                && [
                    messages_total,
                    messages_unread,
                    threads_total,
                    threads_unread,
                ]
                .iter()
                .all(|v| **v <= i32::MAX as u32)
        }
        (
            Outcome::ProjectedSuccess,
            Projection::GoogleInboxPreview {
                messages,
                unavailable,
                ..
            },
            Operation::GoogleInboxPreview,
        ) => {
            messages.len() + usize::from(*unavailable) <= 5
                && messages.iter().all(GoogleMessagePreview::valid)
        }
        (Outcome::ProjectedSuccess, _, _) => false,
        (_, Projection::None, _) => true,
        _ => false,
    }
}
fn commit_current(
    conn: &Connection,
    tenant: &str,
    p: &PairChallenge,
    session: &[u8],
    status: &str,
    invocation: &Invocation,
    digest: Option<[u8; 32]>,
) -> Result<Reply> {
    if let Some(old) = commitment(conn, tenant, invocation)? {
        if old.request.device != p.device
            || digest.is_some_and(|d| {
                d != moosuri_credential_broker::auth::hash(&request_bytes(&old.request))
            })
        {
            return Err(StoreError::Forbidden);
        }
        return Ok(Reply::Commitment {
            commitment: Box::new(old),
        });
    }
    active(conn, p, session)?;
    if status != "active" {
        return Err(StoreError::Forbidden);
    }
    let r = stored_request(conn, tenant, invocation)?;
    if digest
        .map(|d| d != moosuri_credential_broker::auth::hash(&request_bytes(&r)))
        .unwrap_or(r.binding.grant_version != 1)
    {
        return Err(StoreError::Forbidden);
    }
    if !crate::credential_chat::dispatch_current(
        conn,
        tenant,
        &invocation.run.to_string(),
        invocation.step,
        &invocation.call.to_string(),
        &invocation.nonce.to_string(),
    )? {
        return Err(StoreError::Forbidden);
    }
    if r.device != p.device || r.expires_at <= now()? || r.expires_at > r.binding.expires_at {
        return Err(StoreError::Forbidden);
    }
    let bs:String=conn.query_row("SELECT binding FROM credential_broker_bindings WHERE device_id=?1 AND binding_id=?2 AND active=1",params![p.device.id.to_string(),r.binding.id.to_string()],|r|r.get(0)).optional()?.ok_or(StoreError::Forbidden)?;
    let b: Binding = decode(&bs)?;
    if b != r.binding || b.expires_at <= now()? {
        return Err(StoreError::Forbidden);
    }
    let committed = Commitment {
        request: r,
        nonce: Uuid::new_v4(),
    };
    conn.execute(
        "INSERT INTO credential_broker_commitments VALUES(?1,?2,?3,?4,?5,?6,NULL)",
        params![
            tenant,
            invocation.run.to_string(),
            invocation.step,
            invocation.call.to_string(),
            invocation.nonce.to_string(),
            encode(&committed)?
        ],
    )?;
    Ok(Reply::Commitment {
        commitment: Box::new(committed),
    })
}
fn register_google(
    conn: &Connection,
    device: &str,
    status: &str,
    binding: &Binding,
    resource: &Resource,
) -> Result<()> {
    if status != "active"
        || binding.id.is_nil()
        || binding.epoch == 0
        || binding.grant_version == 0
        || binding.key_epoch == 0
        || binding.item_epoch == 0
        || binding.expires_at <= now()?
        || binding.grant_hash == [0; 32]
    {
        return Err(StoreError::Forbidden);
    }
    let old: Option<String> = conn
        .query_row(
            "SELECT binding FROM credential_broker_bindings WHERE device_id=?1 AND binding_id=?2",
            params![device, binding.id.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let json = encode(binding)?;
    if let Some(old) = old {
        if old != json {
            return Err(StoreError::Conflict);
        }
    } else {
        let n: i64 = conn.query_row(
            "SELECT count(*) FROM credential_broker_bindings WHERE device_id=?1",
            [device],
            |r| r.get(0),
        )?;
        if n >= 256 {
            return Err(StoreError::Busy);
        }
        conn.execute(
            "INSERT INTO credential_broker_bindings VALUES(?1,?2,?3,1)",
            params![device, binding.id.to_string(), json],
        )?;
    }
    let resource = encode(resource)?;
    let old:Option<String>=conn.query_row("SELECT repository FROM credential_broker_repositories WHERE device_id=?1 AND binding_id=?2",params![device,binding.id.to_string()],|r|r.get(0)).optional()?;
    if old.is_some_and(|v| v != resource) {
        return Err(StoreError::Conflict);
    }
    conn.execute(
        "INSERT OR IGNORE INTO credential_broker_repositories VALUES(?1,?2,?3)",
        params![device, binding.id.to_string(), resource],
    )?;
    Ok(())
}
impl Store {
    pub async fn broker_pair_challenge(
        &self,
        user: &UserContext,
        origin: String,
        public_key: Vec<u8>,
    ) -> Result<PairChallenge> {
        let user = user.clone();
        self.call(move |db| {
            if public_key.len() != 32 || !moosuri_credential_broker::auth::valid_origin(&origin) {
                return Err(StoreError::InvalidInput);
            }
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &user, true, false)?;
            let time = now()?;
            tx.execute(
                "DELETE FROM credential_broker_pairs WHERE expires_at<=?1",
                [time],
            )?;
            let n: i64 = tx.query_row(
                "SELECT count(*) FROM credential_broker_pairs WHERE account_id=?1 AND consumed=0",
                [&u.account.id],
                |r| r.get(0),
            )?;
            if n >= 16 {
                return Err(StoreError::Busy);
            }
            let c = PairChallenge {
                device: Device {
                    owner: vault_authority::owner(&tx, &u)?,
                    id: Uuid::new_v4(),
                    generation: 1,
                },
                origin,
                public_key,
                nonce: Uuid::new_v4(),
                expires_at: time + 60,
            };
            tx.execute(
                "INSERT INTO credential_broker_pairs VALUES(?1,?2,?3,?4,?5,?6,0)",
                params![
                    c.nonce.to_string(),
                    u.account.id,
                    u.tenant.0,
                    u.digest.as_slice(),
                    encode(&c)?,
                    c.expires_at
                ],
            )?;
            tx.commit()?;
            Ok(c)
        })
        .await
    }
    pub async fn broker_pair(&self, proof: PairProof) -> Result<Device> {
        self.call(move|db|{
  let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
  let (s,session):(String,Vec<u8>)=tx.query_row("SELECT challenge,session_digest FROM credential_broker_pairs WHERE nonce=?1 AND expires_at>?2 AND consumed=0",params![proof.challenge.to_string(),now()?],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::Unauthorized)?;
  let c:PairChallenge=decode(&s)?;active(&tx,&c,&session)?;check_sig(&c.public_key,pairing_bytes(&c),&proof.signature)?;
  let n:i64=tx.query_row("SELECT count(*) FROM credential_broker_devices WHERE account_id=?1",[c.device.owner.account.to_string()],|r|r.get(0))?;if n>=64{return Err(StoreError::Busy)}
  tx.execute("UPDATE credential_broker_pairs SET consumed=1 WHERE nonce=?1",[c.nonce.to_string()])?;
  tx.execute("INSERT INTO credential_broker_devices VALUES(?1,?2,?3,?4,?5,?6,'active')",params![c.device.id.to_string(),c.device.owner.account.to_string(),c.device.owner.tenant.to_string(),session,s,c.public_key])?;tx.commit()?;Ok(c.device)
 }).await
    }
    /// Public challenge is non-authorizing, single use and bounded per enrolled device.
    pub async fn broker_auth_challenge(&self, device: Uuid) -> Result<AuthChallenge> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let time = now()?;
            let s: String = tx
                .query_row(
                    "SELECT enrollment FROM credential_broker_devices WHERE id=?1",
                    [device.to_string()],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(StoreError::Unauthorized)?;
            let p: PairChallenge = decode(&s)?;
            tx.execute(
                "DELETE FROM credential_broker_auth WHERE expires_at<=?1",
                [time],
            )?;
            let n: i64 = tx.query_row(
                "SELECT count(*) FROM credential_broker_auth WHERE device_id=?1",
                [device.to_string()],
                |r| r.get(0),
            )?;
            if n >= 16 {
                return Err(StoreError::Busy);
            }
            let c = AuthChallenge {
                device: p.device,
                origin: p.origin,
                nonce: Uuid::new_v4(),
                expires_at: time + 30,
            };
            tx.execute(
                "INSERT INTO credential_broker_auth VALUES(?1,?2,?3,?4)",
                params![
                    c.nonce.to_string(),
                    device.to_string(),
                    encode(&c)?,
                    c.expires_at
                ],
            )?;
            tx.commit()?;
            Ok(c)
        })
        .await
    }
    pub async fn broker_command(&self, input: SignedCommand) -> Result<Reply> {
        self.call(move|db|{
  let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
  let (cs,ps,key,session,status):(String,String,Vec<u8>,Vec<u8>,String)=tx.query_row("SELECT a.challenge,d.enrollment,d.public_key,d.session_digest,d.status FROM credential_broker_auth a JOIN credential_broker_devices d ON d.id=a.device_id WHERE a.nonce=?1 AND a.expires_at>?2",params![input.challenge.to_string(),now()?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?.ok_or(StoreError::Unauthorized)?;
  let c:AuthChallenge=decode(&cs)?;let p:PairChallenge=decode(&ps)?;
  check_sig(&key,moosuri_credential_broker::auth::authenticated_bytes(&c,"POST",DEVICE_PATH,&input.command),&input.signature)?;
  tx.execute("DELETE FROM credential_broker_auth WHERE nonce=?1",[c.nonce.to_string()])?;
  let tenant=c.device.owner.tenant.to_string();let device=c.device.id.to_string();
  let reply=match input.command {
   Command::Abandon {invocation}=>{
    if commitment(&tx,&tenant,&invocation)?.is_some(){return Err(StoreError::Conflict)}
    let json:String=tx.query_row("SELECT request FROM credential_broker_invocations WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND invocation_nonce=?5",params![tenant,invocation.run.to_string(),invocation.step,invocation.call.to_string(),invocation.nonce.to_string()],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound)?;
    let request:Request=decode(&json)?;if request.device!=c.device{return Err(StoreError::Forbidden)}
    tx.execute("UPDATE credential_broker_invocations SET cancelled=1 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND invocation_nonce=?5",params![tenant,invocation.run.to_string(),invocation.step,invocation.call.to_string(),invocation.nonce.to_string()])?;Reply::Abandoned
   },
   Command::Poll=>github::poll(&tx,&p,&session,&status)?,
   Command::RegisterGoogleInbox{binding}=>{
    active(&tx,&p,&session)?;register_google(&tx,&device,&status,&binding,&Resource::google_inbox())?;Reply::Registered
   },
   Command::RefreshGoogleInbox{expected,binding}=>{
    active(&tx,&p,&session)?;
    if status!="active" || binding.id!=expected.id || binding.epoch!=expected.epoch.checked_add(1).ok_or(StoreError::Forbidden)? || binding.grant_version!=expected.grant_version.checked_add(1).ok_or(StoreError::Forbidden)? || binding.item_epoch<expected.item_epoch || binding.key_epoch<expected.key_epoch || binding.expires_at!=expected.expires_at || binding.expires_at<=now()? || binding.grant_hash==[0;32] || binding.grant_hash==expected.grant_hash{return Err(StoreError::Forbidden)}
    let (saved,resource):(String,String)=tx.query_row("SELECT b.binding,r.repository FROM credential_broker_bindings b JOIN credential_broker_repositories r USING(device_id,binding_id) WHERE b.device_id=?1 AND b.binding_id=?2 AND b.active=1",params![device,binding.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::NotFound)?;
    if decode::<Resource>(&resource)?!=Resource::google_inbox(){return Err(StoreError::Forbidden)}
    let current:Binding=decode(&saved)?;if current!=binding{if current!=expected{return Err(StoreError::Conflict)}
        tx.execute("UPDATE credential_broker_bindings SET binding=?1 WHERE device_id=?2 AND binding_id=?3 AND binding=?4",params![encode(&binding)?,device,binding.id.to_string(),saved])?;
        github::refresh_resource(&tx,&c.device,&expected,&binding,&Resource::google_inbox())?;
    }Reply::Registered
   },
   Command::RegisterGooglePreview{binding}=>{
    active(&tx,&p,&session)?;register_google(&tx,&device,&status,&binding,&Resource::google_preview())?;Reply::Registered
   },
   Command::RefreshGooglePreview{expected,binding}=>{
    active(&tx,&p,&session)?;
    if status!="active" || binding.id!=expected.id || binding.epoch!=expected.epoch.checked_add(1).ok_or(StoreError::Forbidden)? || binding.grant_version!=expected.grant_version.checked_add(1).ok_or(StoreError::Forbidden)? || binding.item_epoch<expected.item_epoch || binding.key_epoch<expected.key_epoch || binding.expires_at!=expected.expires_at || binding.expires_at<=now()? || binding.grant_hash==[0;32] || binding.grant_hash==expected.grant_hash{return Err(StoreError::Forbidden)}
    let (saved,resource):(String,String)=tx.query_row("SELECT b.binding,r.repository FROM credential_broker_bindings b JOIN credential_broker_repositories r USING(device_id,binding_id) WHERE b.device_id=?1 AND b.binding_id=?2 AND b.active=1",params![device,binding.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::NotFound)?;
    if decode::<Resource>(&resource)?!=Resource::google_preview(){return Err(StoreError::Forbidden)}
    let current:Binding=decode(&saved)?;if current!=binding{if current!=expected{return Err(StoreError::Conflict)}
        tx.execute("UPDATE credential_broker_bindings SET binding=?1 WHERE device_id=?2 AND binding_id=?3 AND binding=?4",params![encode(&binding)?,device,binding.id.to_string(),saved])?;
        github::refresh_resource(&tx,&c.device,&expected,&binding,&Resource::google_preview())?;
    }Reply::Registered
   },
   Command::RegisterRepository {binding,repository}=>{
    active(&tx,&p,&session)?;
    if status!="active" || !repository.valid() || binding.id.is_nil() || binding.epoch==0 || binding.grant_version==0 || binding.key_epoch==0 || binding.item_epoch==0 || binding.expires_at<=now()? || binding.grant_hash==[0;32] {return Err(StoreError::Forbidden)}
    let old:Option<String>=tx.query_row("SELECT binding FROM credential_broker_bindings WHERE device_id=?1 AND binding_id=?2",params![device,binding.id.to_string()],|r|r.get(0)).optional()?;
    let json=encode(&binding)?;
    if let Some(old)=old {if old!=json {return Err(StoreError::Conflict)}} else {
      let n:i64=tx.query_row("SELECT count(*) FROM credential_broker_bindings WHERE device_id=?1",[&device],|r|r.get(0))?;if n>=256{return Err(StoreError::Busy)}
      tx.execute("INSERT INTO credential_broker_bindings VALUES(?1,?2,?3,1)",params![device,binding.id.to_string(),json])?;
    }
    let repo=encode(&repository)?;
    let previous:Option<String>=tx.query_row("SELECT repository FROM credential_broker_repositories WHERE device_id=?1 AND binding_id=?2",params![device,binding.id.to_string()],|r|r.get(0)).optional()?;
    if previous.as_ref().is_some_and(|v|v!=&repo){return Err(StoreError::Conflict)}
    tx.execute("INSERT OR IGNORE INTO credential_broker_repositories VALUES(?1,?2,?3)",params![device,binding.id.to_string(),repo])?;
    Reply::Registered
   },
   Command::RefreshRepository{expected,binding,repository}=>{
    active(&tx,&p,&session)?;
    if status!="active" || !repository.valid() || binding.id!=expected.id || binding.epoch!=expected.epoch.checked_add(1).ok_or(StoreError::Forbidden)? || binding.grant_version!=expected.grant_version.checked_add(1).ok_or(StoreError::Forbidden)? || binding.item_epoch<expected.item_epoch || binding.key_epoch<expected.key_epoch || binding.expires_at!=expected.expires_at || binding.expires_at<=now()? || binding.grant_hash==[0;32] || binding.grant_hash==expected.grant_hash {return Err(StoreError::Forbidden)}
    let (saved,repo):(String,String)=tx.query_row("SELECT b.binding,r.repository FROM credential_broker_bindings b JOIN credential_broker_repositories r USING(device_id,binding_id) WHERE b.device_id=?1 AND b.binding_id=?2 AND b.active=1",params![device,binding.id.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::NotFound)?;
    if decode::<GitHubRepository>(&repo)?!=repository {return Err(StoreError::Forbidden)}
    let current:Binding=decode(&saved)?;
    if current!=binding {
        if current!=expected{return Err(StoreError::Conflict)}
        tx.execute("UPDATE credential_broker_bindings SET binding=?1 WHERE device_id=?2 AND binding_id=?3 AND binding=?4",params![encode(&binding)?,device,binding.id.to_string(),saved])?;
        github::refresh_uncommitted(&tx,&c.device,&expected,&binding,&repository)?;
    }
    Reply::Registered
   },
   Command::Register{binding}=>{
    active(&tx,&p,&session)?;if status!="active"||binding.id.is_nil()||binding.epoch==0||binding.grant_version==0||binding.key_epoch==0||binding.item_epoch==0||binding.expires_at<=now()?||binding.grant_hash==[0;32]{return Err(StoreError::Forbidden)}
    let old:Option<String>=tx.query_row("SELECT binding FROM credential_broker_bindings WHERE device_id=?1 AND binding_id=?2",params![device,binding.id.to_string()],|r|r.get(0)).optional()?;
    let json=encode(&binding)?;if let Some(old)=old{if old!=json{return Err(StoreError::Conflict)}}else{
     let n:i64=tx.query_row("SELECT count(*) FROM credential_broker_bindings WHERE device_id=?1",[&device],|r|r.get(0))?;if n>=256{return Err(StoreError::Busy)}
     tx.execute("INSERT INTO credential_broker_bindings VALUES(?1,?2,?3,1)",params![device,binding.id.to_string(),json])?;
    }Reply::Registered
   },
   Command::Commit{invocation}=>commit_current(&tx,&tenant,&p,&session,&status,&invocation,None)?,
   Command::CommitCurrent{invocation,request_digest}=>commit_current(&tx,&tenant,&p,&session,&status,&invocation,Some(request_digest))?,
   Command::Recover{invocation}=>{let old=commitment(&tx,&tenant,&invocation)?.ok_or(StoreError::NotFound)?;if old.request.device!=c.device{return Err(StoreError::Forbidden)}Reply::Commitment{commitment:Box::new(old)}},
   Command::Settle{receipt}=>{
    let r=&receipt.receipt;let i=&r.commitment.request.invocation;
    let old=commitment(&tx,&tenant,i)?.ok_or(StoreError::NotFound)?;
    if old!=r.commitment || old.request.device!=c.device || !projection_allowed(r.outcome,&r.projection,old.request.operation) || r.projection_digest!=moosuri_credential_broker::auth::projection_digest(r.projection.clone()){return Err(StoreError::Forbidden)}
    check_sig(&key,Some(receipt_bytes(r)),&receipt.signature)?;
    let previous:Option<String>=tx.query_row("SELECT receipt FROM credential_broker_commitments WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND invocation_nonce=?5",params![tenant,i.run.to_string(),i.step,i.call.to_string(),i.nonce.to_string()],|r|r.get(0))?;
    let json=encode(&receipt)?;if let Some(previous)=previous {if previous!=json{return Err(StoreError::Conflict)}}else{
     tx.execute("UPDATE credential_broker_commitments SET receipt=?1 WHERE tenant_id=?2 AND run_id=?3 AND step=?4 AND call_id=?5 AND invocation_nonce=?6",params![json,tenant,i.run.to_string(),i.step,i.call.to_string(),i.nonce.to_string()])?;
    }
    // No chat/output mutation. Settlement survives cancellation and authority loss.
    Reply::Settled
   },
   Command::AckRevoke=>{if status=="active"{return Err(StoreError::Conflict)}tx.execute("UPDATE credential_broker_devices SET status='revoked' WHERE id=?1",[device])?;Reply::Revoked},
  };tx.commit()?;Ok(reply)
 }).await
    }
    pub async fn broker_revoke(&self, user: &UserContext, device: Uuid) -> Result<()> {
        let user = user.clone();
        self.call(move|db|{
  let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&user,true,false)?;
  if tx.execute("UPDATE credential_broker_devices SET status=CASE WHEN status='active' THEN 'revocation_pending' ELSE status END WHERE id=?1 AND account_id=?2 AND tenant_id=?3",params![device.to_string(),u.account.id,u.tenant.0])?!=1{return Err(StoreError::NotFound)}
  tx.execute("UPDATE credential_broker_bindings SET active=0 WHERE device_id=?1",[device.to_string()])?;tx.commit()?;Ok(())
 }).await
    }
}

#[cfg(any(test, feature = "test-fixtures"))]
impl Store {
    /// Test-only typed authority. Production has no invocation/effect minting bridge.
    pub async fn broker_fixture_probe(
        &self,
        user: &UserContext,
        device: Device,
        binding: Binding,
        digest: [u8; 32],
    ) -> Result<Request> {
        let user = user.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &user, true, false)?;
            if vault_authority::owner(&tx, &u)? != device.owner {
                return Err(StoreError::Forbidden);
            }
            let r = Request {
                device,
                invocation: Invocation {
                    run: Uuid::new_v4(),
                    step: 1,
                    call: Uuid::new_v4(),
                    nonce: Uuid::new_v4(),
                },
                binding,
                operation: Operation::FixedProbe,
                repository: None,
                digest,
                expires_at: now()? + 30,
            };
            tx.execute(
                "INSERT INTO credential_broker_invocations VALUES(?1,?2,?3,?4,?5,?6,0)",
                params![
                    u.tenant.0,
                    r.invocation.run.to_string(),
                    r.invocation.step,
                    r.invocation.call.to_string(),
                    r.invocation.nonce.to_string(),
                    encode(&r)?
                ],
            )?;
            tx.commit()?;
            Ok(r)
        })
        .await
    }
}
#[cfg(test)]
mod tests;
