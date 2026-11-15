//! Owned creation identity survives response loss and destructive journal replay.
use super::*;

pub(super) async fn create(
    store: &Store,
    user: &UserContext,
    title: String,
    request_id: Option<String>,
) -> Result<ThreadSummary> {
    let title = name_valid(title, 500)?;
    let request_id = request_id
        .map(|value| {
            Uuid::parse_str(&value)
                .map(|id| id.to_string())
                .map_err(|_| StoreError::InvalidInput)
        })
        .transpose()?;
    let user = user.clone();
    store.call(move |db| {
        let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = validate(&tx, &user, true, false)?;
        let id = request_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());
        if request_id.is_some() {
            let digest = Sha256::digest(title.as_bytes()).to_vec();
            let saved: Option<Option<Vec<u8>>> = tx.query_row(
                "SELECT title_sha256 FROM thread_creation_receipts WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",
                params![current.tenant.0, current.account.id, id], |r| r.get(0)
            ).optional()?;
            if let Some(saved) = saved {
                let saved = saved.ok_or(StoreError::NotFound)?;
                if saved != digest { return Err(StoreError::Conflict); }
                let thread = tx.query_row(
                    "SELECT id,title,created_at,updated_at FROM threads WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND mission_internal=0",
                    params![current.tenant.0, current.account.id, id], summary
                ).optional()?.ok_or(StoreError::NotFound)?;
                tx.commit()?;
                return Ok(thread);
            }
            // A matching legacy ID is not proof that this request created it.
            let collision: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND id=?2)",
                params![current.tenant.0, id], |r| r.get(0)
            )?;
            if collision { return Err(StoreError::Conflict); }
            tx.execute(
                "INSERT INTO thread_creation_receipts(tenant_id,account_id,request_id,title_sha256) VALUES(?1,?2,?3,?4)",
                params![current.tenant.0, current.account.id, id, digest]
            )?;
        }
        let time = now()?;
        tx.execute(
            "INSERT INTO threads(tenant_id,id,account_id,title,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?5)",
            params![current.tenant.0, id, current.account.id, title, time]
        )?;
        let thread = ThreadSummary { id, title, created_at: time, updated_at: time };
        tx.commit()?;
        Ok(thread)
    }).await
}

fn summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<ThreadSummary> {
    Ok(ThreadSummary {
        id: row.get(0)?,
        title: row.get(1)?,
        created_at: row.get(2)?,
        updated_at: row.get(3)?,
    })
}

/// The stable request UUID is also its thread ID. A latest deletion therefore
/// fences a request even when restoring a snapshot taken before its creation.
pub(super) fn deleted(conn: &Connection, tenant: &str, target: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO thread_creation_receipts(tenant_id,account_id,request_id,title_sha256) SELECT tenant_id,id,?2,NULL FROM accounts WHERE tenant_id=?1 ON CONFLICT(tenant_id,account_id,request_id) DO UPDATE SET title_sha256=NULL",
        params![tenant, target]
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
