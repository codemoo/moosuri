//! New forward request allowance, never historical accounting reconciliation.
use crate::{Result, Store, StoreError, UserContext, now, validate};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

#[derive(Serialize)]
pub struct Status {
    pub accounting_state: &'static str,
    pub restore_id: Option<String>,
    pub allocation_count: i64,
    pub inference_calls: i64,
    pub committed_calls: i64,
    pub stt_duration_ms: i64,
    pub committed_stt_duration_ms: i64,
}
/// Immutable purpose recognition. Existing normal installations remain unrestricted;
/// restored historical rows are evidence and are never changed by allocating.
fn historical(conn: &Connection) -> Result<Option<(String, String)>> {
    let unknown:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM installation_restores WHERE json_extract(trusted_head,'$.purpose')='assistant-historical-restore-v1' AND json_extract(trusted_head,'$.accounting_state')='accounting_unknown')",[],|r|r.get(0))?;
    if !unknown {
        return Ok(None);
    }
    // A subsequent sealed restore retains the unknown lineage but receives a new
    // installation/restore identity. Imported allocations cannot authorize it.
    conn.query_row("SELECT r.restore_id,r.new_installation_id FROM installation_restores r JOIN backup_installation b ON b.installation_id=r.new_installation_id",[],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::Corrupt).map(Some)
}
fn status_in(conn: &Connection) -> Result<Status> {
    let current = historical(conn)?;
    let Some((restore, installation)) = current else {
        return Ok(Status {
            accounting_state: "known_or_not_historical",
            restore_id: None,
            allocation_count: 0,
            inference_calls: 0,
            committed_calls: 0,
            stt_duration_ms: 0,
            committed_stt_duration_ms: 0,
        });
    };
    let (count,calls,ms):(i64,i64,i64)=conn.query_row("SELECT count(*),coalesce(sum(inference_calls),0),coalesce(sum(stt_duration_ms),0) FROM recovery_forward_allocations WHERE restore_id=?1 AND installation_id=?2",params![restore,installation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let (used,duration):(i64,i64)=conn.query_row("SELECT count(*),coalesce(sum(d.duration_ms),0) FROM recovery_forward_dispatches d JOIN recovery_forward_allocations a ON a.id=d.allocation_id WHERE a.restore_id=?1 AND a.installation_id=?2",params![restore,installation],|r|Ok((r.get(0)?,r.get(1)?)))?;
    Ok(Status {
        accounting_state: "accounting_unknown",
        restore_id: Some(restore),
        allocation_count: count,
        inference_calls: calls,
        committed_calls: used,
        stt_duration_ms: ms,
        committed_stt_duration_ms: duration,
    })
}
/// Identity-only binding/heartbeat gate. Remaining allowance is checked only at
/// dispatch, so consuming the last slot cannot revoke that in-flight operation.
pub(crate) fn require_dispatch_identity(conn: &Connection) -> Result<()> {
    let Some((restore, installation)) = historical(conn)? else {
        return Ok(());
    };
    // Heartbeats need identity only, never aggregation over retained commitments.
    let allocated: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM recovery_forward_allocations WHERE restore_id=?1 AND installation_id=?2)",
        params![restore, installation],
        |r| r.get(0),
    )?;
    if allocated {
        Ok(())
    } else {
        Err(StoreError::RecoveryAllocationRequired)
    }
}
/// Caller owns its dispatch transaction. A duplicate commitment never grants
/// another wire call. Unknown outcomes and requests failing before IO consume.
pub(crate) fn reserve(
    conn: &Connection,
    tenant: &str,
    account: &str,
    nonce: &str,
    kind: &str,
    duration_ms: i64,
) -> Result<()> {
    let Some((restore, installation)) = historical(conn)? else {
        return Ok(());
    };
    if nonce.is_empty() || nonce.len() > 256 || !(0..=120000).contains(&duration_ms) {
        return Err(StoreError::InvalidInput);
    }
    let duplicate: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM recovery_forward_dispatches WHERE nonce=?1)",
        [nonce],
        |r| r.get(0),
    )?;
    if duplicate {
        return Err(StoreError::Conflict);
    }
    let status = status_in(conn)?;
    if status.allocation_count == 0 {
        return Err(StoreError::RecoveryAllocationRequired);
    }
    if status.committed_calls >= status.inference_calls
        || status.committed_stt_duration_ms + duration_ms > status.stt_duration_ms
    {
        return Err(StoreError::RecoveryAllowanceExhausted);
    }
    // Allocations form one current-installation pool. Duration may combine
    // separately authorized allocations; assigning provenance to one call slot
    // must not make displayed remaining duration impossible to spend.
    let allocation:Option<String>=conn.query_row("SELECT a.id FROM recovery_forward_allocations a WHERE a.restore_id=?1 AND a.installation_id=?2 AND (SELECT count(*) FROM recovery_forward_dispatches d WHERE d.allocation_id=a.id)<a.inference_calls ORDER BY a.created_at,a.id LIMIT 1",params![restore,installation],|r|r.get(0)).optional()?;
    let allocation = allocation.ok_or(StoreError::RecoveryAllowanceExhausted)?;
    conn.execute(
        "INSERT INTO recovery_forward_dispatches VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            nonce,
            allocation,
            tenant,
            account,
            kind,
            duration_ms,
            now()?
        ],
    )?;
    Ok(())
}
/// Trusted local stopped writer path; never an HTTP/model tool operation.
pub async fn allocate(
    root: &Path,
    restore: &str,
    request: &str,
    calls: i64,
    duration_ms: i64,
) -> Result<Status> {
    let request = Uuid::parse_str(request)
        .map_err(|_| StoreError::InvalidInput)?
        .to_string();
    let restore = restore.to_owned();
    if !(1..=1000).contains(&calls) || !(0..=3600000).contains(&duration_ms) {
        return Err(StoreError::InvalidInput);
    }
    if !root.join("assistant.sqlite").is_file() {
        return Err(StoreError::InvalidInput);
    }
    let store = Store::open(root)?;
    store.call(move|db|{
        let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (current,installation)=historical(&tx)?.ok_or(StoreError::Conflict)?;
        if restore!=current {return Err(StoreError::Conflict)}
        let previous:Option<(String,String,i64,i64)>=tx.query_row("SELECT restore_id,installation_id,inference_calls,stt_duration_ms FROM recovery_forward_allocations WHERE id=?1",[&request],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        if let Some(previous)=previous {
            if previous!=(current,installation,calls,duration_ms){return Err(StoreError::Conflict)}
        } else {
            // Bound persisted allocation count and total admitted call history.
            let count:i64=tx.query_row("SELECT count(*) FROM recovery_forward_allocations",[],|r|r.get(0))?;
            if count>=10000 {return Err(StoreError::BudgetExhausted)}
            tx.execute("INSERT INTO recovery_forward_allocations VALUES(?1,?2,?3,?4,?5,?6)",params![request,current,installation,calls,duration_ms,now()?])?;
        }
        let status=status_in(&tx)?;tx.commit()?;Ok(status)
    }).await
}
pub async fn status(root: &Path) -> Result<Status> {
    if !root.join("assistant.sqlite").is_file() {
        return Err(StoreError::InvalidInput);
    }
    let store = Store::open(root)?;
    store.call(|db| status_in(&db.conn)).await
}
impl Store {
    pub async fn recovery_budget_status(&self, user: &UserContext) -> Result<Status> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            status_in(&db.conn)
        })
        .await
    }
    pub async fn reserve_probe_call(
        &self,
        user: &UserContext,
        binding: &crate::ProviderBinding,
    ) -> Result<()> {
        self.reserve_probe_identity(
            user,
            binding.id.clone(),
            binding.revision,
            binding.subscription.clone(),
        )
        .await
    }
    pub async fn reserve_subscription_probe(
        &self,
        user: &UserContext,
        binding: &crate::SubscriptionRef,
    ) -> Result<()> {
        self.reserve_probe_identity(
            user,
            binding.id.clone(),
            binding.revision,
            Some(binding.clone()),
        )
        .await
    }
    async fn reserve_probe_identity(
        &self,
        user: &UserContext,
        id: String,
        revision: i64,
        subscription: Option<crate::SubscriptionRef>,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(reference)=subscription {
                if reference.tenant!=user.tenant.0 || reference.account!=user.account.id { return Err(StoreError::NotFound); }
                let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM chatgpt_bindings b JOIN chatgpt_sessions s ON s.tenant_id=b.tenant_id AND s.account_id=b.account_id AND s.binding_id=b.id AND s.revision=b.current_revision WHERE b.tenant_id=?1 AND b.account_id=?2 AND b.id=?3 AND b.current_revision=?4 AND b.status='active' AND s.cipher IS NOT NULL AND s.plan_enabled=1 AND s.refresh_state!='rotation_unknown')",params![reference.tenant,reference.account,id,revision],|r|r.get(0))?;
                if !active {return Err(StoreError::Conflict)}
            } else {
                validate(&tx,&user,true,true)?;
                crate::providers::require_binding(&tx,&id,revision)?;
            }
            let nonce=Uuid::new_v4().to_string();
            reserve(
                &tx,
                &user.tenant.0,
                &user.account.id,
                &nonce,
                "probe",
                0,
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
}
