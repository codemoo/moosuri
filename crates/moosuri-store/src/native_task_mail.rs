//! Native Gmail reads are actual supervised tool slots, never legacy display jobs.
//! The durable actor below is not a fabricated authenticated UserContext.
use super::*;
use crate::agent_execution::{KnowledgeToolInput, KnowledgeToolResult};
use moosuri_core::{MissionWaitView, ScopedConnectionDescriptor};
use moosuri_vault_protocol::broker::*;
pub(crate) const SLOT: &str = "native-mail-slot-v1";
pub(crate) fn completed_continuation(c: &Connection, tenant: &str, run: &str) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM native_task_mail_reads n JOIN agent_execution a ON a.tenant_id=n.tenant_id AND a.run_id=n.run_id WHERE n.tenant_id=?1 AND n.run_id=?2 AND n.step=a.remote_calls AND n.status='completed')",params![tenant,run],|r|r.get(0))?)
}
fn enc<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::Corrupt)
}
fn dec<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
fn millis() -> Result<i64> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_millis(),
    )
    .map_err(|_| StoreError::Unavailable)
}
fn publish(c: &Connection, t: &str, run: &str) -> Result<()> {
    let r = crate::chat::load_run(c, t, run)?;
    crate::chat::emit(
        c,
        &r,
        moosuri_core::ChatChange::State {
            run: crate::chat::view(c, &r)?,
        },
    )?;
    crate::run_instructions::publish_target_changes(c, t, run)
}
struct Grant {
    device: Device,
    binding: Binding,
}
impl Grant {
    fn choice(&self) -> String {
        format!("{}:{}", self.device.id, self.binding.id)
    }
}
fn grants(c: &Connection, t: &str, a: &str, epoch: i64, background: bool) -> Result<Vec<Grant>> {
    let rows=c.prepare("SELECT d.enrollment,b.binding,m.background FROM credential_broker_devices d JOIN credential_broker_bindings b ON b.device_id=d.id JOIN credential_model_mail_grants m USING(device_id,binding_id) JOIN credential_broker_repositories r USING(device_id,binding_id) WHERE d.tenant_id=?1 AND d.account_id=?2 AND d.status='active' AND b.active=1 AND r.repository=?3 LIMIT 64")?.query_map(params![t,a,enc(&Resource::google_model_read(Uuid::nil()))?],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,bool>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = vec![];
    for (pair, pin, bg) in rows {
        let pair: PairChallenge = dec(&pair)?;
        let binding: Binding = dec(&pin)?;
        if pair.device.owner.epoch == epoch && binding.expires_at > now()? && (!background || bg) {
            out.push(Grant {
                device: pair.device,
                binding,
            })
        }
    }
    Ok(out)
}
pub(crate) fn admit_profile(
    c: &Connection,
    u: &UserContext,
    p: &mut moosuri_core::RunToolProfile,
    background: bool,
) -> Result<()> {
    if !cfg!(feature = "credential-broker-chat") {
        return Ok(());
    }
    p.native_mail_model_read = true;
    p.native_mail_model_background = background;
    p.native_mail_scope = Some(moosuri_core::NativeMailRunScope {
        version: 1,
        operation: 8,
        candidate_binding_ids: grants(c, &u.tenant.0, &u.account.id, u.epoch, background)?
            .iter()
            .map(Grant::choice)
            .collect(),
    });
    Ok(())
}
fn background(c: &Connection, r: &crate::chat::Run) -> Result<bool> {
    Ok(c.query_row(
        "SELECT mode='durable' FROM missions WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
        |r| r.get(0),
    )
    .optional()?
    .unwrap_or(false))
}
fn authority(c: &Connection, t: &str, run: &str, acceptance: i64) -> Result<crate::chat::Run> {
    let r = crate::chat::load_run(c, t, run)?;
    let p = r.tool_profile.as_ref().ok_or(StoreError::Forbidden)?;
    if !cfg!(feature = "credential-broker-chat")
        || !p.native_mail_model_read
        || !p
            .native_mail_scope
            .as_ref()
            .is_some_and(|s| s.version == 1 && s.operation == 8)
    {
        return Err(StoreError::Forbidden);
    }
    let owner:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE tenant_id=?1 AND id=?2 AND auth_epoch=?3 AND status='active' AND recovery_required=0 AND must_change_password=0 AND (temporary_expires_at IS NULL OR temporary_expires_at>unixepoch()))",params![r.tenant,r.account,r.epoch],|r|r.get(0))?;
    if !owner {
        return Err(StoreError::Unauthorized);
    }
    let fence:bool=c.query_row("SELECT invalidated=0 AND deadline_ms>?3 AND (last_clock_ms IS NULL OR last_clock_ms<=?3) AND remote_calls<=6 AND tool_calls<=12 FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id,millis()?],|r|r.get(0)).optional()?.unwrap_or(false);
    if !fence {
        return Err(StoreError::BudgetExhausted);
    }
    let mission:Option<(String,bool,i64,i64,String)>=c.query_row("SELECT state,grant_enabled,expires_ms,coalesce((SELECT max(a.revision) FROM mission_acceptances a WHERE a.tenant_id=missions.tenant_id AND a.mission_id=missions.id),1),mode FROM missions WHERE tenant_id=?1 AND run_id=?2 AND auth_epoch=?3 AND uncertain=0",params![r.tenant,r.id,r.epoch],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    if let Some((state, enabled, expiry, revision, mode)) = mission {
        if !enabled
            || expiry <= millis()?
            || revision != acceptance
            || !matches!(
                state.as_str(),
                "queued" | "planning" | "executing" | "verifying" | "waiting"
            )
        {
            return Err(StoreError::Forbidden);
        }
        crate::recurring_missions::authority(c, &r)?;
        if mode == "durable" {
            if !p.native_mail_model_background {
                return Err(StoreError::Forbidden);
            }
        } else {
            crate::chat::authority(c, &r)?;
        }
    } else {
        crate::chat::authority(c, &r)?;
        if acceptance != 1 {
            return Err(StoreError::Forbidden);
        }
    }
    if !r.state.active() {
        return Err(StoreError::Conflict);
    }
    Ok(r)
}
fn selected(c: &Connection, r: &crate::chat::Run) -> Result<Option<Grant>> {
    let raw:Option<(String,String,String)>=c.query_row("SELECT device_id,binding_id,binding FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((device, id, raw)) = raw else {
        return Ok(None);
    };
    let pin: Binding = dec(&raw)?;
    Ok(
        grants(c, &r.tenant, &r.account, r.epoch, background(c, r)?)?
            .into_iter()
            .find(|g| {
                g.device.id.to_string() == device
                    && g.binding.id.to_string() == id
                    && g.binding == pin
            }),
    )
}
fn choices(c: &Connection, r: &crate::chat::Run) -> Result<Vec<Grant>> {
    let pinned: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?2)",
        params![r.tenant, r.id],
        |r| r.get(0),
    )?;
    if pinned {
        return Ok(selected(c, r)?.into_iter().collect());
    }
    let scope = &r
        .tool_profile
        .as_ref()
        .ok_or(StoreError::Forbidden)?
        .native_mail_scope
        .as_ref()
        .ok_or(StoreError::Forbidden)?
        .candidate_binding_ids;
    Ok(
        grants(c, &r.tenant, &r.account, r.epoch, background(c, r)?)?
            .into_iter()
            .filter(|g| scope.is_empty() || scope.contains(&g.choice()))
            .collect(),
    )
}
fn bind(c: &Connection, r: &crate::chat::Run, g: &Grant) -> Result<()> {
    let old: Option<String> = c
        .query_row(
            "SELECT binding FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?2",
            params![r.tenant, r.id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(old) = old {
        if old != enc(&g.binding)? || selected(c, r)?.is_none() {
            return Err(StoreError::Conflict);
        }
        return Ok(());
    }
    c.execute(
        "INSERT INTO native_task_mail_bindings VALUES(?1,?2,?3,?4,?5)",
        params![
            r.tenant,
            r.id,
            g.device.id.to_string(),
            g.binding.id.to_string(),
            enc(&g.binding)?
        ],
    )?;
    Ok(())
}
pub(crate) fn dependencies_current(c: &Connection, r: &crate::chat::Run) -> Result<bool> {
    let has: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?2)",
        params![r.tenant, r.id],
        |r| r.get(0),
    )?;
    if has && selected(c, r)?.is_none() {
        return Ok(false);
    }
    let origins=c.prepare("SELECT origin_run FROM native_mail_context_dependencies WHERE tenant_id=?1 AND run_id=?2")?.query_map(params![r.tenant,r.id],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for origin in origins {
        let source = crate::chat::load_run(c, &r.tenant, &origin)?;
        if selected(c, &source)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}
fn selector_current(
    c: &Connection,
    r: &crate::chat::Run,
    g: &Grant,
    read: &MailModelRead,
) -> Result<()> {
    let selector = match read {
        MailModelRead::Search { cursor, .. } => *cursor,
        MailModelRead::Message { selection }
        | MailModelRead::Thread { selection }
        | MailModelRead::Attachment { selection, .. } => Some(*selection),
    };
    let Some(_selector) = selector else {
        return Ok(());
    };
    let revision:i64=c.query_row("SELECT coalesce((SELECT max(a.revision) FROM mission_acceptances a JOIN missions m ON m.tenant_id=a.tenant_id AND m.id=a.mission_id WHERE m.tenant_id=?1 AND m.run_id=?2),1)",params![r.tenant,r.id],|row|row.get(0))?;
    let rows=c.prepare("SELECT x.receipt,n.payload FROM native_task_mail_reads n JOIN credential_broker_commitments x ON x.tenant_id=n.tenant_id AND x.run_id=n.run_id AND x.step=n.step AND x.call_id=n.broker_call AND x.invocation_nonce=n.invocation_nonce WHERE n.tenant_id=?1 AND n.run_id=?2 AND n.status='completed' AND n.expires_ms>?3 AND n.acceptance_revision=?4 AND x.receipt IS NOT NULL")?.query_map(params![r.tenant,r.id,millis()?,revision],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (raw, origin) in rows {
        let signed: SignedReceipt = dec(&raw)?;
        let req = &signed.receipt.commitment.request;
        if req.device != g.device
            || req.binding != g.binding
            || signed.receipt.outcome != Outcome::ProjectedSuccess
        {
            continue;
        }
        let Projection::GoogleMailModelRead { result } = signed.receipt.projection else {
            continue;
        };
        let origin: MailModelRequest = dec(&origin)?;
        let request = MailModelRequest {
            version: 1,
            background: background(c, r)?,
            read: read.clone(),
        };
        if request.accepts_selector(&origin, &result) {
            return Ok(());
        }
    }
    Err(StoreError::Forbidden)
}
/// Exact real invocation lookup. None means an unrelated legacy/settings invocation.
pub(crate) fn dispatch_current(c: &Connection, t: &str, i: &Invocation) -> Result<Option<bool>> {
    let row:Option<(i64,String,i64)>=c.query_row("SELECT acceptance_revision,status,expires_ms FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND broker_call=?4 AND invocation_nonce=?5",params![t,i.run.to_string(),i.step,i.call.to_string(),i.nonce.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((acceptance, status, expiry)) = row else {
        return Ok(None);
    };
    Ok(Some(
        expiry > millis()?
            && matches!(status.as_str(), "waiting" | "committed")
            && authority(c, t, &i.run.to_string(), acceptance).is_ok(),
    ))
}
pub(crate) fn payload(c: &Connection, p: &PairChallenge, r: &Request) -> Result<Option<Reply>> {
    let t = r.device.owner.tenant.to_string();
    let i = &r.invocation;
    let row:Option<(String,i64,String)>=c.query_row("SELECT payload,acceptance_revision,request FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND broker_call=?4 AND invocation_nonce=?5",params![t,i.run.to_string(),i.step,i.call.to_string(),i.nonce.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((raw, acceptance, request)) = row else {
        return Ok(None);
    };
    let actor = authority(c, &t, &i.run.to_string(), acceptance)?;
    let expected: Request = dec(&request)?;
    let g = selected(c, &actor)?.ok_or(StoreError::Forbidden)?;
    let body: MailModelRequest = dec(&raw)?;
    if p.device != r.device
        || g.device != r.device
        || g.binding != r.binding
        || expected != *r
        || r.expires_at <= now()?
        || dispatch_current(c, &t, i)? != Some(true)
        || !body.valid()
        || r.digest != moosuri_credential_broker::auth::hash(&body.bytes())
    {
        return Err(StoreError::Forbidden);
    }
    selector_current(c, &actor, &g, &body.read)?;
    Ok(Some(Reply::ModelMailPayload {
        payload: Box::new(body),
    }))
}
pub(crate) fn committed(c: &Connection, t: &str, i: &Invocation) -> Result<()> {
    if c.execute("UPDATE native_task_mail_reads SET status='committed' WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND broker_call=?4 AND invocation_nonce=?5 AND status='waiting'",params![t,i.run.to_string(),i.step,i.call.to_string(),i.nonce.to_string()])?==1 {
        c.execute("UPDATE chat_runs SET version=version+1 WHERE tenant_id=?1 AND id=?2",params![t,i.run.to_string()])?;publish(c,t,&i.run.to_string())?;
    }
    Ok(())
}
fn prepare(c: &Connection, r: &crate::chat::Run, id: &str, g: &Grant) -> Result<()> {
    let(raw,step,call,nonce,expiry):(String,u32,String,String,i64)=c.query_row("SELECT payload,step,broker_call,invocation_nonce,expires_ms FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3",params![r.tenant,r.id,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let body: MailModelRequest = dec(&raw)?;
    selector_current(c, r, g, &body.read)?;
    let request = Request {
        device: g.device.clone(),
        invocation: Invocation {
            run: r.id.parse().map_err(|_| StoreError::Corrupt)?,
            step,
            call: call.parse().map_err(|_| StoreError::Corrupt)?,
            nonce: nonce.parse().map_err(|_| StoreError::Corrupt)?,
        },
        binding: g.binding.clone(),
        operation: Operation::GoogleMailModelRead {
            selection: id.parse().map_err(|_| StoreError::Corrupt)?,
        },
        repository: None,
        digest: moosuri_credential_broker::auth::hash(&body.bytes()),
        expires_at: (expiry / 1000).min(g.binding.expires_at),
    };
    let encoded = enc(&request)?;
    c.execute(
        "INSERT OR IGNORE INTO credential_broker_invocations VALUES(?1,?2,?3,?4,?5,?6,0)",
        params![r.tenant, r.id, step, call, nonce, encoded],
    )?;
    c.execute("UPDATE native_task_mail_reads SET request=?4,expires_ms=min(expires_ms,?5) WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3 AND request IS NULL",params![r.tenant,r.id,id,encoded,request.expires_at*1000])?;
    Ok(())
}
pub(crate) fn poll_device(
    c: &Connection,
    p: &PairChallenge,
    status: &str,
) -> Result<Option<Reply>> {
    if status != "active" {
        return Ok(None);
    }
    settle(c, millis()?)?;
    crate::connection_waits::poll(c, millis()?)?;
    let rows=c.prepare("SELECT n.request,n.acceptance_revision FROM native_task_mail_reads n JOIN native_task_mail_bindings b USING(tenant_id,run_id) WHERE b.device_id=?1 AND n.status='waiting' AND n.request IS NOT NULL AND NOT EXISTS(SELECT 1 FROM credential_broker_commitments x WHERE x.tenant_id=n.tenant_id AND x.run_id=n.run_id AND x.step=n.step AND x.call_id=n.broker_call AND x.invocation_nonce=n.invocation_nonce) LIMIT 32")?.query_map([p.device.id.to_string()],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut requests = vec![];
    for (raw, _acceptance) in rows {
        let r: Request = dec(&raw)?;
        if payload(c, p, &r)?.is_some() {
            requests.push(r)
        }
    }
    if requests.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Reply::Waiting {
            requests,
            revocation_pending: false,
        }))
    }
}
pub(crate) fn view(c: &Connection, t: &str, run: &str) -> Result<Option<MissionWaitView>> {
    let state: String = c.query_row(
        "SELECT state FROM chat_runs WHERE tenant_id=?1 AND id=?2",
        params![t, run],
        |row| row.get(0),
    )?;
    let active = matches!(
        state.as_str(),
        "queued" | "running" | "outcome_unknown" | "incomplete"
    );
    if !active {
        return Ok(None);
    }
    let row:Option<(String,i64,i64,String,i64)>=c.query_row("SELECT request_id,step,acceptance_revision,status,expires_ms FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND status IN('waiting','committed','ready','unknown','expired') ORDER BY step DESC LIMIT 1",params![t,run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let Some((request_id, step, acceptance_revision, status, expires_at_ms)) = row else {
        return Ok(None);
    };
    let r = crate::chat::load_run(c, t, run)?;
    let pinned: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?2)",
        params![t, run],
        |r| r.get(0),
    )?;
    let mut out = vec![];
    if status == "waiting" && !pinned {
        for g in choices(c, &r)? {
            let alias=c.query_row("SELECT alias FROM connection_aliases WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3",params![t,r.account,g.binding.id.to_string()],|r|r.get(0)).optional()?;
            out.push(ScopedConnectionDescriptor {
                binding_id: g.choice(),
                device_id: g.device.id.to_string(),
                operation: "google.mail.model_read.v1".into(),
                resource: "mailbox".into(),
                schema_version: 1,
                state: "ready".into(),
                expires_at_ms: expires_at_ms.min(g.binding.expires_at * 1000),
                alias,
            })
        }
    }
    Ok(Some(MissionWaitView {
        request_id,
        slot_id: SLOT.into(),
        run_id: run.into(),
        step,
        acceptance_revision,
        cause: if pinned {
            "vault_unlock"
        } else {
            "account_selection"
        }
        .into(),
        state: status,
        expires_at_ms,
        choices: out,
    }))
}
/// Promotion is called in B's shared admission-order loop, after its owner-slot guard.
pub(crate) fn promote(c: &Connection, t: &str, run: &str, id: &str) -> Result<()> {
    let guard:Option<(i64,i64)>=c.query_row("SELECT acceptance_revision,expires_ms FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3 AND status='ready'",params![t,run,id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let Some((acceptance, expiry)) = guard else {
        return Ok(());
    };
    let actor = authority(c, t, run, acceptance);
    let current = match actor {
        Ok(r) => dependencies_current(c, &r)?,
        Err(_) => false,
    };
    if expiry <= millis()? || !current {
        settle(c, millis()?)?;
        return Ok(());
    }
    let pending:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND status IN('waiting','committed'))",params![t,run],|row|row.get(0))?;
    if pending {
        return Ok(());
    }
    c.execute("UPDATE native_task_mail_reads SET status='completed' WHERE tenant_id=?1 AND run_id=?2 AND status='ready'",params![t,run])?;
    c.execute(
        "UPDATE missions SET state='queued' WHERE tenant_id=?1 AND run_id=?2 AND state='waiting'",
        params![t, run],
    )?;
    c.execute("UPDATE chat_runs SET state='queued',attempt=min(attempt+1,32),version=version+1 WHERE tenant_id=?1 AND id=?2",params![t,run])?;
    publish(c, t, run)?;
    Ok(())
}
pub(crate) fn revoke_device(c: &Connection, device: &str) -> Result<()> {
    let rows = c
        .prepare("SELECT tenant_id,run_id FROM native_task_mail_bindings WHERE device_id=?1")?
        .query_map([device], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, run) in rows {
        crate::agent_execution::purge_runs(c, &t, vec![run.clone()])?;
        c.execute(
            "DELETE FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?2",
            params![t, run],
        )?;
    }
    Ok(())
}
/// Each finite pending row is revisited in at most ceil(N/64) writer sweeps,
/// including after restart. Positions are progress metadata, never task identity.
fn sweep(c: &Connection, dependencies: bool) -> Result<Vec<i64>> {
    let (table, column, filter) = if dependencies {
        ("native_task_mail_bindings", "dependency_rowid", "1=1")
    } else {
        (
            "native_task_mail_reads",
            "read_rowid",
            "status IN('waiting','committed','ready')",
        )
    };
    let cursor: i64 = c.query_row(
        &format!("SELECT {column} FROM native_mail_settlement_cursor WHERE singleton=1"),
        [],
        |row| row.get(0),
    )?;
    let mut ids = c
        .prepare(&format!(
            "SELECT rowid FROM {table} WHERE {filter} AND rowid>?1 ORDER BY rowid LIMIT ?2"
        ))?
        .query_map(params![cursor, 64], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if ids.len() < 64 {
        let remaining = 64 - ids.len();
        ids.extend(
            c.prepare(&format!(
                "SELECT rowid FROM {table} WHERE {filter} AND rowid<=?1 ORDER BY rowid LIMIT ?2"
            ))?
            .query_map(params![cursor, remaining as i64], |row| {
                row.get::<_, i64>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        );
    }
    let next = ids.last().copied().unwrap_or(0);
    c.execute(
        &format!("UPDATE native_mail_settlement_cursor SET {column}=?1 WHERE singleton=1"),
        [next],
    )?;
    Ok(ids)
}
/// Called by the normal DB loops. Signed late receipts are retained without re-running inference.
pub(crate) fn settle(c: &Connection, time: i64) -> Result<u32> {
    let ids = sweep(c, true)?;
    let mut deps = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(row) = c
            .query_row(
                "SELECT tenant_id,run_id FROM native_task_mail_bindings WHERE rowid=?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?
        {
            deps.push(row)
        }
    }
    for (t, run) in deps {
        let r = crate::chat::load_run(c, &t, &run)?;
        if !dependencies_current(c, &r)? {
            let unknown:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND status='committed')",params![t,run],|row|row.get(0))?;
            crate::agent_execution::purge_runs(c, &t, vec![run.clone()])?;
            if unknown {
                c.execute("UPDATE chat_runs SET state='outcome_unknown',error='interrupted',version=version+1 WHERE tenant_id=?1 AND id=?2",params![t,run])?;
                c.execute("UPDATE missions SET state='unknown',uncertain=1 WHERE tenant_id=?1 AND run_id=?2 AND state!='deleted'",params![t,run])?;
            }
            c.execute(
                "DELETE FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?2",
                params![t, run],
            )?;
        }
    }
    let ids = sweep(c, false)?;
    let mut rows = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(row)=c.query_row("SELECT tenant_id,run_id,step,call_id,request_id,status,expires_ms,acceptance_revision,broker_call,invocation_nonce FROM native_task_mail_reads WHERE rowid=?1 AND status IN('waiting','committed','ready')",[id],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,i64>(2)?,row.get::<_,String>(3)?,row.get::<_,String>(4)?,row.get::<_,String>(5)?,row.get::<_,i64>(6)?,row.get::<_,i64>(7)?,row.get::<_,String>(8)?,row.get::<_,String>(9)?))).optional()? {rows.push(row)}
    }
    let mut count = 0;
    let mut dirty = std::collections::BTreeSet::new();
    for (t, run, step, call, id, status, expiry, acceptance, broker_call, nonce) in rows {
        let actor = authority(c, &t, &run, acceptance);
        if expiry <= time || actor.is_err() {
            let unknown = status == "committed";
            c.execute("UPDATE native_task_mail_reads SET status=?4 WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3",params![t,run,id,if unknown{"unknown"}else{"expired"}])?;
            c.execute("UPDATE missions SET state=?3,grant_enabled=0,uncertain=max(uncertain,?4),report='[\"Native mail wait expired or authority changed. No automatic replay.\"]' WHERE tenant_id=?1 AND run_id=?2 AND state='waiting'",params![t,run,if unknown{"unknown"}else{"partial"},unknown])?;
            c.execute("UPDATE chat_runs SET state=?3,error='budget_exhausted',version=version+1 WHERE tenant_id=?1 AND id=?2 AND state='queued'",params![t,run,if unknown{"outcome_unknown"}else{"incomplete"}])?;
            dirty.insert((t.clone(), run.clone()));
            count += 1;
            continue;
        }
        let actor = actor?;
        if status == "ready" {
            continue;
        }
        let options = choices(c, &actor)?;
        if selected(c, &actor)?.is_none() && options.len() == 1 {
            bind(c, &actor, &options[0])?;
            dirty.insert((t.clone(), run.clone()));
        }
        let Some(g) = selected(c, &actor)? else {
            continue;
        };
        let exists:bool=c.query_row("SELECT request IS NOT NULL FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3",params![t,run,id],|r|r.get(0))?;
        if !exists {
            match prepare(c, &actor, &id, &g) {
                Ok(()) => {
                    dirty.insert((t.clone(), run.clone()));
                }
                Err(StoreError::Forbidden | StoreError::InvalidInput) => {
                    let result = KnowledgeToolResult::Error(crate::ToolReadError::Unavailable);
                    c.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND result IS NULL",params![t,run,step,call,enc(&result)?])?;
                    c.execute("UPDATE native_task_mail_reads SET status=?4 WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3",params![t,run,id,"ready"])?;
                    dirty.insert((t.clone(), run.clone()));
                    count += 1;
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        let signed:Option<String>=c.query_row("SELECT receipt FROM credential_broker_commitments WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND invocation_nonce=?5",params![t,run,step,broker_call,nonce],|r|r.get(0)).optional()?.flatten();
        let Some(signed) = signed else { continue };
        let signed: SignedReceipt = dec(&signed)?;
        let receipt = &signed.receipt;
        let expected:String=c.query_row("SELECT request FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3",params![t,run,id],|r|r.get(0))?;
        if receipt.commitment.request != dec::<Request>(&expected)?
            || receipt.commitment.request.device != g.device
            || receipt.commitment.request.binding != g.binding
        {
            return Err(StoreError::Corrupt);
        }
        if receipt.outcome == Outcome::Unknown {
            c.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND result IS NULL",params![t,run,step,call,enc(&KnowledgeToolResult::Error(crate::ToolReadError::OutcomeUnknown))?])?;
            c.execute("UPDATE native_task_mail_reads SET status='unknown' WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3",params![t,run,id])?;
            c.execute("UPDATE missions SET state='unknown',uncertain=1,grant_enabled=0 WHERE tenant_id=?1 AND run_id=?2 AND state='waiting'",params![t,run])?;
            c.execute("UPDATE chat_runs SET state='outcome_unknown',error='interrupted',version=version+1 WHERE tenant_id=?1 AND id=?2",params![t,run])?;
            dirty.insert((t.clone(), run.clone()));
            count += 1;
            continue;
        }
        let result = match (&receipt.outcome, &receipt.projection) {
            (Outcome::ProjectedSuccess, Projection::GoogleMailModelRead { result })
                if result.valid() =>
            {
                KnowledgeToolResult::External(
                    serde_json::json!({"native_mail":result,"native_mail_provenance":{"tenant":t,"account":actor.account,"device_id":g.device.id,"binding_id":g.binding.id,"request_id":id,"run_id":run,"step":step,"call_id":call,"projection_digest":receipt.projection_digest}}),
                )
            }
            _ => KnowledgeToolResult::Error(crate::ToolReadError::Unavailable),
        };
        c.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND result IS NULL",params![t,run,step,call,enc(&result)?])?;
        c.execute("UPDATE native_task_mail_reads SET status=?4 WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3",params![t,run,id,"ready"])?;
        dirty.insert((t.clone(), run.clone()));
        count += 1;
    }
    let chats=c.prepare("SELECT DISTINCT n.tenant_id,n.run_id FROM native_task_mail_reads n JOIN chat_runs r ON r.tenant_id=n.tenant_id AND r.id=n.run_id WHERE n.status='ready' AND r.state='queued' AND NOT EXISTS(SELECT 1 FROM native_task_mail_reads f WHERE f.tenant_id=n.tenant_id AND f.run_id=n.run_id AND f.status IN('unknown','expired','cancelled')) AND NOT EXISTS(SELECT 1 FROM missions m WHERE m.tenant_id=n.tenant_id AND m.run_id=n.run_id) AND NOT EXISTS(SELECT 1 FROM native_task_mail_reads p WHERE p.tenant_id=n.tenant_id AND p.run_id=n.run_id AND p.status IN('waiting','committed')) ORDER BY n.rowid LIMIT 64")?.query_map([],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, run) in chats {
        c.execute("UPDATE native_task_mail_reads SET status='completed' WHERE tenant_id=?1 AND run_id=?2 AND status='ready'",params![t,run])?;
        c.execute("UPDATE chat_runs SET state='queued',attempt=min(attempt+1,32),version=version+1 WHERE tenant_id=?1 AND id=?2",params![t,run])?;
        dirty.insert((t, run));
    }
    for (t, run) in &dirty {
        c.execute(
            "UPDATE chat_runs SET version=version+1 WHERE tenant_id=?1 AND id=?2",
            params![t, run],
        )?;
        publish(c, t, run)?;
    }
    Ok(count.max(dirty.len() as u32))
}
impl Store {
    pub async fn wait_for_native_mail_tool(
        &self,
        lease: &crate::StepLease,
        call: String,
    ) -> Result<bool> {
        let l = lease.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let r=match crate::agent_execution::guard_step(&tx,&db.provider_key,&l){Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
            let(raw,result):(String,Option<String>)=tx.query_row("SELECT input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![r.tenant,r.id,l.step,call],|r|Ok((r.get(0)?,r.get(1)?)))?;if result.is_some(){return Ok(false)}let KnowledgeToolInput::NativeMail{read}=dec(&raw)? else{return Err(StoreError::InvalidInput)};
            let acceptance:i64=tx.query_row("SELECT coalesce((SELECT max(a.revision) FROM mission_acceptances a JOIN missions m ON m.tenant_id=a.tenant_id AND m.id=a.mission_id WHERE m.tenant_id=?1 AND m.run_id=?2),1)",params![r.tenant,r.id],|r|r.get(0))?;authority(&tx,&r.tenant,&r.id,acceptance)?;
            if !moosuri_core::mission_read_domains(&r.text).0{return Err(StoreError::Forbidden)}
            if crate::mailbox_intent::requested(&r.text)?.is_some() {return Err(StoreError::Forbidden)}
            let bg=background(&tx,&r)?;let body=MailModelRequest{version:1,background:bg,read};if !body.valid(){return Err(StoreError::InvalidInput)}
            let unstable:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state IN('dispatched','unknown')) OR EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND state='started')",params![r.tenant,r.id],|r|r.get(0))?;if unstable{return Err(StoreError::Conflict)}
            if crate::missions::is_mission(&tx,&r.tenant,&r.id)? && !crate::missions::capacity::waiting_available(&tx,&r.tenant,&r.account)? {return Err(StoreError::Busy)}
            let old:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4)",params![r.tenant,r.id,l.step,call],|r|r.get(0))?;
            if !old {
                // Every pending native call in the round needs budget before yielding;
                // otherwise the worker would strand a later resultless tool slot.
                let pending:i64=tx.query_row("SELECT count(*) FROM agent_tool_slots s WHERE s.tenant_id=?1 AND s.run_id=?2 AND s.step=?3 AND s.result IS NULL AND json_extract(s.input,'$.tool')='native_mail' AND NOT EXISTS(SELECT 1 FROM native_task_mail_reads n WHERE n.tenant_id=s.tenant_id AND n.run_id=s.run_id AND n.step=s.step AND n.call_id=s.call_id)",params![r.tenant,r.id,l.step],|row|row.get(0))?;
                let used:i64=tx.query_row("SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|row|row.get(0))?;
                if used+pending>12{return Err(StoreError::BudgetExhausted)}
                let expiry:i64=tx.query_row("SELECT min(a.deadline_ms,coalesce(m.expires_ms,a.deadline_ms)) FROM agent_execution a LEFT JOIN missions m ON m.tenant_id=a.tenant_id AND m.run_id=a.run_id WHERE a.tenant_id=?1 AND a.run_id=?2",params![r.tenant,r.id],|r|r.get(0))?;
                if tx.execute("UPDATE agent_execution SET tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2 AND tool_calls<12",params![r.tenant,r.id])?!=1{return Err(StoreError::BudgetExhausted)}
                tx.execute("INSERT INTO native_task_mail_reads VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'waiting',?9,?10,?11,NULL)",params![r.tenant,r.id,l.step,call,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),enc(&body)?,expiry,acceptance,r.attempt])?;
            }
            let pending=tx.prepare("SELECT call_id,input FROM agent_tool_slots s WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND result IS NULL AND json_extract(input,'$.tool')='native_mail' AND NOT EXISTS(SELECT 1 FROM native_task_mail_reads n WHERE n.tenant_id=s.tenant_id AND n.run_id=s.run_id AND n.step=s.step AND n.call_id=s.call_id)")?.query_map(params![r.tenant,r.id,l.step],|row|Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            for(other,raw)in pending {
                let KnowledgeToolInput::NativeMail{read}=dec(&raw)? else{return Err(StoreError::Corrupt)};
                let payload=MailModelRequest{version:1,background:bg,read};let expiry:i64=tx.query_row("SELECT min(a.deadline_ms,coalesce(m.expires_ms,a.deadline_ms)) FROM agent_execution a LEFT JOIN missions m ON m.tenant_id=a.tenant_id AND m.run_id=a.run_id WHERE a.tenant_id=?1 AND a.run_id=?2",params![r.tenant,r.id],|row|row.get(0))?;
                tx.execute("UPDATE agent_execution SET tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2 AND tool_calls<12",params![r.tenant,r.id])?;
                tx.execute("INSERT INTO native_task_mail_reads VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'waiting',?9,?10,?11,NULL)",params![r.tenant,r.id,l.step,other,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),enc(&payload)?,expiry,acceptance,r.attempt])?;
            }
            tx.execute("UPDATE missions SET state='waiting' WHERE tenant_id=?1 AND run_id=?2 AND state IN('planning','executing','verifying')",params![r.tenant,r.id])?;
            tx.execute("UPDATE chat_runs SET state='queued',version=version+1 WHERE tenant_id=?1 AND id=?2",params![r.tenant,r.id])?;tx.execute("DELETE FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id])?;
            settle(&tx,millis()?)?;publish(&tx,&r.tenant,&r.id)?;tx.commit()?;Ok(true)
        }).await
    }
    pub async fn settle_native_task_mail(&self) -> Result<u32> {
        self.call(|db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let n = settle(&tx, millis()?)?;
            crate::connection_waits::poll(&tx, millis()?)?;
            tx.commit()?;
            Ok(n)
        })
        .await
    }
    pub async fn native_mail_continuation(&self, lease: &crate::RunLease) -> Result<bool> {
        let l = lease.clone();
        self.call(move |db| {
            let r = crate::chat::guard(&db.conn, &db.provider_key, &l)?;
            completed_continuation(&db.conn, &r.tenant, &r.id)
        })
        .await
    }
    pub async fn select_native_mail_connection(
        &self,
        user: &UserContext,
        target: String,
        input: moosuri_core::MissionConnectionSelection,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&user,true,false)?;Uuid::parse_str(&input.request_id).map_err(|_|StoreError::InvalidInput)?;
            let run:String=tx.query_row("SELECT id FROM chat_runs WHERE tenant_id=?1 AND account_id=?2 AND (id=?3 OR id=(SELECT run_id FROM missions WHERE tenant_id=?1 AND id=?3 AND account_id=?2))",params![u.tenant.0,u.account.id,target],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound)?;
            let payload=enc(&input)?;let old:Option<String>=tx.query_row("SELECT payload FROM native_task_mail_selections WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3",params![u.tenant.0,run,input.request_id],|r|r.get(0)).optional()?;if let Some(old)=old {return if old==payload{Ok(())}else{Err(StoreError::Conflict)}}
            let w=view(&tx,&u.tenant.0,&run)?.ok_or(StoreError::Conflict)?;let revision:i64=tx.query_row("SELECT coalesce((SELECT revision FROM missions WHERE tenant_id=?1 AND run_id=?2),version) FROM chat_runs WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,run],|r|r.get(0))?;
            if input.slot_id!=SLOT||w.request_id!=input.wait_request_id||w.state!="waiting"||revision!=input.expected_revision{return Err(StoreError::Conflict)}let pinned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?2)",params![u.tenant.0,run],|row|row.get(0))?;if pinned{return Err(StoreError::Conflict)}let r=authority(&tx,&u.tenant.0,&run,w.acceptance_revision)?;let options=choices(&tx,&r)?;let matches:Vec<_>=options.iter().filter(|g|g.choice()==input.binding_id).collect();if matches.len()!=1{return Err(StoreError::Forbidden)}bind(&tx,&r,matches[0])?;
            tx.execute("INSERT INTO native_task_mail_selections VALUES(?1,?2,?3,?4)",params![u.tenant.0,run,input.request_id,payload])?;settle(&tx,millis()?)?;publish(&tx,&u.tenant.0,&run)?;tx.commit()?;Ok(())
        }).await
    }
}
impl Store {
    /// Reissues only a completed current model step after its native handoff.
    pub async fn native_mail_step_lease(
        &self,
        lease: &crate::RunLease,
        step: i64,
    ) -> Result<crate::StepLease> {
        let l = lease.clone();
        self.call(move|db|{let r=crate::chat::guard(&db.conn,&db.provider_key,&l)?;
            let valid:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM native_task_mail_reads n JOIN agent_execution a USING(tenant_id,run_id) WHERE n.tenant_id=?1 AND n.run_id=?2 AND n.step=?3 AND n.step=a.remote_calls AND n.status='completed')",params![r.tenant,r.id,step],|r|r.get(0))?;if !valid{return Err(StoreError::Forbidden)}
            db.conn.execute("UPDATE agent_steps SET attempt=?4 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND state='completed'",params![r.tenant,r.id,step,r.attempt])?;
            let nonce:Vec<u8>=db.conn.query_row("SELECT nonce FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND state='completed'",params![r.tenant,r.id,step],|r|r.get(0))?;Ok(crate::StepLease{run:l,step,nonce:nonce.try_into().map_err(|_|StoreError::Corrupt)?})
        }).await
    }
}
#[cfg(all(test, feature = "credential-broker-chat"))]
mod tests;
pub(crate) fn quarantine(c: &Connection) -> Result<()> {
    let rows=c.prepare("SELECT tenant_id,run_id FROM native_task_mail_reads UNION SELECT tenant_id,run_id FROM native_task_mail_bindings")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, run) in rows {
        crate::agent_execution::purge_runs(c, &t, vec![run])?;
    }
    c.execute_batch("UPDATE credential_broker_invocations SET cancelled=1 WHERE json_type(request,'$.operation.google_mail_model_read') IS NOT NULL OR json_extract(request,'$.operation.kind')='google_mail_model_read';DELETE FROM native_task_mail_selections;DELETE FROM native_task_mail_bindings;DELETE FROM credential_model_mail_payloads;DELETE FROM credential_model_mail_grants;")?;
    Ok(())
}
/// A's requested-report verifier can consume this exact signed source address.
/// Search/display results alone do not qualify as a report body read.
pub(crate) fn report_receipt(
    c: &Connection,
    r: &crate::chat::Run,
    step: i64,
    call: &str,
) -> Result<bool> {
    if !dependencies_current(c, r)? {
        return Ok(false);
    }
    let row:Option<(String,String,String)>=c.query_row("SELECT n.payload,s.result,x.receipt FROM native_task_mail_reads n JOIN agent_tool_slots s ON s.tenant_id=n.tenant_id AND s.run_id=n.run_id AND s.step=n.step AND s.call_id=n.call_id JOIN credential_broker_commitments x ON x.tenant_id=n.tenant_id AND x.run_id=n.run_id AND x.step=n.step AND x.call_id=n.broker_call AND x.invocation_nonce=n.invocation_nonce WHERE n.tenant_id=?1 AND n.run_id=?2 AND n.step=?3 AND n.call_id=?4 AND n.status='completed' AND s.result IS NOT NULL AND x.receipt IS NOT NULL",params![r.tenant,r.id,step,call],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?))).optional()?;
    let Some((payload, result, signed)) = row else {
        return Ok(false);
    };
    let payload: MailModelRequest = dec(&payload)?;
    if matches!(payload.read, MailModelRead::Search { .. }) {
        return Ok(false);
    }
    let signed: SignedReceipt = dec(&signed)?;
    let Some(g) = selected(c, r)? else {
        return Ok(false);
    };
    let receipt = &signed.receipt;
    if receipt.outcome != Outcome::ProjectedSuccess
        || receipt.commitment.request.device != g.device
        || receipt.commitment.request.binding != g.binding
    {
        return Ok(false);
    }
    let Projection::GoogleMailModelRead { result: source } = &receipt.projection else {
        return Ok(false);
    };
    if source.status != MailModelStatus::Ready
        || !source.valid()
        || source.messages.is_empty() && source.attachment.is_none()
    {
        return Ok(false);
    }
    let KnowledgeToolResult::External(saved) = dec(&result)? else {
        return Ok(false);
    };
    Ok(saved.get("native_mail")
        == Some(&serde_json::to_value(source).map_err(|_| StoreError::Corrupt)?)
        && saved["native_mail_provenance"]["run_id"] == r.id
        && saved["native_mail_provenance"]["step"] == step
        && saved["native_mail_provenance"]["call_id"] == call)
}
pub(crate) fn has(c: &Connection, t: &str, run: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2)",
        params![t, run],
        |r| r.get(0),
    )?)
}
pub(crate) fn cancel(c: &Connection, r: &crate::chat::Run) -> Result<bool> {
    let committed:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2 AND status='committed')",params![r.tenant,r.id],|row|row.get(0))?;
    c.execute("UPDATE native_task_mail_reads SET status=CASE WHEN status='committed' THEN 'unknown' ELSE 'cancelled' END WHERE tenant_id=?1 AND run_id=?2 AND status IN('waiting','ready','committed')",params![r.tenant,r.id])?;
    c.execute("UPDATE credential_broker_invocations SET cancelled=1 WHERE tenant_id=?1 AND run_id=?2 AND NOT EXISTS(SELECT 1 FROM credential_broker_commitments x WHERE x.tenant_id=credential_broker_invocations.tenant_id AND x.run_id=credential_broker_invocations.run_id AND x.step=credential_broker_invocations.step AND x.call_id=credential_broker_invocations.call_id AND x.invocation_nonce=credential_broker_invocations.invocation_nonce)",params![r.tenant,r.id])?;
    Ok(committed)
}
pub(crate) fn inherit(c: &Connection, t: &str, run: &str, prior: &str) -> Result<()> {
    c.execute("INSERT OR IGNORE INTO native_mail_context_dependencies SELECT tenant_id,?2,run_id FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?3",params![t,run,prior])?;
    c.execute("INSERT OR IGNORE INTO native_mail_context_dependencies SELECT tenant_id,?2,origin_run FROM native_mail_context_dependencies WHERE tenant_id=?1 AND run_id=?3",params![t,run,prior])?;
    Ok(())
}
pub(crate) fn expand(c: &Connection, t: &str, ids: Vec<String>) -> Result<Vec<String>> {
    let mut out = std::collections::BTreeSet::from_iter(ids);
    let roots = out.clone();
    for root in roots {
        out.extend(c.prepare("SELECT run_id FROM native_mail_context_dependencies WHERE tenant_id=?1 AND origin_run=?2")?.query_map(params![t,root],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    Ok(out.into_iter().collect())
}
pub(crate) fn delete_thread(c: &Connection, t: &str, thread: &str) -> Result<()> {
    let roots=c.prepare("SELECT b.run_id FROM native_task_mail_bindings b JOIN chat_runs r ON r.tenant_id=b.tenant_id AND r.id=b.run_id WHERE b.tenant_id=?1 AND r.thread_id=?2")?.query_map(params![t,thread],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if !roots.is_empty() {
        crate::agent_execution::purge_runs(c, t, roots)?;
    }
    Ok(())
}
