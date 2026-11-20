//! A private browser job uses one ordinary chat turn across observation, human control and completion.
use super::*;
use crate::{chat, personal_text_preview::ModelPin};
use moosuri_core::{
    ChatChange, MessagePhase, ModelChoice, ProviderKind, RunState, personal_browser::Workspace,
};
use personal_volume::VolumeOwner;

mod tasks;

#[derive(Clone)]
pub struct Scope {
    pub owner: VolumeOwner,
    pub user: UserContext,
    pub thread: String,
}
impl Scope {
    fn authorize(&self, c: &Connection) -> Result<()> {
        authorize_member(c, self.owner, &self.user)?;
        chat::thread_exists(c, &self.user, &self.thread)
    }
}
fn authorize_member(c: &Connection, owner: VolumeOwner, user: &UserContext) -> Result<()> {
    owner.check_identity(c, None)?;
    let u = validate(c, user, true, false)?;
    if u.tenant.0 != owner.tenant.to_string()
        || u.account.id != owner.account.to_string()
        || u.helper
    {
        return Err(StoreError::Forbidden);
    }
    Ok(())
}
pub struct Job {
    pub view: Workspace,
    pub original: String,
    pub guidance: Vec<String>,
    pub human_touched: bool,
    pub remaining_reads: i64,
    pending: Option<String>,
}
fn expire_workers(c: &Connection, owner: VolumeOwner) -> Result<()> {
    // Do not renew sessions merely because the background cleanup is running.
    c.execute("UPDATE personal_browser_workers SET stopping=1 WHERE tenant_id=?1 AND volume_id=?2 AND generation=?3 AND stopping=0 AND (expires_at<=unixepoch() OR NOT EXISTS(SELECT 1 FROM sessions s JOIN accounts a ON a.id=s.account_id AND a.tenant_id=s.tenant_id WHERE s.digest=session_digest AND a.id=?4 AND a.status='active' AND a.auth_epoch=s.auth_epoch AND a.must_change_password=0 AND a.recovery_required=0 AND s.expires_at>unixepoch() AND (s.absolute_deadline IS NULL OR s.absolute_deadline>unixepoch())))", params![owner.tenant.to_string(), owner.volume.to_string(), owner.generation as i64, owner.account.to_string()])?;
    let revoked = c.prepare("SELECT b.run_id,b.view FROM personal_browser_jobs b JOIN personal_browser_workers w ON w.tenant_id=b.tenant_id AND w.run_id=b.run_id WHERE w.volume_id=?1 AND w.generation=?2 AND w.stopping=1 AND json_extract(b.view,'$.state') NOT IN('closed','outcome_unknown')")?.query_map(params![owner.volume.to_string(), owner.generation as i64], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, encoded) in revoked {
        let mut view: Workspace =
            serde_json::from_str(&encoded).map_err(|_| StoreError::Corrupt)?;
        view.state = "closed".into();
        view.reason = Some("browser_authority_expired".into());
        view.revision += 1;
        view.tabs.clear();
        view.links.clear();
        view.active_tab_id = None;
        let run = chat::load_run(c, &owner.tenant.to_string(), &id)?;
        c.execute("UPDATE personal_browser_jobs SET view=?1,pending=NULL,controller_session=NULL,controller_digest=NULL,controller_until=NULL WHERE tenant_id=?2 AND run_id=?3", params![serde_json::to_string(&view).map_err(|_| StoreError::Corrupt)?, run.tenant, run.id])?;
        if run.state.active() {
            chat::transition(
                c,
                &run,
                RunState::Cancelled,
                Some(chat::RunFailure::AuthorityRevoked),
            )?;
        }
    }
    // Release control when either the task list or the original chat observes
    // expiry. Keep the expired deadline so a stale relay view cannot renew it.
    c.execute(
        "UPDATE personal_browser_jobs
         SET view = json_set(view, '$.state', 'ready', '$.reason', 'controller_expired',
                             '$.revision', json_extract(view, '$.revision') + 1)
         WHERE tenant_id=?1 AND volume_id=?2 AND generation=?3 AND pending IS NULL
           AND controller_until<=unixepoch() AND json_extract(view,'$.state')='user_control'",
        params![owner.tenant.to_string(), owner.volume.to_string(), owner.generation as i64],
    )?;
    Ok(())
}
fn current(c: &Connection, scope: &Scope) -> Result<Option<Job>> {
    scope.authorize(c)?;
    expire_workers(c, scope.owner)?;
    let row = c.query_row("SELECT b.view,r.user_text,b.human_touched,b.remaining_reads,r.auth_epoch,b.pending FROM personal_browser_jobs b JOIN chat_runs r ON r.tenant_id=b.tenant_id AND r.id=b.run_id WHERE b.tenant_id=?1 AND b.thread_id=?2 AND r.account_id=?3 AND b.volume_id=?4 AND b.generation=?5 ORDER BY r.sequence DESC LIMIT 1", params![scope.user.tenant.0, scope.thread, scope.user.account.id, scope.owner.volume.to_string(), scope.owner.generation as i64], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)?,r.get::<_,Option<String>>(5)?))).optional()?;
    let Some((encoded, original, human_touched, remaining_reads, epoch, pending)) = row else {
        return Ok(None);
    };
    if scope.user.epoch != epoch {
        return Ok(None);
    }
    let view: Workspace = serde_json::from_str(&encoded).map_err(|_| StoreError::Corrupt)?;
    let guidance = c.prepare("SELECT text FROM run_instructions WHERE tenant_id=?1 AND run_id=?2 AND state!='closed' ORDER BY sequence")?.query_map(params![scope.user.tenant.0, view.run_id], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(Some(Job {
        view,
        original,
        guidance,
        human_touched,
        remaining_reads,
        pending,
    }))
}
fn save(c: &Connection, scope: &Scope, view: &Workspace) -> Result<()> {
    let bytes = serde_json::to_string(view).map_err(|_| StoreError::Corrupt)?;
    c.execute(
        "UPDATE personal_browser_jobs SET view=?3 WHERE tenant_id=?1 AND run_id=?2",
        params![scope.user.tenant.0, view.run_id, bytes],
    )?;
    Ok(())
}
pub(crate) fn has(c: &Connection, tenant: &str, run: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM personal_browser_jobs WHERE tenant_id=?1 AND run_id=?2)",
        params![tenant, run],
        |r| r.get(0),
    )?)
}
pub struct Version {
    pub workspace_id: String,
    pub revision: i64,
    pub request_id: Uuid,
}
pub enum Admission {
    Fresh(Job),
    Existing(Job),
}
pub enum Output {
    Observation(String),
    Generated {
        text: String,
        model: ModelPin,
        applied: usize,
    },
}

