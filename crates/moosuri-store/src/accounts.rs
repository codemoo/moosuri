//! Administrator mutations are serialized with authentication-epoch adoption.
use super::*;
use moosuri_core::{
    AccountManagementAction, AccountManagementInput, AccountManagementReceipt, AccountStatus,
    ManagedAccount,
};

/// A generated password is returned once and never retained in a receipt or log.
pub struct AccountManagementResult {
    pub receipt: AccountManagementReceipt,
    pub temporary_password: Option<Zeroizing<String>>,
}

pub struct PreparedAccountManagement {
    user: UserContext,
    account_id: String,
    input: AccountManagementInput,
    hash: [u8; 32],
    temporary: Option<Zeroizing<String>>,
    password_hash: Option<String>,
}

fn managed_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ManagedAccount> {
    let status = match row.get::<_, String>(5)?.as_str() {
        "active" => AccountStatus::Active,
        "suspended" => AccountStatus::Suspended,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(ManagedAccount {
        user: summary_row(row)?,
        status,
        revision: row.get(6)?,
        recovery_required: row.get(7)?,
        temporary_expires_at: row.get(8)?,
    })
}

pub(super) fn target(conn: &Connection, id: &str) -> Result<ManagedAccount> {
    conn.query_row("SELECT id,name,email,role,must_change_password,status,management_revision,recovery_required,temporary_expires_at FROM accounts WHERE id=?1", [id], managed_row)
        .optional()?.ok_or(StoreError::NotFound)
}

fn prior(
    conn: &Connection,
    actor: &str,
    id: &str,
    input: &AccountManagementInput,
    hash: &[u8; 32],
) -> Result<Option<AccountManagementReceipt>> {
    let row: Option<(String, Vec<u8>, i64)> = conn.query_row("SELECT target_id,request_digest,resulting_revision FROM account_management_receipts WHERE actor_id=?1 AND request_id=?2", params![actor,input.request_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    match row {
        None => Ok(None),
        Some((target, digest, revision)) if target == id && digest == hash => {
            Ok(Some(AccountManagementReceipt {
                request_id: input.request_id.clone(),
                account_id: id.into(),
                revision,
            }))
        }
        Some(_) => Err(StoreError::Conflict),
    }
}

fn check_target(
    conn: &Connection,
    actor: &UserContext,
    id: &str,
    input: &AccountManagementInput,
) -> Result<ManagedAccount> {
    let current = target(conn, id)?;
    if current.recovery_required || current.revision != input.expected_revision {
        return Err(StoreError::Conflict);
    }
    if current.user.id == actor.account.id
        && !matches!(input.action, AccountManagementAction::EndSessions)
    {
        return Err(StoreError::InvalidInput);
    }
    match &input.action {
        AccountManagementAction::Suspend if current.status != AccountStatus::Active => {
            return Err(StoreError::Conflict);
        }
        AccountManagementAction::Resume if current.status != AccountStatus::Suspended => {
            return Err(StoreError::Conflict);
        }
        AccountManagementAction::ChangeRole { role } if role == &current.user.role => {
            return Err(StoreError::Conflict);
        }
        _ => (),
    }
    if current.status == AccountStatus::Active
        && current.user.role == Role::Admin
        && matches!(
            input.action,
            AccountManagementAction::Suspend
                | AccountManagementAction::ChangeRole { role: Role::Member }
                | AccountManagementAction::ResetPassword
        )
    {
        let others: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE id!=?1 AND role='admin' AND status='active' AND recovery_required=0 AND must_change_password=0)",[id],|r|r.get(0))?;
        if !others {
            return Err(StoreError::Conflict);
        }
    }
    Ok(current)
}

impl Store {
    pub async fn managed_accounts(&self, user: &UserContext) -> Result<Vec<ManagedAccount>> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, true)?;
            let mut stmt = db.conn.prepare("SELECT id,name,email,role,must_change_password,status,management_revision,recovery_required,temporary_expires_at FROM accounts ORDER BY email")?;
            Ok(stmt.query_map([], managed_row)?.collect::<rusqlite::Result<Vec<_>>>()?)
        }).await
    }

    pub async fn prepare_account_management(
        &self,
        user: &UserContext,
        account_id: String,
        input: AccountManagementInput,
    ) -> Result<PreparedAccountManagement> {
        if Uuid::parse_str(&account_id).is_err()
            || Uuid::parse_str(&input.request_id).is_err()
            || input.expected_revision <= 0
        {
            return Err(StoreError::InvalidInput);
        }
        let hash: [u8; 32] = Sha256::digest(
            serde_json::to_vec(&(&account_id, &input)).map_err(|_| StoreError::InvalidInput)?,
        )
        .into();
        let user = user.clone();
        let (check_user, check_id, check_input) = (user.clone(), account_id.clone(), input.clone());
        let receipt = self
            .call(move |db| {
                let current = validate(&db.conn, &check_user, true, true)?;
                if let Some(receipt) = prior(
                    &db.conn,
                    &current.account.id,
                    &check_id,
                    &check_input,
                    &hash,
                )? {
                    return Ok(Some(receipt));
                }
                check_target(&db.conn, &current, &check_id, &check_input)?;
                Ok(None)
            })
            .await?;
        // No unauthenticated caller spends KDF capacity. Recheck all authority
        // after awaiting it, including role changes and competing requests.
        let temporary = if receipt.is_none()
            && matches!(input.action, AccountManagementAction::ResetPassword)
        {
            Some(Zeroizing::new(random_token()?))
        } else {
            None
        };
        let password_hash = match &temporary {
            Some(password) => Some(hash_password(password.clone()).await?),
            None => None,
        };
        Ok(PreparedAccountManagement {
            user,
            account_id,
            input,
            hash,
            temporary,
            password_hash,
        })
    }

    /// Revalidates the complete preparation; callers serialize this short commit
    /// with the delivery gate, never the password hashing above.
    pub async fn apply_account_management(
        &self,
        prepared: PreparedAccountManagement,
    ) -> Result<AccountManagementResult> {
        let PreparedAccountManagement {
            user,
            account_id,
            input,
            hash,
            temporary,
            password_hash,
        } = prepared;
        self.call(move |db| {
            let actor = validate(&db.conn, &user, true, true)?;
            if let Some(receipt) = prior(&db.conn, &actor.account.id, &account_id, &input, &hash)? {
                return Ok(AccountManagementResult { receipt, temporary_password: None });
            }
            let target = check_target(&db.conn, &actor, &account_id, &input)?;
            let (old_epoch, tenant): (i64, String) = db.conn.query_row("SELECT auth_epoch,tenant_id FROM accounts WHERE id=?1", [&account_id], |r| Ok((r.get(0)?,r.get(1)?)))?;
            let epoch = old_epoch.checked_add(1).ok_or(StoreError::Corrupt)?;
            let revision = target.revision.checked_add(1).ok_or(StoreError::Corrupt)?;
            let time = now()?;
            let expires = time.checked_add(TEMP_SECONDS).ok_or(StoreError::Corrupt)?;
            // Crash after this durable revocation cannot restore old authority:
            // existing startup replay locks the target for local recovery.
            db.poisoned = true;
            let record = db.journal.append(&account_id, epoch)?;
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            match &input.action {
                AccountManagementAction::Suspend => { tx.execute("UPDATE accounts SET status='suspended' WHERE id=?1", [&account_id])?; }
                AccountManagementAction::Resume => { tx.execute("UPDATE accounts SET status='active' WHERE id=?1", [&account_id])?; }
                AccountManagementAction::ChangeRole { role } => { tx.execute("UPDATE accounts SET role=?1 WHERE id=?2", params![if *role==Role::Admin {"admin"} else {"member"}, account_id])?; }
                AccountManagementAction::ResetPassword => { tx.execute("UPDATE accounts SET password_hash=?1,must_change_password=1,temporary_expires_at=?2 WHERE id=?3", params![password_hash.ok_or(StoreError::Corrupt)?, expires, account_id])?; }
                AccountManagementAction::EndSessions => (),
            }
            let changed = tx.execute("UPDATE accounts SET auth_epoch=?1,management_revision=?2 WHERE id=?3 AND auth_epoch=?4 AND management_revision=?5 AND recovery_required=0",params![epoch,revision,account_id,old_epoch,target.revision])?;
            if changed != 1 { return Err(StoreError::Conflict); }
            tx.execute("DELETE FROM sessions WHERE tenant_id=?1 AND account_id=?2", params![tenant,account_id])?;
            tx.execute("INSERT INTO account_management_receipts(actor_id,request_id,target_id,request_digest,resulting_revision,created_at) VALUES(?1,?2,?3,?4,?5,?6)",params![actor.account.id,input.request_id,account_id,hash.as_slice(),revision,time])?;
            tx.execute("UPDATE store_meta SET journal_seq=?1,journal_head=?2 WHERE singleton=1",params![record.seq,record.hash.as_slice()])?;
            tx.commit()?;
            db.poisoned = false;
            Ok(AccountManagementResult {
                receipt: AccountManagementReceipt { request_id: input.request_id, account_id, revision },
                temporary_password: temporary,
            })
        }).await
    }
}

#[cfg(test)]
mod tests;
