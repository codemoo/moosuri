//! Administrator-owned shared model assignments and durable dispatch checks.
use super::*;

pub(super) fn member_selection(
    conn: &Connection,
    tenant: &str,
    account: &str,
) -> Result<Option<ModelSelection>> {
    Ok(conn
        .query_row(
            "SELECT provider_id,revision,model,effort FROM admin_model_assignments
             WHERE tenant_id=?1 AND account_id=?2",
            params![tenant, account],
            selection_row,
        )
        .optional()?)
}

fn member_tenant(conn: &Connection, account: &str) -> Result<String> {
    conn.query_row(
        "SELECT tenant_id FROM accounts WHERE id=?1 AND role='member' AND deleted=0",
        [account],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

fn assignment_settings(conn: &Connection, tenant: &str, account: &str) -> Result<ModelSettings> {
    let selection = member_selection(conn, tenant, account)?;
    let configured = selection.clone().or(selected(conn, None)?);
    let choices = choices(conn, None)?;
    let effective =
        configured.and_then(|s| choices.iter().find(|c| matches_selection(c, &s)).cloned());
    Ok(ModelSettings {
        selection,
        effective,
        choices,
    })
}

impl Store {
    pub async fn account_model_assignment(
        &self,
        actor: &UserContext,
        account: String,
    ) -> Result<ModelSettings> {
        let actor = actor.clone();
        self.call(move |db| {
            validate(&db.conn, &actor, true, true)?;
            let tenant = member_tenant(&db.conn, &account)?;
            assignment_settings(&db.conn, &tenant, &account)
        })
        .await
    }

    pub async fn set_account_model_assignment(
        &self,
        actor: &UserContext,
        account: String,
        selection: Option<ModelSelection>,
    ) -> Result<ModelSettings> {
        let actor = actor.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            validate(&tx, &actor, true, true)?;
            let tenant = member_tenant(&tx, &account)?;
            if let Some(selection) = selection {
                verified(&tx, &selection)?;
                tx.execute(
                    "INSERT INTO admin_model_assignments(tenant_id,account_id,provider_id,revision,model,effort)
                     VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(tenant_id,account_id) DO UPDATE SET
                     provider_id=excluded.provider_id,revision=excluded.revision,model=excluded.model,effort=excluded.effort",
                    params![tenant, account, selection.provider_id, selection.provider_revision,
                            selection.model, selection.effort.unwrap_or_default()],
                )?;
            } else {
                tx.execute(
                    "DELETE FROM admin_model_assignments WHERE tenant_id=?1 AND account_id=?2",
                    params![tenant, account],
                )?;
            }
            let settings = assignment_settings(&tx, &tenant, &account)?;
            tx.commit()?;
            Ok(settings)
        })
        .await
    }
}

/// Acquired work retains an exact pin. Recheck member authorization at dispatch,
/// including work admitted before model assignments were introduced.
pub(super) fn require_member_choice(
    conn: &Connection,
    tenant: &str,
    account: &str,
    choice: &ModelChoice,
) -> Result<()> {
    let role: String = conn
        .query_row(
            "SELECT role FROM accounts WHERE tenant_id=?1 AND id=?2 AND status='active' AND deleted=0",
            params![tenant, account],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict)?;
    if role == "admin" {
        return Ok(());
    }
    let current = member_selection(conn, tenant, account)?.or(selected(conn, None)?);
    if current.is_some_and(|s| matches_selection(choice, &s)) {
        Ok(())
    } else {
        Err(StoreError::Conflict)
    }
}
