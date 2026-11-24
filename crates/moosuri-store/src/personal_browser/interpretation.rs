//! A member's semantic request keeps the selected action/revision before applying it.
//! A replay uses that plan and the existing idempotent work commands.
use super::*;
use crate::providers;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

fn associated(request: &Request) -> Vec<u8> {
    let owner = request.scope.owner;
    let mut aad = b"moosuri/member-work-plan/v1".to_vec();
    for id in [
        owner.tenant,
        owner.account,
        owner.volume,
        request.request_id,
    ] {
        aad.extend(id.as_bytes());
    }
    aad.extend(owner.generation.to_be_bytes());
    aad.extend(request.host_epoch.to_be_bytes());
    aad.extend(request.scope.user.epoch.to_be_bytes());
    aad.extend(request.fingerprint);
    aad
}

fn open_plan(key: &[u8; 32], request: &Request, raw: String) -> Result<String> {
    // Before schema 98, pending plans were stored as the ordinary JSON enum.
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|_| StoreError::Corrupt)?;
    let Some(sealed) = value.get("sealed") else {
        return Ok(raw);
    };
    let encoded = sealed.as_str().ok_or(StoreError::Corrupt)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| StoreError::Corrupt)?;
    let plain = providers::decrypt(key, &bytes, &associated(request))?;
    String::from_utf8(plain.to_vec()).map_err(|_| StoreError::Corrupt)
}

#[derive(Clone)]
pub struct Request {
    pub scope: Scope,
    pub request_id: Uuid,
    pub fingerprint: [u8; 32],
    pub host_epoch: i64,
    pub lease: Uuid,
}

pub enum Progress {
    Missing,
    Claimed(i64),
    InFlight,
    Planned(String),
    Done(String),
}

impl Store {
    pub async fn interpretation_progress(&self, request: Request, claim: bool) -> Result<Progress> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let scope = &request.scope;
            scope.authorize(&tx)?;
            let owner = scope.owner;
            let id = request.request_id.to_string();
            let row = tx.query_row(
                "SELECT fingerprint,thread_id,host_epoch,auth_epoch,plan,outcome,lease_until
                 FROM member_conversation_interpretations WHERE request_id=?1 AND tenant_id=?2
                 AND account_id=?3 AND volume_id=?4 AND generation=?5",
                params![id, owner.tenant.to_string(), owner.account.to_string(), owner.volume.to_string(), owner.generation as i64],
                |r| Ok((r.get::<_, Vec<u8>>(0)?,r.get::<_, String>(1)?,r.get::<_, i64>(2)?,
                       r.get::<_, i64>(3)?,r.get::<_, Option<String>>(4)?,r.get::<_, Option<String>>(5)?,r.get::<_, i64>(6)?)),
            ).optional()?;
            if let Some((hash, thread, host, auth, plan, outcome, expires)) = row {
                if hash != request.fingerprint || thread != scope.thread || host != request.host_epoch || auth != scope.user.epoch {
                    return Err(StoreError::Conflict);
                }
                if let Some(value) = outcome { return Ok(Progress::Done(value)); }
                if let Some(value) = plan { return Ok(Progress::Planned(open_plan(&db.provider_key, &request, value)?)); }
                if expires > now()? { return Ok(Progress::InFlight); }
                if !claim { return Ok(Progress::Missing); }
                let attempt: i64 = tx.query_row("UPDATE member_conversation_interpretations
                    SET lease_until=unixepoch()+120, lease=?2, attempt=attempt+1 WHERE request_id=?1
                    RETURNING attempt", params![id,request.lease.to_string()], |r| r.get(0))?;
                tx.commit()?;
                return Ok(Progress::Claimed(attempt));
            }
            if !claim { return Ok(Progress::Missing); }
            tx.execute(
                "INSERT INTO member_conversation_interpretations
                 (request_id,tenant_id,account_id,volume_id,generation,thread_id,host_epoch,auth_epoch,fingerprint,lease_until,lease)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,unixepoch()+120,?10)",
                params![id, owner.tenant.to_string(), owner.account.to_string(), owner.volume.to_string(),
                    owner.generation as i64, scope.thread, request.host_epoch, scope.user.epoch, request.fingerprint.as_slice(),request.lease.to_string()],
            )?;
            tx.commit()?;
            Ok(Progress::Claimed(1))
        }).await
    }

    pub async fn release_interpretation(&self, request: &Request) -> Result<()> {
        let request = request.clone();
        self.call(move |db| {
            request.scope.authorize(&db.conn)?;
            db.conn.execute("UPDATE member_conversation_interpretations SET lease_until=0
                WHERE request_id=?1 AND lease=?2 AND fingerprint=?3 AND plan IS NULL AND outcome IS NULL",
                params![request.request_id.to_string(),request.lease.to_string(),request.fingerprint.as_slice()])?;
            Ok(())
        }).await
    }

    pub async fn recorded_interpretation(&self, scope: Scope, request: Uuid) -> Result<String> {
        self.call(move |db| {
            scope.authorize(&db.conn)?;
            let owner = scope.owner;
            db.conn.query_row("SELECT outcome FROM member_conversation_interpretations WHERE request_id=?1 AND
                thread_id=?2 AND tenant_id=?3 AND account_id=?4 AND volume_id=?5 AND generation=?6 AND outcome IS NOT NULL",
                params![request.to_string(),scope.thread,owner.tenant.to_string(),owner.account.to_string(),owner.volume.to_string(),owner.generation as i64],
                |r| r.get(0)).optional()?.ok_or(StoreError::NotFound)
        }).await
    }

    pub async fn advance_interpretation(
        &self,
        request: Request,
        value: String,
        completed: bool,
        turn: Option<(String, String)>,
    ) -> Result<()> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let scope = &request.scope;
            scope.authorize(&tx)?;
            let owner = scope.owner;
            let value = if completed { value } else {
                let sealed = providers::encrypt(&db.provider_key,value.as_bytes(),&associated(&request))?;
                serde_json::json!({"sealed": URL_SAFE_NO_PAD.encode(sealed)}).to_string()
            };
            let query = if completed {
                "UPDATE member_conversation_interpretations SET outcome=?1,plan='null' WHERE ?11 IS NOT NULL AND plan IS NOT NULL AND (outcome IS NULL OR outcome=?1)"
            } else {
                "UPDATE member_conversation_interpretations SET plan=?1 WHERE lease=?11 AND plan IS NULL AND outcome IS NULL"
            };
            let n = tx.execute(&format!("{query} AND request_id=?2 AND fingerprint=?3 AND host_epoch=?4 AND auth_epoch=?5
                AND tenant_id=?6 AND account_id=?7 AND volume_id=?8 AND generation=?9 AND thread_id=?10"),
                params![value, request.request_id.to_string(), request.fingerprint.as_slice(), request.host_epoch, scope.user.epoch,
                    owner.tenant.to_string(), owner.account.to_string(), owner.volume.to_string(), owner.generation as i64, scope.thread,request.lease.to_string()])?;
            if n != 1 { return Err(StoreError::Conflict); }
            if let Some((message, answer)) = turn {
                let id = request.request_id.to_string();
                let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND account_id=?2 AND thread_id=?3 AND request_id=?4)",
                    params![owner.tenant.to_string(), owner.account.to_string(), scope.thread, id], |r| r.get(0))?;
                if !exists {
                    workflows::member_work_turn(&tx, &scope.user, &scope.thread, &Uuid::new_v4().to_string(),
                        &id, &message, "member-work-result", answer)?;
                }
            }
            tx.commit()?;
            Ok(())
        }).await
    }
}
