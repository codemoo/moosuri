//! Provider drafts share the existing MailAction, but never its send approval.
//! Gmail has no idempotency key for a draft POST. Persist before IO and retain
//! an unresolved write after restart; a retry may read a receipt, never POST twice.
use super::*;
use moosuri_core::{GmailDraftRef, GmailDraftSave};

#[derive(Clone)]
pub(crate) struct ProviderDraftOwner {
    pub tenant: String,
    pub account: String,
    pub epoch: i64,
}
struct Snapshot {
    connection_id: String,
    revision: i64,
    digest: String,
}
pub struct ProviderDraftPermit {
    owner: ProviderDraftOwner,
    action: String,
    digest: String,
    nonce: Vec<u8>,
    target_draft: Option<String>,
}
impl ProviderDraftPermit {
    pub fn target_draft(&self) -> Option<&str> {
        self.target_draft.as_deref()
    }
}
pub enum ProviderDraftSave {
    Dispatch {
        permit: ProviderDraftPermit,
        draft: Box<FrozenMail>,
    },
    Recorded {
        receipt: GmailDraftRef,
        mime_sha256: String,
    },
    Reconcile {
        permit: ProviderDraftPermit,
        mime_sha256: String,
    },
}

fn operation_digest(input: &GmailDraftSave) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(json(input)?.as_bytes())))
}

