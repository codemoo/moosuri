//! The gateway requests a read for an administrator. Only the broker uses a key.
use super::*;
use moosuri_core::{ModelSource, ProviderQuota, QuotaMetric, QuotaScope, QuotaStatus};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub connection_revision: Uuid,
    pub revision: i64,
    pub probe_id: Option<Uuid>,
}
#[derive(Serialize)]
pub struct Reply {
    pub probe_id: Uuid,
    pub report: Option<ProviderQuota>,
}
// A server-generated UUID identifies one read. Never reuse an earlier completed read.
const LIFETIME: i64 = 30;
fn available(s: &Current, connection: Uuid) -> bool {
    s.enabled
        && s.connection == connection.to_string()
        && s.mode.as_deref() == Some("api")
        && s.catalog.kind == ProviderKind::OpenRouter
}

pub fn request(root: &Path, actor: &AdminSession, input: Request) -> Result<Reply> {
    let (_lock, mut db) = open(root, false)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    admin::check(&tx, actor)?;
    let s = current(&tx)?.ok_or(StoreError::NotFound)?;
    if !available(&s, input.connection_revision) || s.revision != input.revision {
        return Err(StoreError::Conflict);
    }
    let (owner, role, epoch) = actor.quota_owner();
    let cutoff = crate::now()? - LIFETIME;
    if let Some(id) = input.probe_id {
        let (state, observed_at, raw): (String, Option<i64>, Option<String>) = tx
            .query_row(
                "SELECT state,observed_at,metrics FROM engine_quota_requests WHERE account_id=?1
             AND role_revision=?2 AND session_epoch=?3 AND probe_id=?4 AND connection_revision=?5
             AND settings_revision=?6 AND requested_at>?7",
                params![
                    owner,
                    role,
                    epoch,
                    id.to_string(),
                    s.connection,
                    s.revision,
                    cutoff
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(StoreError::Conflict)?;
        let report = if state == "done" {
            let metrics: Option<Vec<QuotaMetric>> = raw.as_deref().map(parse).transpose()?;
            Some(ProviderQuota {
                source: ModelSource::Shared,
                provider_id: s.connection,
                provider_revision: s.revision,
                scope: QuotaScope::SharedProviderKey,
                status: if metrics.is_some() {
                    QuotaStatus::Known
                } else {
                    QuotaStatus::Unavailable
                },
                observed_at,
                metrics: metrics.unwrap_or_default(),
                evidence_url: "https://openrouter.ai/docs/api_reference/limits".into(),
                message: "OpenRouter shared API key capacity across models.".into(),
            })
        } else {
            None
        };
        return Ok(Reply {
            probe_id: id,
            report,
        });
    }
    tx.execute(
        "DELETE FROM engine_quota_requests WHERE requested_at<=?1",
        [cutoff],
    )?;
    let prior: Option<String> = tx
        .query_row(
            "SELECT probe_id FROM engine_quota_requests
        WHERE account_id=?1 AND role_revision=?2 AND session_epoch=?3 AND connection_revision=?4
        AND settings_revision=?5 AND state IN('pending','running')",
            params![owner, role, epoch, s.connection, s.revision],
            |r| r.get(0),
        )
        .optional()?;
    let id = if let Some(id) = prior {
        id.parse().map_err(|_| StoreError::Corrupt)?
    } else {
        let id = Uuid::new_v4();
        tx.execute("INSERT OR REPLACE INTO engine_quota_requests
            (account_id,role_revision,session_epoch,probe_id,connection_revision,settings_revision,requested_at,state)
            VALUES(?1,?2,?3,?4,?5,?6,unixepoch(),'pending')",
            params![owner,role,epoch,id.to_string(),s.connection,s.revision])?;
        id
    };
    tx.commit()?;
    Ok(Reply {
        probe_id: id,
        report: None,
    })
}

pub fn claim(root: &Path, connection: Uuid) -> Result<Option<Uuid>> {
    let (_lock, db) = open(root, false)?;
    let candidate: Option<(String, String, i64, i64, i64)> = db
        .query_row(
            "SELECT probe_id,account_id,role_revision,session_epoch,settings_revision
         FROM engine_quota_requests WHERE state='pending' AND connection_revision=?1
         AND requested_at>?2 ORDER BY requested_at,rowid LIMIT 1",
            params![connection.to_string(), crate::now()? - LIFETIME],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((id, owner, revision, epoch, settings)) = candidate else {
        return Ok(None);
    };
    // The registry directory lock covers all checks and this single state transition.
    let bound = current(&db)?.is_some_and(|s| available(&s, connection) && s.revision == settings);
    if !bound || !admin::quota_owner_current(&db, &owner, revision, epoch)? {
        db.execute("DELETE FROM engine_quota_requests WHERE probe_id=?1", [id])?;
        return Ok(None);
    }
    db.execute(
        "UPDATE engine_quota_requests SET state='running' WHERE probe_id=?1",
        [&id],
    )?;
    Ok(Some(id.parse().map_err(|_| StoreError::Corrupt)?))
}

pub fn finish(
    root: &Path,
    connection: Uuid,
    id: Uuid,
    metrics: Option<&[QuotaMetric]>,
) -> Result<()> {
    let (_lock, db) = open(root, false)?;
    let s = current(&db)?.ok_or(StoreError::Conflict)?;
    if !available(&s, connection) {
        return Err(StoreError::Conflict);
    }
    let metrics = metrics
        .map(|m| serde_json::to_string(m).map_err(|_| StoreError::InvalidInput))
        .transpose()?;
    let changed = db.execute(
        "UPDATE engine_quota_requests SET state='done',metrics=?1,
        observed_at=CASE WHEN ?1 IS NOT NULL THEN unixepoch() END WHERE state='running'
        AND probe_id=?2 AND connection_revision=?3 AND settings_revision=?4 AND requested_at>?5",
        params![
            metrics,
            id.to_string(),
            s.connection,
            s.revision,
            crate::now()? - LIFETIME
        ],
    )?;
    if changed != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
