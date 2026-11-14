//! Explicit scheduled read-only missions; no session capability is minted by polling.
use super::*;
use crate::agent_execution::KnowledgeToolResult;
use moosuri_core::*;
#[cfg(test)]
mod tests;
fn encode<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
pub(crate) fn is_recurring(c: &Connection, t: &str, id: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM recurring_missions WHERE tenant_id=?1 AND schedule_id=?2)",
        params![t, id],
        |r| r.get(0),
    )?)
}
pub(crate) fn authority(c: &Connection, r: &crate::chat::Run) -> Result<()> {
    let row:Option<bool>=c.query_row("SELECT s.state='active' AND s.revision=o.schedule_revision AND s.auth_epoch=?3 AND (s.valid_until IS NULL OR s.valid_until>?4) FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id JOIN reminder_schedules s ON s.tenant_id=o.tenant_id AND s.id=o.schedule_id WHERE m.tenant_id=?1 AND m.run_id=?2",params![r.tenant,r.id,r.epoch,now()?],|r|r.get(0)).optional()?;
    if row == Some(false) {
        return Err(StoreError::Unauthorized);
    }
    Ok(())
}
fn view(c: &Connection, u: &UserContext, id: &str) -> Result<RecurringMissionView> {
    let schedule = crate::schedules::recurring_view(c, u, id)?;
    let(template,model,changes):(String,String,bool)=c.query_row("SELECT template,model,changes_only FROM recurring_missions WHERE tenant_id=?1 AND schedule_id=?2 AND account_id=?3",params![u.tenant.0,id,u.account.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    let recent=c.prepare("SELECT o.occurrence_id,o.mission_id,m.state,o.notification_id,o.publication_state='exact_unchanged',o.scheduled_for FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id WHERE o.tenant_id=?1 AND o.schedule_id=?2 ORDER BY o.scheduled_for DESC,o.occurrence_id DESC LIMIT 20")?.query_map(params![u.tenant.0,id],|r|Ok(RecurringMissionOccurrence{occurrence_id:r.get(0)?,mission_id:r.get(1)?,state:r.get(2)?,notification_id:r.get(3)?,exact_unchanged:r.get(4)?,scheduled_for:r.get(5)?}))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let occurrences=c.prepare("SELECT id,revision,scheduled_for,local_slot,state,reason,notification_id,delayed,coalesce((SELECT source_suppressed FROM local_notifications n WHERE n.tenant_id=reminder_occurrences.tenant_id AND n.id=reminder_occurrences.notification_id),0) FROM reminder_occurrences WHERE tenant_id=?1 AND schedule_id=?2 ORDER BY created_at DESC,id DESC LIMIT 50")?.query_map(params![u.tenant.0,id],|r|Ok(ScheduleOccurrence{id:r.get(0)?,revision:r.get(1)?,scheduled_for:r.get(2)?,local_slot:r.get(3)?,state:r.get(4)?,reason:r.get(5)?,notification_id:r.get(6)?,delayed:r.get(7)?,notification_suppressed:r.get(8)?}))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let skipped_ranges=c.prepare("SELECT from_slot,to_slot,count,reason,revision FROM reminder_skipped_ranges WHERE tenant_id=?1 AND schedule_id=?2 ORDER BY created_at DESC,id DESC LIMIT 50")?.query_map(params![u.tenant.0,id],|r|Ok(ScheduleSkippedRange{from_slot:r.get(0)?,to_slot:r.get(1)?,count:r.get(2)?,reason:r.get(3)?,revision:r.get(4)?}))?.collect::<std::result::Result<Vec<_>,_>>()?;
    Ok(RecurringMissionView {
        schedule,
        template: decode(&template)?,
        model: decode(&model)?,
        changes_only: changes,
        recent,
        history: ScheduleHistory {
            occurrences,
            skipped_ranges,
        },
    })
}
// Fence only unfinished children. Committed historical results and accounting stay historical.
fn fence_children(c: &Connection, t: &str, id: &str) -> Result<()> {
    let runs=c.prepare("SELECT m.run_id FROM missions m JOIN recurring_mission_occurrences o ON o.tenant_id=m.tenant_id AND o.mission_id=m.id WHERE o.tenant_id=?1 AND o.schedule_id=?2 AND m.state IN('queued','planning','executing','verifying','paused')")?.query_map(params![t,id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for run in runs {
        let dispatched:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state IN('dispatched','unknown')) OR EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND state='started')",params![t,run],|r|r.get(0))?;
        c.execute("UPDATE missions SET grant_enabled=0,state=?3,uncertain=max(uncertain,?4),candidate=NULL,round_text=NULL,plan='[]',revision=revision+1,report='[\"Recurring authority changed; this occurrence will not resume.\"]' WHERE tenant_id=?1 AND run_id=?2",params![t,run,if dispatched{"unknown"}else{"cancelled"},dispatched])?;
        c.execute(
            "UPDATE chat_runs SET state=?3 WHERE tenant_id=?1 AND id=?2",
            params![
                t,
                run,
                if dispatched {
                    "outcome_unknown"
                } else {
                    "cancelled"
                }
            ],
        )?;
        c.execute(
            "UPDATE agent_execution SET invalidated=1 WHERE tenant_id=?1 AND run_id=?2",
            params![t, run],
        )?;
        c.execute("UPDATE recurring_mission_occurrences SET publication_state='fenced' WHERE tenant_id=?1 AND mission_id=(SELECT id FROM missions WHERE tenant_id=?1 AND run_id=?2) AND publication_state='pending'",params![t,run])?;
    }
    Ok(())
}
impl Store {
    pub async fn create_recurring_mission(
        &self,
        user: &UserContext,
        input: RecurringMissionCreate,
    ) -> Result<RecurringMissionView> {
        let user = user.clone();
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u=validate(&tx,&user,true,false)?;
            let digest=Sha256::digest(encode(&input)?.as_bytes()).to_vec();
            if let Some((id,old))=tx.query_row("SELECT schedule_id,payload_digest FROM recurring_missions WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![u.tenant.0,u.account.id,input.request_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?))).optional()?{if old!=digest{return Err(StoreError::Conflict)}return view(&tx,&u,&id)}
            let choice=providers::selected_choice(&tx,&u)?;let profile=crate::web::admitted_profile(&tx,&u,&db.web_profile)?;
            let out=create_owner_in(&tx,&db.provider_key,&u,input,choice,profile)?;tx.commit()?;Ok(out)
        }).await
    }
    pub async fn recurring_missions(
        &self,
        user: &UserContext,
    ) -> Result<Vec<RecurringMissionView>> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;let ids=db.conn.prepare("SELECT x.schedule_id FROM recurring_missions x JOIN reminder_schedules s ON s.tenant_id=x.tenant_id AND s.id=x.schedule_id WHERE x.tenant_id=?1 AND x.account_id=?2 AND s.state!='revoked' ORDER BY s.created_at DESC,x.schedule_id LIMIT 64")?.query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;ids.iter().map(|id|view(&db.conn,&u,id)).collect()}).await
    }
    pub async fn control_recurring_mission(
        &self,
        user: &UserContext,
        id: String,
        input: ScheduleControl,
    ) -> Result<RecurringMissionView> {
        let user = user.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &user, true, false)?;
            let out = control_owner_in(&tx, &db.provider_key, &u, &id, input)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
    pub async fn edit_recurring_mission(
        &self,
        user: &UserContext,
        id: String,
        input: ScheduleEdit,
    ) -> Result<RecurringMissionView> {
        let user = user.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &user, true, false)?;
            let out = edit_owner_in(&tx, &db.provider_key, &u, &id, input)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
    /// Trusted runtime only. DB polling makes no model/network calls.
    pub async fn poll_recurring_missions(&self) -> Result<u32> {
        self.poll_recurring_missions_at(now()?).await
    }
    #[doc(hidden)]
    pub async fn poll_recurring_missions_at(&self, time: i64) -> Result<u32> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let n = poll(&tx, &db.provider_key, time)?;
            tx.commit()?;
            Ok(n)
        })
        .await
    }
}
fn poll(c: &Connection, key: &[u8; 32], time: i64) -> Result<u32> {
    crate::missions::expire(
        c,
        time.checked_mul(1000).ok_or(StoreError::InvalidInput)?,
        None,
    )?;
    // Account epoch/suspension and parent changes also retire unfinished children during idle polling.
    let stale=c.prepare("SELECT DISTINCT o.tenant_id,o.schedule_id FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id JOIN reminder_schedules s ON s.tenant_id=o.tenant_id AND s.id=o.schedule_id JOIN accounts a ON a.tenant_id=s.tenant_id AND a.id=s.account_id WHERE m.state IN('queued','planning','executing','verifying','paused') AND (s.state!='active' OR s.revision!=o.schedule_revision OR s.auth_epoch!=m.auth_epoch OR a.auth_epoch!=m.auth_epoch OR a.status!='active' OR a.recovery_required=1 OR a.must_change_password=1) LIMIT 64")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (t, id) in stale {
        fence_children(c, &t, &id)?;
    }
    publish_results(c, time)?;
    // Owner fairness is persisted with the existing scheduling round-robin counter.
    let rows=c.prepare("WITH owners AS (SELECT o.tenant_id,o.id,o.schedule_id,o.account_id,o.revision,o.auth_epoch,o.scheduled_for,x.template,x.model,f.last_claim,row_number() OVER(PARTITION BY o.tenant_id,o.account_id ORDER BY o.scheduled_for,o.id) AS position FROM reminder_occurrences o JOIN recurring_missions x ON x.tenant_id=o.tenant_id AND x.schedule_id=o.schedule_id JOIN reminder_owners f ON f.tenant_id=o.tenant_id AND f.account_id=o.account_id WHERE o.state='queued') SELECT tenant_id,id,schedule_id,account_id,revision,auth_epoch,scheduled_for,template,model FROM owners WHERE position=1 ORDER BY last_claim,scheduled_for,id LIMIT 64")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let mut count = 0;
    for (t, occurrence, schedule, account, revision, epoch, at, template, model) in rows {
        let(valid,end):(bool,Option<i64>)=c.query_row("SELECT s.state='active' AND s.revision=?3 AND s.auth_epoch=?4 AND a.auth_epoch=?4 AND a.status='active' AND a.recovery_required=0 AND a.must_change_password=0 AND (a.temporary_expires_at IS NULL OR a.temporary_expires_at>?5),s.valid_until FROM reminder_schedules s JOIN accounts a ON a.tenant_id=s.tenant_id AND a.id=s.account_id WHERE s.tenant_id=?1 AND s.id=?2",params![t,schedule,revision,epoch,time],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if !valid {
            continue;
        }
        let expiry = at
            .checked_add(86400)
            .ok_or(StoreError::InvalidInput)?
            .min(end.unwrap_or(i64::MAX));
        if expiry <= time {
            c.execute("UPDATE reminder_occurrences SET state='expired',reason='mission_window_expired',updated_at=?3 WHERE tenant_id=?1 AND id=?2",params![t,occurrence,time])?;
            c.execute("UPDATE reminder_schedules SET state='expired',reason='mission_window_expired' WHERE tenant_id=?1 AND id=?2 AND next_due IS NULL",params![t,schedule])?;
            continue;
        }
        let choice: ModelChoice = decode(&model)?;
        if providers::binding_for_choice(c, key, &choice).is_err() {
            c.execute("UPDATE reminder_schedules SET state='paused',reason='provider_unavailable',revision=revision+1 WHERE tenant_id=?1 AND id=?2",params![t,schedule])?;
            c.execute("UPDATE reminder_occurrences SET state='skipped',reason='provider_unavailable',updated_at=?3 WHERE tenant_id=?1 AND id=?2",params![t,occurrence,time])?;
            fence_children(c, &t, &schedule)?;
            continue;
        }
        let busy:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND state IN('queued','planning','executing','verifying','paused')) OR (SELECT count(*) FROM missions WHERE state IN('queued','planning','executing','verifying'))>=16",[&t],|r|r.get(0))?;
        if busy {
            continue;
        }
        let mut input: MissionCreate = decode(&template)?;
        input.request_id = occurrence.clone();
        let model: serde_json::Value = decode(&model)?;
        // A zero digest is a non-session marker; durable authority never uses chat session auth.
        let id = crate::missions::create_pinned_in(
            c,
            &t,
            &account,
            epoch,
            &[0; 32],
            &input,
            &model,
            expiry.checked_mul(1000).ok_or(StoreError::InvalidInput)?,
            time,
        )?;
        c.execute("INSERT INTO recurring_mission_occurrences(tenant_id,occurrence_id,schedule_id,schedule_revision,mission_id,scheduled_for) VALUES(?1,?2,?3,?4,?5,?6)",params![t,occurrence,schedule,revision,id,at])?;
        c.execute("UPDATE reminder_occurrences SET state='delivered',reason='mission_admitted',updated_at=?3 WHERE tenant_id=?1 AND id=?2",params![t,occurrence,time])?;
        c.execute("UPDATE reminder_clock SET claim_order=claim_order+1 WHERE singleton=1 AND claim_order<9223372036854775807",[])?;
        c.execute("UPDATE reminder_owners SET last_claim=(SELECT claim_order FROM reminder_clock WHERE singleton=1) WHERE tenant_id=?1 AND account_id=?2",params![t,account])?;
        count += 1;
    }
    Ok(count)
}
// Deliberately narrow: successful exact body plus actual decimal/source receipts only.
// Unrecognized evidence is incomparable and always produces a notice.
fn exact_result(c: &Connection, t: &str, id: &str) -> Result<Option<String>> {
    let run:String=c.query_row("SELECT run_id FROM missions WHERE tenant_id=?1 AND id=?2 AND state='achieved' AND uncertain=0 AND candidate IS NOT NULL",params![t,id],|r|r.get(0)).optional()?.unwrap_or_default();
    if run.is_empty() {
        return Ok(None);
    }
    let candidate: String = c.query_row(
        "SELECT candidate FROM missions WHERE tenant_id=?1 AND id=?2",
        params![t, id],
        |r| r.get(0),
    )?;
    let candidate: MissionCandidate = decode(&candidate)?;
    let mut values = Vec::new();
    for e in candidate.evidence {
        let raw:Option<String>=c.query_row("SELECT t.result FROM agent_tool_slots t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND s.state='completed'",params![t,run,e.step,e.call_id],|r|r.get(0)).optional()?.flatten();
        let Some(raw) = raw else { return Ok(None) };
        let value = match decode::<KnowledgeToolResult>(&raw)? {
            KnowledgeToolResult::Decimal { exact } => {
                serde_json::json!({"kind":"decimal","exact":exact})
            }
            KnowledgeToolResult::Source(s) => {
                serde_json::json!({"kind":"source","id":s.source.id,"revision":s.source.revision,"text":s.text})
            }
            _ => return Ok(None),
        };
        values.push(encode(&value)?);
    }
    if values.is_empty() {
        return Ok(None);
    }
    values.sort();
    values.dedup();
    Ok(Some(encode(
        &serde_json::json!({"text":candidate.text,"receipts":values}),
    )?))
}
fn publish_results(c: &Connection, time: i64) -> Result<()> {
    let rows=c.prepare("SELECT o.tenant_id,o.occurrence_id,o.schedule_id,o.mission_id,o.scheduled_for,o.schedule_revision,m.account_id,m.state,m.auth_epoch,x.changes_only FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id JOIN recurring_missions x ON x.tenant_id=o.tenant_id AND x.schedule_id=o.schedule_id WHERE o.publication_state='pending' AND m.state NOT IN('queued','planning','executing','verifying','paused') ORDER BY o.scheduled_for,o.occurrence_id LIMIT 32")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,i64>(8)?,r.get::<_,bool>(9)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (t, occurrence, schedule, mission, at, revision, account, state, epoch, changes) in rows {
        let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM reminder_schedules s JOIN accounts a ON a.tenant_id=s.tenant_id AND a.id=s.account_id WHERE s.tenant_id=?1 AND s.id=?2 AND s.revision=?3 AND s.auth_epoch=?4 AND s.state='active' AND a.auth_epoch=?4 AND a.status='active' AND a.recovery_required=0 AND a.must_change_password=0 AND (s.valid_until IS NULL OR s.valid_until>?5))",params![t,schedule,revision,epoch,time],|r|r.get(0))?;
        if !valid || state == "deleted" {
            c.execute("UPDATE recurring_mission_occurrences SET publication_state='fenced' WHERE tenant_id=?1 AND occurrence_id=?2",params![t,occurrence])?;
            continue;
        }
        let mut unchanged = false;
        if changes
            && state == "achieved"
            && let Some(current) = exact_result(c, &t, &mission)?
        {
            let previous:Option<String>=c.query_row("SELECT o.mission_id FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id WHERE o.tenant_id=?1 AND o.schedule_id=?2 AND o.scheduled_for<?3 AND o.schedule_revision=?4 AND m.auth_epoch=?5 ORDER BY o.scheduled_for DESC LIMIT 1",params![t,schedule,at,revision,epoch],|r|r.get(0)).optional()?;
            if let Some(previous) = previous {
                unchanged = exact_result(c, &t, &previous)?.as_ref() == Some(&current);
            }
        }
        if unchanged {
            c.execute("UPDATE recurring_mission_occurrences SET publication_state='exact_unchanged' WHERE tenant_id=?1 AND occurrence_id=?2",params![t,occurrence])?;
        } else {
            let message = format!("실행 상태: {state}");
            let notification = crate::schedules::publish_mission_notice(
                c,
                &t,
                &account,
                &schedule,
                &occurrence,
                at,
                time,
                &message,
            )?;
            c.execute("UPDATE recurring_mission_occurrences SET publication_state='published',notification_id=?3 WHERE tenant_id=?1 AND occurrence_id=?2",params![t,occurrence,notification])?;
            c.execute(
                "UPDATE reminder_occurrences SET notification_id=?3 WHERE tenant_id=?1 AND id=?2",
                params![t, occurrence, notification],
            )?;
        }
        c.execute("UPDATE reminder_schedules SET state='completed',reason=NULL,updated_at=?3 WHERE tenant_id=?1 AND id=?2 AND state='active' AND next_due IS NULL",params![t,schedule,time])?;
    }
    Ok(())
}

