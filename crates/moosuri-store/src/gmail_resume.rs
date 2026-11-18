//! Resume only a completed server-issued latest-INBOX setup request, never a
//! generic retry. Identity, original payload and newest-turn fences share the
//! canonical submission transaction, including lost acknowledgements.
use super::*;
use crate::chat::{self, Run, RunLease};
use moosuri_core::{ArtifactTarget, ChatTurn, MailConnectionRecovery, RunState};

fn original(c: &Connection, r: &Run) -> Result<Option<(String, Option<ArtifactTarget>)>> {
    if r.state != RunState::Completed
        || !Store::latest_inbox_intent(&r.text)
        || !matches!(
            crate::mail::latest::recovery_view(c, &r.tenant, &r.id)?,
            Some(MailConnectionRecovery::GwsSetup)
        )
        || crate::media::has_context(c, &r.tenant, &r.id)?
        || crate::missions::is_mission(c, &r.tenant, &r.id)?
    {
        return Ok(None);
    }
    let active: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE tenant_id=?1 AND id=?2 AND auth_epoch=?3 AND status='active' AND recovery_required=0 AND must_change_password=0 AND (temporary_expires_at IS NULL OR temporary_expires_at>unixepoch()))", params![r.tenant,r.account,r.epoch], |row|row.get(0))?;
    if !active {
        return Ok(None);
    }
    let target = c.query_row("SELECT selected_id,selected_revision FROM personal_artifact_run_profiles WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|row|Ok((row.get::<_,Option<String>>(0)?,row.get::<_,Option<i64>>(1)?))).optional()?.and_then(|(id,revision)|Some(ArtifactTarget { id:id?, revision:revision? }));
    // Selected artifact targets bypass the literal read path; do not resume
    // them into a general model/action run.
    if target.is_some() {
        return Ok(None);
    }
    // Reconstruct exactly the original canonical no-media/no-context payload.
    // Extra attachments, schedule/mission context or unknown submit variants
    // therefore cannot be silently discarded during a resume.
    let canonical = if target.is_some() {
        serde_json::to_vec(&(&r.thread, &r.text, &target))
    } else {
        serde_json::to_vec(&(&r.thread, &r.text))
    }
    .map_err(|_| StoreError::Corrupt)?;
    let digest: [u8; 32] = Sha256::digest(canonical).into();
    let stored: Vec<u8> = c.query_row(
        "SELECT payload_digest FROM chat_runs WHERE tenant_id=?1 AND id=?2",
        params![r.tenant, r.id],
        |row| row.get(0),
    )?;
    if stored != digest {
        return Ok(None);
    }
    let identity = serde_json::to_vec(&(
        "gmail-resume-v1",
        &r.tenant,
        &r.account,
        &r.thread,
        &r.id,
        digest,
    ))
    .map_err(|_| StoreError::Corrupt)?;
    let hash: [u8; 32] = Sha256::digest(identity).into();
    let mut bytes: [u8; 16] = hash[..16].try_into().map_err(|_| StoreError::Corrupt)?;
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(Some((Uuid::from_bytes(bytes).to_string(), target)))
}

fn latest(c: &Connection, r: &Run, request: &str) -> Result<bool> {
    // One deterministic child may be the current turn after a lost response.
    // A subsequent actual user turn makes an old card stale even on replay.
    Ok(c.query_row("SELECT NOT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND thread_id=?2 AND sequence>?3 AND (request_id!=?4 OR sequence!=?3+1))",params![r.tenant,r.thread,r.sequence,request],|row|row.get(0))?)
}

pub(crate) fn available(c: &Connection, r: &Run) -> Result<bool> {
    let Some((request, _)) = original(c, r)? else {
        return Ok(false);
    };
    latest(c, r, &request)
}

fn checked(
    c: &Connection,
    user: &UserContext,
    thread: &str,
    id: &str,
) -> Result<(Run, String, Option<ArtifactTarget>)> {
    let user = validate(c, user, true, false)?;
    let r = chat::load_run(c, &user.tenant.0, id)?;
    if r.thread != thread {
        return Err(StoreError::NotFound);
    }
    if r.account != user.account.id {
        return Err(StoreError::Forbidden);
    }
    if r.epoch != user.epoch {
        return Err(StoreError::Conflict);
    }
    let Some((request, target)) = original(c, &r)? else {
        return Err(StoreError::Conflict);
    };
    if !latest(c, &r, &request)? {
        return Err(StoreError::Conflict);
    }
    Ok((r, request, target))
}

impl Store {
    pub async fn check_gmail_resume(
        &self,
        user: &UserContext,
        thread: String,
        run: String,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move |db| checked(&db.conn, &user, &thread, &run).map(|_| ()))
            .await
    }
    pub async fn resume_gmail(
        &self,
        user: &UserContext,
        thread: String,
        run: String,
        fingerprint: [u8; 32],
    ) -> Result<ChatTurn> {
        let user = user.clone();
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let (original,request,target)=checked(&tx,&user,&thread,&run)?;
            if !crate::mail::gws::gws_inbox_confirmed_in(&tx,&user,&fingerprint)? { return Err(StoreError::Conflict); }
            let turn=chat::submit_turn_context_in(&tx,&db.provider_key,&db.web_profile,user,thread,request,original.text,target,vec![],None,None)?;
            let previous:Option<Vec<u8>>=tx.query_row("SELECT fingerprint FROM gmail_resume_bindings WHERE tenant_id=?1 AND run_id=?2",params![original.tenant,turn.run.id],|row|row.get(0)).optional()?;
            if previous.as_ref().is_some_and(|p|p.as_slice()!=fingerprint) { return Err(StoreError::Conflict); }
            tx.execute("INSERT OR IGNORE INTO gmail_resume_bindings VALUES(?1,?2,?3)",params![original.tenant,turn.run.id,fingerprint.as_slice()])?;
            tx.commit()?;
            Ok(turn)
        }).await
    }
    /// Read under the execution lease; consumed by the literal GWS route before
    /// IO and at delivery. A resumed turn cannot select another mailbox/provider.
    pub async fn gmail_resume_fingerprint(&self, lease: &RunLease) -> Result<Option<[u8; 32]>> {
        let lease = lease.clone();
        self.call(move|db| {
            let r=chat::guard(&db.conn,&db.provider_key,&lease)?;
            let pin:Option<Vec<u8>>=db.conn.query_row("SELECT fingerprint FROM gmail_resume_bindings WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|row|row.get(0)).optional()?;
            pin.map(|p|p.try_into().map_err(|_|StoreError::Corrupt)).transpose()
        }).await
    }
}

#[cfg(test)]
mod tests;
