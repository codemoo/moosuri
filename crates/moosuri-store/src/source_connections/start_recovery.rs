//! Recoverable custom starts use a generation-bound composite identity, not
//! indefinitely retained global UUIDs. Ordinary pollers retain their old path.
use super::*;
pub(super) fn begin_in(
    conn: &Connection,
    cap: &SourceIngestCapability,
    time: i64,
) -> Result<SourceRunLease> {
    conn.execute("UPDATE source_sync_runs SET state='unknown',error='lease_expired',finished_at=?3 WHERE tenant_id=?1 AND connection_id=?2 AND state='running' AND lease_until<=?3",params![cap.tenant,cap.connection,time])?;
    if active_run(conn, cap)?.is_some() {
        return Err(StoreError::Conflict);
    }
    let changed=conn.execute("UPDATE source_connections SET run_generation=run_generation+1,last_heartbeat=?3 WHERE tenant_id=?1 AND id=?2 AND run_generation<9223372036854775807",params![cap.tenant,cap.connection,time])?;
    if changed != 1 {
        return Err(StoreError::BudgetExhausted);
    }
    let generation = conn.query_row(
        "SELECT run_generation FROM source_connections WHERE tenant_id=?1 AND id=?2",
        params![cap.tenant, cap.connection],
        |r| r.get(0),
    )?;
    let id = Uuid::new_v4().to_string();
    let nonce = digest(&random_token()?).to_vec();
    conn.execute("INSERT INTO source_sync_runs(tenant_id,connection_id,id,generation,config_revision,credential_epoch,nonce,state,started_at,heartbeat_at,lease_until) VALUES(?1,?2,?3,?4,?5,?6,?7,'running',?8,?8,?9)",params![cap.tenant,cap.connection,id,generation,cap.config_revision,cap.credential_epoch,nonce,time,time+LEASE_SECONDS])?;
    prune_source_history(conn, &cap.tenant, &cap.connection, RECEIPT_LIMIT, RUN_LIMIT)?;
    Ok(SourceRunLease {
        cap: cap.clone(),
        id,
        generation,
        nonce,
    })
}
pub(super) fn grant_binding(cap: &SourceIngestCapability) -> Result<String> {
    let scope = json(&(
        "source-start-v1",
        &cap.tenant,
        &cap.account,
        &cap.connection,
        cap.auth_epoch,
        cap.credential_epoch,
        cap.config_revision,
        &cap.token_hash,
    ))?;
    Ok(format!("{:x}", Sha256::digest(scope.as_bytes())))
}
fn recover_in(
    conn: &Connection,
    cap: &SourceIngestCapability,
    input: &SourceRunStartIntent,
    time: i64,
) -> Result<SourceRunStartReceipt> {
    let c = guard(conn, cap)?;
    if cap.token_hash.is_none() {
        return Err(StoreError::Forbidden);
    }
    if input.start_binding != grant_binding(cap)? {
        return Err(StoreError::Conflict);
    }
    if Uuid::parse_str(&input.request_id).is_err()
        || input.config_revision != cap.config_revision
        || input.expected_run_generation < 0
        || input.expected_run_generation == i64::MAX
        || input.expected_sequence < 0
        || input
            .checkpoint_before
            .as_ref()
            .is_some_and(|s| s.len() > 4096)
    {
        return Err(StoreError::InvalidInput);
    }
    let encoded = json(&(input, cap.auth_epoch, cap.credential_epoch))?;
    let digest = digest(&encoded).to_vec();
    let expected_generation = input.expected_run_generation + 1;
    let existing:Option<(String,String,Vec<u8>)>=conn.query_row("SELECT r.id,s.client_request_id,s.request_digest FROM source_sync_runs r JOIN source_run_starts s ON s.tenant_id=r.tenant_id AND s.connection_id=r.connection_id AND s.run_id=r.id WHERE r.tenant_id=?1 AND r.connection_id=?2 AND r.generation=?3",params![cap.tenant,cap.connection,expected_generation],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let id = if let Some((id, key, old_digest)) = existing {
        if key != input.request_id || old_digest != digest {
            return Err(StoreError::Conflict);
        }
        conn.execute("UPDATE source_sync_runs SET state='unknown',error='lease_expired',finished_at=?4 WHERE tenant_id=?1 AND connection_id=?2 AND id=?3 AND state='running' AND lease_until<=?4",params![cap.tenant,cap.connection,id,time])?;
        id
    } else {
        let (generation, checkpoint): (i64, Option<String>) = conn.query_row(
            "SELECT run_generation,checkpoint FROM source_connections WHERE tenant_id=?1 AND id=?2",
            params![cap.tenant, cap.connection],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if generation != input.expected_run_generation
            || c.local_sequence != input.expected_sequence
            || checkpoint != input.checkpoint_before
        {
            return Err(StoreError::Conflict);
        }
        let lease = begin_in(conn, cap, time)?;
        conn.execute(
            "INSERT INTO source_run_starts VALUES(?1,?2,?3,?4,?5)",
            params![
                cap.tenant,
                cap.connection,
                lease.id,
                input.request_id,
                digest
            ],
        )?;
        lease.id
    };
    let (generation,state)=conn.query_row("SELECT generation,state FROM source_sync_runs WHERE tenant_id=?1 AND connection_id=?2 AND id=?3",params![cap.tenant,cap.connection,id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    Ok(SourceRunStartReceipt {
        request_id: input.request_id.clone(),
        expected_run_generation: input.expected_run_generation,
        run_id: id,
        generation,
        state,
    })
}
impl Store {
    pub async fn begin_source_run_recoverable(
        &self,
        cap: &SourceIngestCapability,
        input: SourceRunStartIntent,
    ) -> Result<SourceRunStartReceipt> {
        let cap = cap.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let receipt = recover_in(&tx, &cap, &input, now()?)?;
            tx.commit()?;
            Ok(receipt)
        })
        .await
    }
}
#[cfg(test)]
mod tests;