fn current(
    c: &Connection,
    u: &ProviderDraftOwner,
    id: &str,
    expected: &MailApproval,
) -> Result<Snapshot> {
    let (connection_id,revision,digest,state,content,expires,connection_epoch,auth_epoch):(String,i64,String,String,bool,i64,i64,i64)=c.query_row("SELECT connection_id,revision,digest,state,content IS NOT NULL,expires_at,connection_epoch,auth_epoch FROM mail_actions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant,u.account,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional()?.ok_or(StoreError::NotFound)?;
    let connection = connection_owned(c, &u.tenant, &u.account, &connection_id)?;
    if revision != expected.revision
        || digest != expected.digest
        || !matches!(state.as_str(), "pending_approval" | "approved")
        || !content
        || expires <= now()?
        || connection.status != "active"
        || !connection.compose_enabled
        || connection.native_send
        || (connection_epoch, auth_epoch) != (connection.epoch, u.epoch)
    {
        return Err(StoreError::Conflict);
    }
    Ok(Snapshot {
        connection_id,
        revision,
        digest,
    })
}
pub(crate) fn recorded(
    c: &Connection,
    owner: &ProviderDraftOwner,
    action_id: &str,
    input: &GmailDraftSave,
) -> Result<Option<ProviderDraftSave>> {
    current(c, owner, action_id, &input.expected)?;
    let digest = operation_digest(input)?;
    let row:Option<(String,Option<String>,Vec<u8>,String)>=c.query_row("SELECT state,receipt,nonce,mime_sha256 FROM mail_provider_saves WHERE tenant_id=?1 AND account_id=?2 AND action_id=?3 AND operation_digest=?4",params![owner.tenant,owner.account,action_id,digest],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    match row {
        None => Ok(None),
        Some((state, receipt, nonce, mime_sha256)) => match state.as_str() {
            "saved" => Ok(Some(ProviderDraftSave::Recorded {
                receipt: parse(&receipt.ok_or(StoreError::Corrupt)?)?,
                mime_sha256,
            })),
            "started" | "outcome_unknown" => Ok(Some(ProviderDraftSave::Reconcile {
                permit: ProviderDraftPermit {
                    owner: owner.clone(),
                    action: action_id.into(),
                    digest,
                    nonce,
                    target_draft: input.draft_id.clone(),
                },
                mime_sha256,
            })),
            _ => Err(StoreError::Conflict),
        },
    }
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn begin(
    c: &Connection,
    owner: &ProviderDraftOwner,
    action_id: &str,
    input: &GmailDraftSave,
    app: &GoogleAppPin,
    binding_epoch: i64,
    binding_revision: i64,
) -> Result<ProviderDraftSave> {
    let a = current(c, owner, action_id, &input.expected)?;
    crate::google_setup::guard_app(c, app)?;
    let pinned:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND epoch=?4 AND credential_revision=?5 AND oauth_client_id=?6 AND status='active')",params![owner.tenant,owner.account,a.connection_id,binding_epoch,binding_revision,app.client_id],|r|r.get(0))?;
    if !pinned {
        return Err(StoreError::Conflict);
    }
    if let Some(saved) = recorded(c, owner, action_id, input)? {
        return Ok(saved);
    }
    let pending:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mail_provider_saves WHERE tenant_id=?1 AND action_id=?2 AND state IN('started','outcome_unknown','quarantined'))",params![owner.tenant,action_id],|r|r.get(0))?;
    if pending {
        return Err(StoreError::Conflict);
    }
    let digest = operation_digest(input)?;
    let nonce = hash(&random()?);
    let epoch = connection_owned(c, &owner.tenant, &owner.account, &a.connection_id)?.epoch;
    let (mime, reply): (Vec<u8>, Option<String>) = c.query_row(
        "SELECT raw,reply_thread FROM mail_actions WHERE tenant_id=?1 AND id=?2",
        params![owner.tenant, action_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let original = FrozenMail::restore(mime, digest_bytes(&a.digest)?, reply)
        .map_err(|_| StoreError::Corrupt)?;
    let draft = original
        .draft_copy(Uuid::new_v4())
        .map_err(|_| StoreError::InvalidInput)?;
    let mime_sha256 = draft.mime_sha256();
    c.execute(
        "INSERT INTO mail_provider_saves VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'started',NULL,?10,?11)",
        params![
            owner.tenant,
            owner.account,
            action_id,
            digest,
            a.revision,
            a.connection_id,
            epoch,
            owner.epoch,
            nonce,
            now()?,
            mime_sha256
        ],
    )?;
    Ok(ProviderDraftSave::Dispatch {
        permit: ProviderDraftPermit {
            owner: owner.clone(),
            action: action_id.into(),
            digest,
            nonce,
            target_draft: input.draft_id.clone(),
        },
        draft: Box::new(draft),
    })
}

impl Store {
    /// Replays only a recorded result or offers readback of the original draft.
    pub async fn recorded_provider_draft_save(
        &self,
        user: &UserContext,
        action_id: String,
        input: GmailDraftSave,
    ) -> Result<Option<ProviderDraftSave>> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            recorded(
                &db.conn,
                &ProviderDraftOwner {
                    tenant: user.tenant.0,
                    account: user.account.id,
                    epoch: user.epoch,
                },
                &action_id,
                &input,
            )
        })
        .await
    }

    /// Called after refresh/provider precondition reads and immediately before IO.
    pub async fn begin_provider_draft_save(
        &self,
        user: &UserContext,
        action_id: String,
        input: GmailDraftSave,
        app: GoogleAppPin,
        binding_epoch: i64,
        binding_revision: i64,
    ) -> Result<ProviderDraftSave> {
        let user = user.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            validate(&tx, &user, true, false)?;
            let result = begin(
                &tx,
                &ProviderDraftOwner {
                    tenant: user.tenant.0,
                    account: user.account.id,
                    epoch: user.epoch,
                },
                &action_id,
                &input,
                &app,
                binding_epoch,
                binding_revision,
            )?;
            tx.commit()?;
            Ok(result)
        })
        .await
    }

    /// Settlement records original IO even after cancellation/session loss. The
    /// permit cannot authorize IO and the caller separately guards UI disclosure.
    pub async fn finish_provider_draft_save(
        &self,
        permit: ProviderDraftPermit,
        receipt: Option<GmailDraftRef>,
    ) -> Result<()> {
        self.call(move |db| {
            if let (Some(target), Some(receipt)) = (&permit.target_draft,&receipt) && target != &receipt.id { return Err(StoreError::InvalidInput); }
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let state = if receipt.is_some() { "saved" } else { "outcome_unknown" };
            let encoded = receipt.as_ref().map(json).transpose()?;
            let changed = tx.execute(
                "UPDATE mail_provider_saves SET state=?6,receipt=?7 WHERE tenant_id=?1 AND account_id=?2 AND action_id=?3 AND operation_digest=?4 AND nonce=?5 AND state IN('started','outcome_unknown')",
                params![permit.owner.tenant,permit.owner.account,permit.action,permit.digest,permit.nonce,state,encoded],
            )?;
            if changed != 1 { return Err(StoreError::Conflict); }
            tx.commit()?;
            Ok(())
        }).await
    }
}