fn observation_model() -> ModelChoice {
    ModelChoice {
        source: Default::default(),
        provider_name: "Browser observation".into(),
        provider_id: "personal-browser".into(),
        provider_revision: 1,
        kind: ProviderKind::Compatible,
        model: "browser-read".into(),
        effort: None,
        verified_at: 0,
        function_tools: false,
    }
}

impl Store {
    pub async fn personal_browser_current(&self, scope: Scope) -> Result<Option<Job>> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let job = current(&tx, &scope)?;
            tx.commit()?;
            Ok(job)
        })
        .await
    }
    pub async fn personal_browser_open(
        &self,
        scope: Scope,
        request: Uuid,
        url: String,
        text: String,
        wait: bool,
    ) -> Result<Admission> {
        if text.trim().is_empty() || text.len() > 8192 || url.len() > 2048 || request.is_nil() {
            return Err(StoreError::InvalidInput);
        }
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let previous = current(&tx, &scope)?;
            let u = &scope.user;
            let digest = Sha256::digest(serde_json::to_vec(&("browser-v1", &url, &text, wait)).map_err(|_| StoreError::Corrupt)?);
            if let Some(job) = previous {
                if job.view.request_id == request.to_string() {
                    let same: bool = tx.query_row("SELECT payload_digest=?3 FROM chat_runs WHERE tenant_id=?1 AND id=?2", params![u.tenant.0, job.view.run_id, digest.as_slice()], |r| r.get(0))?;
                    return if same {
                        Ok(Admission::Existing(job))
                    } else {
                        Err(StoreError::Conflict)
                    };
                }
                if job.view.state == "completed" {
                    // Opening the next explicitly requested task retires the previous preview and its profile.
                    tx.execute("UPDATE personal_browser_workers SET stopping=1 WHERE workspace_id=?1", [&job.view.id])?;
                    expire_workers(&tx, scope.owner)?;
                } else if !matches!(job.view.state.as_str(), "closed" | "outcome_unknown") {
                    return Err(StoreError::Conflict);
                }
            }
            let workers: i64 = tx.query_row("SELECT count(*) FROM personal_browser_workers WHERE volume_id=?1 AND generation=?2", params![scope.owner.volume.to_string(), scope.owner.generation as i64], |r| r.get(0))?;
            if workers >= 4 {
                return Err(StoreError::Busy);
            }
            let run_id = Uuid::new_v4().to_string();
            let model = observation_model();
            let view = Workspace {
                id: Uuid::new_v4().to_string(),
                run_id: run_id.clone(),
                thread_id: scope.thread.clone(),
                request_id: request.to_string(),
                state: "starting".into(),
                revision: 1,
                url,
                title: String::new(),
                tabs: vec![],
                active_tab_id: None,
                links: vec![],
                reason: None,
            };
            let sequence: i64 = tx.query_row("UPDATE threads SET chat_turn_seq=chat_turn_seq+1,updated_at=unixepoch() WHERE tenant_id=?1 AND id=?2 RETURNING chat_turn_seq", params![u.tenant.0, scope.thread], |r| r.get(0))?;
            let order = chat::next_order(&tx)?;
            tx.execute("INSERT INTO chat_runs(tenant_id,id,account_id,thread_id,request_id,payload_digest,sequence,user_text,model,session_digest,auth_epoch,attempt,state,version,created_at,admitted_order) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,1,'running',1,unixepoch(),?12)", params![u.tenant.0, run_id, u.account.id, scope.thread, request.to_string(), digest.as_slice(), sequence, text, serde_json::to_string(&model).map_err(|_| StoreError::Corrupt)?, u.digest.as_slice(), u.epoch, order])?;
            tx.execute("INSERT INTO personal_browser_jobs(tenant_id,run_id,thread_id,volume_id,generation,view,pending,remaining_reads) VALUES(?1,?2,?3,?4,?5,?6,?7,5)", params![u.tenant.0, run_id, scope.thread, scope.owner.volume.to_string(), scope.owner.generation as i64, serde_json::to_string(&view).map_err(|_| StoreError::Corrupt)?, request.to_string()])?;
            tx.execute("INSERT INTO personal_browser_workers(tenant_id,volume_id,generation,workspace_id,run_id,session_digest,expires_at) VALUES(?1,?2,?3,?4,?5,?6,unixepoch()+28800)", params![u.tenant.0, scope.owner.volume.to_string(), scope.owner.generation as i64, view.id, run_id, u.digest.as_slice()])?;
            let run = chat::load_run(&tx, &u.tenant.0, &run_id)?;
            chat::emit(
                &tx,
                &run,
                ChatChange::Turn {
                    turn: chat::turn(&tx, &run)?,
                },
            )?;
            tx.commit()?;
            Ok(Admission::Fresh(Job {
                view,
                original: text,
                guidance: vec![],
                human_touched: false,
                remaining_reads: 5,pending:Some(request.to_string()),
            }))
        })
        .await
    }
    pub async fn personal_browser_guide(
        &self,
        scope: Scope,
        id: String,
        request: Uuid,
        text: String,
    ) -> Result<Job> {
        if text.trim().is_empty() || text.len() > 2000 || request.is_nil() {
            return Err(StoreError::InvalidInput);
        }
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let job = current(&tx, &scope)?.ok_or(StoreError::NotFound)?;
            if job.view.id != id {
                return Err(StoreError::Conflict);
            }
            let tenant = &scope.user.tenant.0;
            let run = chat::load_run(&tx, tenant, &job.view.run_id)?;
            if let Some(old) = crate::run_instructions::receipts(&tx, &run)?.into_iter().find(|r| r.request_id == request.to_string()) {
                return if old.text == text {
                    Ok(job)
                } else {
                    Err(StoreError::Conflict)
                };
            }
            if !matches!(job.view.state.as_str(), "starting" | "reading" | "ready" | "user_control") || !run.state.active() || job.guidance.len() >= 8 {
                return Err(StoreError::Conflict);
            }
            tx.execute("INSERT INTO run_instructions(tenant_id,run_id,request_id,attempt,execution_attempt,sequence,text,state,session_digest,auth_epoch) VALUES(?1,?2,?3,1,1,?4,?5,'received',?6,?7)", params![tenant, run.id, request.to_string(), job.guidance.len() as i64 + 1, text, scope.user.digest.as_slice(), scope.user.epoch])?;
            let updated = current(&tx, &scope)?.ok_or(StoreError::NotFound)?;
            tx.commit()?;
            Ok(updated)
        })
        .await
    }
    pub async fn personal_browser_reserve(
        &self,
        scope: Scope,
        version: Version,
        op: &'static str,
        token: Option<String>,
    ) -> Result<Admission> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut job = current(&tx, &scope)?.ok_or(StoreError::NotFound)?;
            if job.view.id != version.workspace_id || version.request_id.is_nil() {
                return Err(StoreError::Conflict);
            }
            let tenant = &scope.user.tenant.0;
            let run = &job.view.run_id;
            let id = version.request_id.to_string();
            let previous: Option<String> = tx.query_row("SELECT operation FROM personal_browser_actions WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3", params![tenant, run, id], |r| r.get(0)).optional()?;
            if let Some(old) = previous {
                return if old == op && matches!(op, "resume" | "close") {
                    Ok(Admission::Existing(job))
                } else {
                    Err(StoreError::Conflict)
                };
            }
            if job.view.revision != version.revision {
                return Err(StoreError::Conflict);
            }
            let (pending, count): (Option<String>, i64) = tx.query_row("SELECT pending,(SELECT count(*) FROM personal_browser_actions a WHERE a.tenant_id=b.tenant_id AND a.run_id=b.run_id) FROM personal_browser_jobs b WHERE tenant_id=?1 AND run_id=?2", params![tenant, run], |r| Ok((r.get(0)?, r.get(1)?)))?;
            let interruptible = op == "close" || (job.view.state == "reading" && op == "takeover");
            if pending.is_some() && !interruptible || count >= 4096 && op != "close" {
                return Err(StoreError::Conflict);
            }
            let valid = match op {
                "takeover" => matches!(job.view.state.as_str(), "reading" | "ready" | "completed" | "user_control"),
                "input" | "resume" | "switch" | "close_tab" => job.view.state == "user_control",
                "follow" => !job.human_touched && matches!(job.view.state.as_str(), "ready" | "completed"),
                "close" => !matches!(job.view.state.as_str(), "closed" | "stopping"),
                _ => false,
            };
            if !valid {
                return Err(StoreError::Conflict);
            }
            if matches!(op, "input" | "resume" | "switch" | "close_tab") {
                let secret = token.as_deref().ok_or(StoreError::Forbidden)?;
                let ok: bool = tx.query_row("SELECT coalesce(controller_digest=?3 AND controller_session=?4 AND controller_until>unixepoch(),0) FROM personal_browser_jobs WHERE tenant_id=?1 AND run_id=?2", params![tenant, run, Sha256::digest(secret.as_bytes()).as_slice(), scope.user.digest.as_slice()], |r| r.get(0))?;
                if !ok {
                    return Err(StoreError::Forbidden);
                }
            }
            if matches!(op, "resume" | "follow") {
                if job.remaining_reads <= 0 {
                    return Err(StoreError::BudgetExhausted);
                }
                tx.execute("UPDATE personal_browser_jobs SET remaining_reads=remaining_reads-1 WHERE tenant_id=?1 AND run_id=?2", params![tenant, run])?;
                job.remaining_reads -= 1;
                job.view.state = "resuming".into();
            }
            if op == "close" {
                job.view.state = "stopping".into();
            }
            if op == "takeover" {
                tx.execute("UPDATE personal_browser_workers SET session_digest=?2 WHERE workspace_id=?1 AND stopping=0", params![job.view.id, scope.user.digest.as_slice()])?;
                let new_token = token.as_deref().ok_or(StoreError::InvalidInput)?;
                tx.execute("UPDATE personal_browser_jobs SET human_touched=1,controller_digest=?3,controller_session=?4,controller_until=unixepoch()+600 WHERE tenant_id=?1 AND run_id=?2", params![tenant, run, Sha256::digest(new_token.as_bytes()).as_slice(), scope.user.digest.as_slice()])?;
                job.view.state = "user_control".into();
                job.view.links.clear();
                job.human_touched = true;
                let loaded = chat::load_run(&tx, tenant, run)?;
                if !loaded.state.active() {
                    chat::transition(&tx, &loaded, RunState::Running, None)?;
                }
            }
            tx.execute("INSERT INTO personal_browser_actions VALUES(?1,?2,?3,?4)", params![tenant, run, id, op])?;
            tx.execute("UPDATE personal_browser_jobs SET pending=?3 WHERE tenant_id=?1 AND run_id=?2", params![tenant, run, (op != "takeover").then_some(id)])?;
            job.view.revision += 1;
            save(&tx, &scope, &job.view)?;
            tx.commit()?;
            Ok(Admission::Fresh(job))
        })
        .await
    }
    /// The broker positively rejected the operation before dispatch. Undo only this receipt,
    /// restoring the interrupted reservation as well as its frame revision and read budget.
    pub async fn personal_browser_busy(
        &self,
        scope: Scope,
        prior: Job,
        revision: i64,
        request: Uuid,
    ) -> Result<()> {
        self.call(move|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current=current(&tx,&scope)?.ok_or(StoreError::NotFound)?;
            if current.view.id!=prior.view.id||current.view.revision!=revision||current.pending!=Some(request.to_string()){
                return Err(StoreError::Conflict);
            }
            let id=&prior.view.run_id;
            tx.execute("UPDATE personal_browser_jobs SET pending=?3,remaining_reads=?4 WHERE tenant_id=?1 AND run_id=?2",params![scope.user.tenant.0,id,prior.pending,prior.remaining_reads])?;
            tx.execute("DELETE FROM personal_browser_actions WHERE tenant_id=?1 AND run_id=?2 AND request_id=?3",params![scope.user.tenant.0,id,request.to_string()])?;
            save(&tx,&scope,&prior.view)?;
            tx.commit()?;Ok(())
        }).await
    }
    /// An observation and the corresponding chat answer commit together, conditioned on the same dispatch.
    pub async fn personal_browser_finish(
        &self,
        scope: Scope,
        view: Workspace,
        request: Uuid,
        output: Option<Output>,
    ) -> Result<Workspace> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current = current(&tx, &scope)?.ok_or(StoreError::NotFound)?;
            if current.view.id != view.id || current.view.revision != view.revision {
                return Err(StoreError::Conflict);
            }
            let tenant = &scope.user.tenant.0;
            if tx.execute("UPDATE personal_browser_jobs SET pending=NULL WHERE tenant_id=?1 AND run_id=?2 AND pending=?3", params![tenant, view.run_id, request.to_string()])? != 1 {
                return Err(StoreError::Conflict);
            }
            let mut run = chat::load_run(&tx, tenant, &view.run_id)?;
            let mut view = view;
            let (text, applied) = match output {
                Some(Output::Observation(text)) => {
                    run.model = observation_model();
                    (Some(text), 0)
                }
                Some(Output::Generated { text, model, applied }) => {
                    run.model = ModelChoice {
                        source: Default::default(),
                        provider_name: "Deployment browser answer".into(),
                        provider_id: model.connection_revision.to_string(),
                        provider_revision: 1,
                        kind: model.kind,
                        model: model.model,
                        effort: model.effort,
                        verified_at: now()?,
                        function_tools: false,
                    };
                    (Some(text), applied)
                }
                None => (None, 0),
            };
            // Instructions arriving after the model request need another explicit read.
            let complete = view.state == "completed" && current.guidance.len() == applied;
            if view.state == "completed" && !complete {
                view.state = "ready".into();
                view.reason = Some("new_guidance".into());
            }
            if let Some(answer) = text {
                let model = serde_json::to_string(&run.model).map_err(|_| StoreError::Corrupt)?;
                tx.execute("UPDATE chat_runs SET model=?1 WHERE tenant_id=?2 AND id=?3", params![model, tenant, run.id])?;
                let item = if complete {
                    "browser-answer"
                } else {
                    "browser-observation"
                };
                // The selected page replaces the prior draft/answer of this same job.
                tx.execute("DELETE FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item IN('browser-answer','browser-observation')", params![tenant, run.id])?;
                let mut rest = answer.as_str();
                while !rest.is_empty() {
                    let mut end = rest.len().min(8192);
                    while !rest.is_char_boundary(end) {
                        end -= 1;
                    }
                    chat::apply_patches(
                        &tx,
                        &run,
                        vec![chat::OutputPatch::Append {
                            item: item.into(),
                            text: rest[..end].into(),
                            phase: Some(if complete {
                                MessagePhase::FinalAnswer
                            } else {
                                MessagePhase::Commentary
                            }),
                            refusal: false,
                        }],
                    )?;
                    rest = &rest[end..];
                }
                tx.execute("UPDATE run_instructions SET state='applied' WHERE tenant_id=?1 AND run_id=?2 AND sequence<=?3", params![tenant, run.id, applied as i64])?;
            }
            if view.state == "closed" {
                tx.execute("DELETE FROM personal_browser_workers WHERE workspace_id=?1", [&view.id])?;
            } else if view.state == "outcome_unknown" {
                tx.execute("UPDATE personal_browser_workers SET stopping=1 WHERE workspace_id=?1", [&view.id])?;
            }
            let next = match view.state.as_str() {
                "completed" => Some((RunState::Completed, None)),
                "outcome_unknown" => Some((RunState::OutcomeUnknown, Some(chat::RunFailure::ConnectionFailed))),
                "closed" if run.state.active() => Some((RunState::Cancelled, None)),
                _ => None,
            };
            if let Some((state, error)) = next {
                chat::transition(&tx, &run, state, error)?;
            }
            if view.state != "user_control" {
                tx.execute("UPDATE personal_browser_jobs SET controller_digest=NULL,controller_session=NULL,controller_until=NULL WHERE tenant_id=?1 AND run_id=?2", params![tenant, run.id])?;
            }
            view.revision += 1;
            save(&tx, &scope, &view)?;
            tx.commit()?;
            Ok(view)
        })
        .await
    }
}

