//! Writer-linearized device authority. No secrets or generic effect minting API.
use super::*;
use moosuri_credential_broker::auth::verify;
use moosuri_vault_protocol::broker::Command;
use moosuri_vault_protocol::broker::*;
use serde::{Serialize, de::DeserializeOwned};
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
   Command::Register{binding}=>{
    active(&tx,&p,&session)?;if status!="active"||binding.id.is_nil()||binding.epoch==0||binding.grant_version==0||binding.key_epoch==0||binding.item_epoch==0||binding.expires_at<=now()?||binding.grant_hash==[0;32]{return Err(StoreError::Forbidden)}
    let old:Option<String>=tx.query_row("SELECT binding FROM credential_broker_bindings WHERE device_id=?1 AND binding_id=?2",params![device,binding.id.to_string()],|r|r.get(0)).optional()?;
    let json=encode(&binding)?;if let Some(old)=old{if old!=json{return Err(StoreError::Conflict)}}else{
     let n:i64=tx.query_row("SELECT count(*) FROM credential_broker_bindings WHERE device_id=?1",[&device],|r|r.get(0))?;if n>=256{return Err(StoreError::Busy)}
     tx.execute("INSERT INTO credential_broker_bindings VALUES(?1,?2,?3,1)",params![device,binding.id.to_string(),json])?;
    }Reply::Registered
   },
   Command::Commit{invocation}=>{
    // Exact retries reconcile existing adoption, never mint a new nonce, including
    // after revoke. The original device/generation remains the only owner.
    if let Some(old)=commitment(&tx,&tenant,&invocation)? {
     if old.request.device!=c.device{return Err(StoreError::Forbidden)}Reply::Commitment{commitment:Box::new(old)}
    }else{
     active(&tx,&p,&session)?;if status!="active"{return Err(StoreError::Forbidden)}
     let r=stored_request(&tx,&tenant,&invocation)?;
     if r.device!=c.device || r.expires_at<=now()? || r.expires_at>r.binding.expires_at{return Err(StoreError::Forbidden)}
     let bs:String=tx.query_row("SELECT binding FROM credential_broker_bindings WHERE device_id=?1 AND binding_id=?2 AND active=1",params![device,r.binding.id.to_string()],|r|r.get(0)).optional()?.ok_or(StoreError::Forbidden)?;
     let b:Binding=decode(&bs)?;if b!=r.binding || b.expires_at<=now()?{return Err(StoreError::Forbidden)}
     let committed=Commitment{request:r,nonce:Uuid::new_v4()};
     tx.execute("INSERT INTO credential_broker_commitments VALUES(?1,?2,?3,?4,?5,?6,NULL)",params![tenant,invocation.run.to_string(),invocation.step,invocation.call.to_string(),invocation.nonce.to_string(),encode(&committed)?])?;Reply::Commitment{commitment:Box::new(committed)}
    }
   },
   Command::Recover{invocation}=>{let old=commitment(&tx,&tenant,&invocation)?.ok_or(StoreError::NotFound)?;if old.request.device!=c.device{return Err(StoreError::Forbidden)}Reply::Commitment{commitment:Box::new(old)}},
   Command::Settle{receipt}=>{
    let r=&receipt.receipt;let i=&r.commitment.request.invocation;
    let old=commitment(&tx,&tenant,i)?.ok_or(StoreError::NotFound)?;
    if old!=r.commitment || old.request.device!=c.device || (r.outcome==Outcome::ProjectedSuccess)!=(r.projection==Projection::FixedProbeOk) || r.projection_digest!=moosuri_credential_broker::auth::projection_digest(r.projection){return Err(StoreError::Forbidden)}
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