pub(crate) fn create_owner_in(
    tx: &Connection,
    key: &[u8; 32],
    u: &UserContext,
    input: RecurringMissionCreate,
    choice: ModelChoice,
    profile: RunToolProfile,
) -> Result<RecurringMissionView> {
    Uuid::parse_str(&input.request_id).map_err(|_| StoreError::InvalidInput)?;
    crate::missions::validate_input(&input.template)?;
    if !matches!(input.template.execution_mode, MissionMode::Durable) {
        return Err(StoreError::InvalidInput);
    }
    let digest = Sha256::digest(encode(&input)?.as_bytes()).to_vec();
    if let Some((id,old))=tx.query_row("SELECT schedule_id,payload_digest FROM recurring_missions WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![u.tenant.0,u.account.id,input.request_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?))).optional()? {
                if old!=digest{return Err(StoreError::Conflict)}
                return view(tx,u,&id)
            }
    if !choice.function_tools {
        return Err(StoreError::InvalidInput);
    }
    providers::binding_for_choice(tx, key, &choice)?;
    let profile = crate::missions::read_profile(profile);
    let mut model = serde_json::to_value(choice).map_err(|_| StoreError::InvalidInput)?;
    model["agent_profile"] = serde_json::to_value(profile).map_err(|_| StoreError::InvalidInput)?;
    // Titles/messages of the scheduler sidecar never contain mission or source content.
    let mut draft = input.schedule;
    draft.message = "예약한 읽기 업무".into();
    let schedule = crate::schedules::create_in(
        tx,
        u,
        &ScheduleCreate {
            request_id: Uuid::new_v4().to_string(),
            draft,
        },
        now()?,
    )?;
    tx.execute(
        "INSERT INTO recurring_missions VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            u.tenant.0,
            schedule.id,
            u.account.id,
            input.request_id,
            digest,
            encode(&input.template)?,
            encode(&model)?,
            input.changes_only
        ],
    )?;
    let out = view(tx, u, &schedule.id)?;
    Ok(out)
}

pub(crate) fn control_owner_in(
    tx: &Connection,
    key: &[u8; 32],
    u: &UserContext,
    id: &str,
    input: ScheduleControl,
) -> Result<RecurringMissionView> {
    let old = view(tx, u, id)?;
    if input.action == "resume" {
        providers::binding_for_choice(tx, key, &old.model)?;
    }
    let changed = crate::schedules::control_recurring_in(tx, u, id, &input, now()?)?;
    if changed.revision != old.schedule.revision {
        fence_children(tx, &u.tenant.0, id)?;
    }
    let out = view(tx, u, id)?;
    Ok(out)
}

pub(crate) fn edit_owner_in(
    tx: &Connection,
    key: &[u8; 32],
    u: &UserContext,
    id: &str,
    mut input: ScheduleEdit,
) -> Result<RecurringMissionView> {
    let old = view(tx, u, id)?;
    providers::binding_for_choice(tx, key, &old.model)?;
    input.draft.message = "예약한 읽기 업무".into();
    crate::schedules::edit_recurring_in(tx, u, id, &input, now()?)?;
    fence_children(tx, &u.tenant.0, id)?;
    let out = view(tx, u, id)?;
    Ok(out)
}