impl Store {
    /// This lease authorizes only destruction of known, no-longer-authorized workspace IDs.
    pub async fn personal_browser_retired(&self, owner: VolumeOwner) -> Result<Vec<String>> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            owner.check_identity(&tx, None)?;
            expire_workers(&tx, owner)?;
            let ids = tx.prepare("SELECT workspace_id FROM personal_browser_workers WHERE tenant_id=?1 AND volume_id=?2 AND generation=?3 AND stopping=1 ORDER BY expires_at LIMIT 4")?.query_map(params![owner.tenant.to_string(), owner.volume.to_string(), owner.generation as i64], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            tx.commit()?;
            Ok(ids)
        })
        .await
    }
    pub async fn personal_browser_retired_ack(&self, owner: VolumeOwner, id: String) -> Result<()> {
        self.call(move |db| {
            owner.check_identity(&db.conn, None)?;
            db.conn.execute("DELETE FROM personal_browser_workers WHERE tenant_id=?1 AND volume_id=?2 AND generation=?3 AND workspace_id=?4 AND stopping=1", params![owner.tenant.to_string(), owner.volume.to_string(), owner.generation as i64, id])?;
            Ok(())
        })
        .await
    }
    /// Commit the exact public link and its read budget before asking the broker to follow it.
    pub async fn personal_browser_follow_observed(
        &self,
        scope: Scope,
        request: Uuid,
        revision: i64,
        url: String,
        guidance_count: usize,
    ) -> Result<Job> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut j = current(&tx, &scope)?.ok_or(StoreError::NotFound)?;
            if j.view.revision != revision || j.view.state != "reading" || j.human_touched || j.guidance.len() != guidance_count || !j.view.links.iter().any(|l| l.url == url) || j.view.tabs.iter().any(|t| t.url == url) {
                return Err(StoreError::Conflict);
            }
            if tx.execute("UPDATE personal_browser_jobs SET remaining_reads=remaining_reads-1 WHERE tenant_id=?1 AND run_id=?2 AND pending=?3 AND remaining_reads>0 AND human_touched=0", params![scope.user.tenant.0, j.view.run_id, request.to_string()])? != 1 {
                return Err(StoreError::Conflict);
            }
            j.remaining_reads -= 1;
            j.view.state = "resuming".into();
            j.view.revision += 1;
            save(&tx, &scope, &j.view)?;
            tx.commit()?;
            Ok(j)
        })
        .await
    }
    /// The first valid frame can be streamed while the pinned model prepares its answer.
    pub async fn personal_browser_publish(
        &self,
        scope: Scope,
        request: Uuid,
        mut view: Workspace,
    ) -> Result<Job> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let prior = current(&tx, &scope)?.ok_or(StoreError::NotFound)?;
            if prior.view.id != view.id || prior.view.revision != view.revision {
                return Err(StoreError::Conflict);
            }
            let pending: Option<String> = tx.query_row(
                "SELECT pending FROM personal_browser_jobs WHERE tenant_id=?1 AND run_id=?2",
                params![scope.user.tenant.0, view.run_id],
                |r| r.get(0),
            )?;
            if pending != Some(request.to_string()) {
                return Err(StoreError::Conflict);
            }
            view.state = "reading".into();
            view.revision += 1;
            save(&tx, &scope, &view)?;
            let job = current(&tx, &scope)?.ok_or(StoreError::NotFound)?;
            tx.commit()?;
            Ok(job)
        })
        .await
    }
    pub async fn personal_browser_refresh(
        &self,
        scope: Scope,
        expected: i64,
        mut view: Workspace,
    ) -> Result<Workspace> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let job = current(&tx, &scope)?.ok_or(StoreError::NotFound)?;
            if job.view.id != view.id || job.view.revision != expected {
                return Ok(job.view);
            }
            let (pending, expired): (Option<String>, bool) = tx.query_row("SELECT pending,coalesce(controller_until<=unixepoch(),0) FROM personal_browser_jobs WHERE tenant_id=?1 AND run_id=?2", params![scope.user.tenant.0, view.run_id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if pending.is_some() {
                return Ok(job.view);
            }
            if expired && view.state == "user_control" {
                view.state = "ready".into();
                view.reason = Some("controller_expired".into());
            }
            if view.state == "outcome_unknown" && job.view.state != view.state {
                tx.execute("UPDATE personal_browser_workers SET stopping=1 WHERE workspace_id=?1", [&view.id])?;
                let run = chat::load_run(&tx, &scope.user.tenant.0, &view.run_id)?;
                if run.state.active() {
                    chat::transition(&tx, &run, RunState::OutcomeUnknown, Some(chat::RunFailure::ConnectionFailed))?;
                }
            }
            if view.state != job.view.state || view.active_tab_id != job.view.active_tab_id || view.tabs != job.view.tabs {
                view.revision += 1;
                save(&tx, &scope, &view)?;
            }
            tx.commit()?;
            Ok(view)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn personal_browser_worker_cap_expiry_and_move_to_next_job() {
        let _guard = crate::tests::TEST_LOCK.lock().await;
        let dir = crate::tests::directory();
        let root = dir.path().canonicalize().unwrap();
        let owner = VolumeOwner {
            account: Uuid::new_v4(),
            tenant: Uuid::new_v4(),
            volume: Uuid::new_v4(),
            generation: 1,
        };
        owner
            .initialize(
                &root,
                "Member".into(),
                "browser@example.test".into(),
                "Temporary-browser-password-42".into(),
            )
            .await
            .unwrap();
        let store = owner.open_verified(&root).await.unwrap();
        let login = store
            .login(
                "browser@example.test".into(),
                "Temporary-browser-password-42".into(),
            )
            .await
            .unwrap();
        let user = store
            .change_password(&login.user, "Changed-browser-password-42".into())
            .await
            .unwrap()
            .user;
        let mut jobs = Vec::new();
        for i in 0..5 {
            let thread = store
                .create_thread(&user, format!("Work {i}"))
                .await
                .unwrap()
                .id;
            let scope = Scope {
                owner,
                user: user.clone(),
                thread,
            };
            let request = Uuid::new_v4();
            let result = store
                .personal_browser_open(
                    scope.clone(),
                    request,
                    "https://example.org/".into(),
                    "Read it".into(),
                    false,
                )
                .await;
            if i == 4 {
                assert!(matches!(result, Err(StoreError::Busy)));
                break;
            }
            let Admission::Fresh(job) = result.unwrap() else {
                panic!("expected fresh")
            };
            jobs.push((scope, request, job.view));
        }
        let (expired, _, view) = jobs.remove(0);
        let id = view.id.clone();
        store
            .call(move |db| {
                db.conn.execute(
                    "UPDATE personal_browser_workers SET expires_at=0 WHERE workspace_id=?1",
                    [id],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(
            store
                .personal_browser_current(expired.clone())
                .await
                .unwrap()
                .unwrap()
                .view
                .state,
            "closed"
        );
        assert_eq!(
            store.personal_browser_retired(owner).await.unwrap(),
            [view.id.clone()]
        );
        store
            .personal_browser_retired_ack(owner, view.id)
            .await
            .unwrap();
        let (scope, request, mut completed) = jobs.remove(0);
        completed.state = "completed".into();
        store
            .personal_browser_finish(
                scope.clone(),
                completed.clone(),
                request,
                Some(Output::Observation("Synthetic page evidence".into())),
            )
            .await
            .unwrap();
        let new = store
            .personal_browser_open(
                scope.clone(),
                Uuid::new_v4(),
                "https://example.org/page2".into(),
                "Read another page".into(),
                false,
            )
            .await
            .unwrap();
        assert!(matches!(new, Admission::Fresh(_)));
        assert_eq!(
            store.personal_browser_retired(owner).await.unwrap(),
            [completed.id]
        );
        assert_eq!(
            store
                .conversation(&user, scope.thread, None)
                .await
                .unwrap()
                .turns
                .len(),
            2
        );
    }
}
