//! Shared stage accounting and HTTP continuity drivers. No oracle imports.
use super::*;
use crate::everyday as fixture;
use std::collections::BTreeSet;
type ToolSlots = BTreeSet<(String, i64, String)>;

#[derive(Clone, Serialize)]
pub struct Stage {
    pub name: &'static str,
    pub status: &'static str,
    pub model_limit: usize,
    pub tool_limit: usize,
    pub model_calls: Option<usize>,
    pub tool_calls: Option<usize>,
    pub elapsed_ms: Option<u64>,
    pub postconditions: Vec<(String, bool)>,
    #[serde(skip)]
    active: Option<(usize, ToolSlots, Instant)>,
}
pub fn plan(kind: Kind) -> Vec<Stage> {
    let stages: &[(&str, usize, usize)] = match kind {
        Kind::FollowupMonitor => &[
            ("read_status_and_propose", 2, 1),
            ("accept_and_exact_replay", 0, 0),
            ("unchanged_no_inference", 0, 0),
            ("changed_source_one_child", 2, 1),
            ("unchanged_no_duplicate", 0, 0),
            ("owner_stop_and_replay", 0, 0),
            ("stopped_reopen", 0, 0),
        ],
        Kind::EverydayGuidance => &[
            ("read_and_calculate", 4, 3),
            ("instruction_ack_and_exact_replay", 0, 0),
            ("same_run_continuation", 1, 0),
            ("reopen", 0, 0),
        ],
        Kind::EverydayClarification => &[
            ("read_and_ask", 4, 2),
            ("waiting_reopen", 0, 0),
            ("owner_answer_and_resume", 2, 1),
            ("reply_exact_replay", 0, 0),
        ],
        Kind::EverydayReminder => &[
            ("create", 0, 0),
            ("receipt_bound_change", 0, 0),
            ("change_exact_replay", 0, 0),
            ("receipt_bound_cancel", 0, 0),
            ("reopen", 0, 0),
        ],
        _ => &[],
    };
    stages
        .iter()
        .map(|&(name, model_limit, tool_limit)| Stage {
            name,
            status: "not_run",
            model_limit,
            tool_limit,
            model_calls: None,
            tool_calls: None,
            elapsed_ms: None,
            postconditions: vec![],
            active: None,
        })
        .collect()
}
pub(super) struct Mark {
    index: usize,
    model: usize,
    tools: ToolSlots,
    at: Instant,
}
impl World {
    pub fn stages(&self) -> Vec<Stage> {
        let mut stages = self
            .stages
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let model = self.provider.ledger().len();
        let tools = self.tool_slots().ok();
        for stage in &mut stages {
            if let Some((start_model, start_tools, start)) = &stage.active {
                stage.model_calls = model.checked_sub(*start_model);
                stage.tool_calls = tools
                    .as_ref()
                    .map(|now| now.difference(start_tools).count());
                stage.elapsed_ms = Some(ms(*start));
            }
        }
        stages
    }
    fn tool_slots(&self) -> EvalResult<ToolSlots> {
        let conn = observe::open(self.dir.path())?;
        conn.prepare("SELECT run_id,step,call_id FROM agent_tool_slots")
            .map_err(|_| "stage_tool_slots")?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|_| "stage_tool_slots")?
            .collect::<Result<ToolSlots, _>>()
            .map_err(|_| "stage_tool_slots")
    }
    pub(super) fn begin_stage(&self, name: &str) -> EvalResult<Mark> {
        let model = self.provider.ledger().len();
        let tools = self.tool_slots()?;
        let mut stages = self.stages.lock().unwrap_or_else(|e| e.into_inner());
        let index = stages
            .iter()
            .position(|s| s.name == name)
            .ok_or("stage_name")?;
        // If execution errors before finish, this remains failed, and subsequent
        // planned stages remain not_run. No successful denominator shrinkage.
        stages[index].status = "fail";
        stages[index].active = Some((model, tools.clone(), Instant::now()));
        Ok(Mark {
            index,
            model,
            tools,
            at: Instant::now(),
        })
    }
    pub(super) fn end_stage(&self, mark: Mark, checks: &[(&str, bool)]) -> EvalResult<()> {
        let model = self
            .provider
            .ledger()
            .len()
            .checked_sub(mark.model)
            .ok_or("stage_model_counter")?;
        // Rows may be withdrawn while a new source revision is ingested.
        // Count newly observed identities rather than subtracting total row
        // counts, which would hide a child's real tool when parent rows vanish.
        let tools = self.tool_slots()?.difference(&mark.tools).count();
        let mut stages = self.stages.lock().unwrap_or_else(|e| e.into_inner());
        let stage = &mut stages[mark.index];
        stage.active = None;
        stage.model_calls = Some(model);
        stage.tool_calls = Some(tools);
        stage.elapsed_ms = Some(ms(mark.at));
        stage.postconditions = checks
            .iter()
            .map(|(name, ok)| (name.to_string(), *ok))
            .collect();
        stage.status = if model <= stage.model_limit
            && tools <= stage.tool_limit
            && checks.iter().all(|(_, ok)| *ok)
        {
            "pass"
        } else {
            "fail"
        };
        if stage.status == "pass" {
            Ok(())
        } else {
            Err("stage_contract_failed")
        }
    }
    async fn wait_dispatches(&self, count: usize) -> EvalResult<()> {
        tokio::time::timeout(Duration::from_secs(8), async {
            while self.provider.ledger().len() < count {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .map_err(|_| "everyday_dispatch_timeout")
    }
    pub(super) async fn mission_view(&self, id: &str) -> EvalResult<Value> {
        self.checked("GET", "/api/v1/missions", Value::Null, 200)
            .await?
            .as_array()
            .and_then(|ms| ms.iter().find(|m| m["id"] == id))
            .cloned()
            .ok_or("everyday_mission_view")
    }
    pub(super) async fn mission_settled(&self, id: &str) -> EvalResult<Value> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
        loop {
            let m = self.mission_view(id).await?;
            if !matches!(
                m["state"].as_str(),
                Some("queued" | "planning" | "executing" | "verifying")
            ) {
                return Ok(m);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err("everyday_mission_timeout");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    pub(super) fn mission_snapshot(&self, id: &str) -> EvalResult<Observation> {
        let run: String = observe::open(self.dir.path())?
            .query_row("SELECT run_id FROM missions WHERE id=?1", [id], |r| {
                r.get(0)
            })
            .map_err(|_| "everyday_mission_run")?;
        observe::snapshot(self.dir.path(), &run, self.provider.ledger())
    }
    fn record_run_timing(&self, view: &Value) {
        let index = self.admitted_timing();
        self.server_timings
            .lock()
            .unwrap_or_else(|e| e.into_inner())[index] =
            serde_json::from_value(view["timings"].clone()).ok();
    }
    fn schedule_snapshot(&self, id: &str) -> EvalResult<Value> {
        let conn = observe::open(self.dir.path())?;
        let (mut row, rule) = conn.query_row(
            "SELECT tenant_id,account_id,id,revision,state,rule,title,message,created_at FROM reminder_schedules WHERE account_id=?1 AND id=?2",
            rusqlite::params![self.login().user.account().id, id],
            |r| Ok((json!({"tenant":r.get::<_,String>(0)?,"account":r.get::<_,String>(1)?,"id":r.get::<_,String>(2)?,"revision":r.get::<_,i64>(3)?,"state":r.get::<_,String>(4)?,"title":r.get::<_,String>(6)?,"message":r.get::<_,String>(7)?,"created_at":r.get::<_,i64>(8)?}), r.get::<_,String>(5)?)),
        ).map_err(|_| "everyday_schedule_snapshot")?;
        row["rule"] = serde_json::from_str(&rule).map_err(|_| "everyday_schedule_rule")?;
        Ok(row)
    }
    async fn reminder_turn(&self, thread: &str, text: &str) -> EvalResult<Value> {
        let turn=self.checked("POST", &format!("/api/v1/threads/{thread}/messages"), json!({"request_id":uuid::Uuid::new_v4(),"text":text,"schedule_context":{"timezone":"Asia/Seoul","target":null}}),201).await?;
        self.record_run_timing(&turn["run"]);
        Ok(turn)
    }
    pub(super) async fn execute_everyday(
        &mut self,
        kind: Kind,
        out: &mut Execution,
    ) -> EvalResult<()> {
        *self.stages.lock().unwrap_or_else(|e| e.into_inner()) = plan(kind);
        match kind {
            Kind::EverydayGuidance => {
                out.source = Some(self.source(self.login(), fixture::QUOTE).await?);
                let mark = self.begin_stage("read_and_calculate")?;
                let thread = self.new_thread().await?;
                let path = format!("/api/v1/threads/{thread}/messages");
                let started = Instant::now();
                let accepted = self
                    .checked(
                        "POST",
                        &path,
                        json!({"request_id":uuid::Uuid::new_v4(),"text":fixture::QUESTION}),
                        201,
                    )
                    .await?;
                let index = self.admitted_timing();
                let admission = ms(started);
                let run = accepted["run"]["id"]
                    .as_str()
                    .ok_or("everyday_run")?
                    .to_string();
                self.wait_dispatches(4).await?;
                let before = observe::snapshot(self.dir.path(), &run, self.provider.ledger())?;
                self.end_stage(
                    mark,
                    &[(
                        "three_completed_tool_receipts",
                        before.slots.len() == 3 && before.slots.iter().all(|s| s.result.is_some()),
                    )],
                )?;
                let mark = self.begin_stage("instruction_ack_and_exact_replay")?;
                let instruction = json!({"request_id":uuid::Uuid::new_v4(),"expected_attempt":1,"text":fixture::GUIDANCE});
                let url = format!("/api/v1/runs/{run}/instructions");
                let ack = self.checked("POST", &url, instruction.clone(), 202).await?;
                let replay = self.checked("POST", &url, instruction.clone(), 202).await?;
                self.end_stage(
                    mark,
                    &[
                        ("received", ack["state"] == "received"),
                        ("same_receipt", ack == replay),
                    ],
                )?;
                let mark = self.begin_stage("same_run_continuation")?;
                self.provider.release();
                let (observation, timing, _) = self
                    .observe_turn(&thread, &run, 0, started, admission, index, true)
                    .await?;
                let receipt = self
                    .checked(
                        "GET",
                        &format!(
                            "{url}/{}",
                            instruction["request_id"]
                                .as_str()
                                .ok_or("everyday_request")?
                        ),
                        Value::Null,
                        200,
                    )
                    .await?;
                self.end_stage(
                    mark,
                    &[
                        ("applied", receipt["state"] == "applied"),
                        ("same_run", receipt["run_id"] == run),
                    ],
                )?;
                let mark = self.begin_stage("reopen")?;
                self.restart().await?;
                let reopened = observe::snapshot(self.dir.path(), &run, self.provider.ledger())?;
                let persisted = self
                    .checked(
                        "GET",
                        &format!(
                            "{url}/{}",
                            instruction["request_id"]
                                .as_str()
                                .ok_or("everyday_request")?
                        ),
                        Value::Null,
                        200,
                    )
                    .await?;
                self.end_stage(
                    mark,
                    &[
                        ("result_preserved", reopened.output == observation.output),
                        ("receipt_preserved", receipt == persisted),
                    ],
                )?;
                out.outcome = json!({"ack":ack,"replay":replay,"receipt":receipt,"before_slots":before.slots,"reopened_slots":reopened.slots,"reopened_output":reopened.output});
                out.observations.push(observation);
                out.timings.push(timing);
            }
            Kind::EverydayClarification => {
                let source = self.source(self.login(), fixture::QUOTE).await?;
                out.source = Some(source.clone());
                let mark = self.begin_stage("read_and_ask")?;
                let started = Instant::now();
                let admitted=self.checked("POST","/api/v1/missions",json!({"request_id":uuid::Uuid::new_v4(),"task":fixture::CRITERION_TASK,"constraints":"","checks":[{"kind":"decimal_equals","expected":"24.75"},{"kind":"source_quote","id":source,"revision":1,"quote":fixture::QUOTE}],"execution_mode":"durable"}),200).await?;
                self.admitted_timing();
                let admission_ms = ms(started);
                let id = value_id(&admitted)?;
                let waiting = self.mission_settled(&id).await?;
                let before = self.mission_snapshot(&id)?;
                self.end_stage(
                    mark,
                    &[
                        ("waiting_for_owner", waiting["state"] == "waiting"),
                        (
                            "source_read_saved",
                            before.slots.iter().any(|s| {
                                s.input["tool"] == "knowledge_read_source" && s.result.is_some()
                            }),
                        ),
                    ],
                )?;
                let mark = self.begin_stage("waiting_reopen")?;
                self.restart().await?;
                let reopened = self.mission_view(&id).await?;
                let reopen_observation = self.mission_snapshot(&id)?;
                self.end_stage(
                    mark,
                    &[
                        ("same_wait", reopened["input_wait"] == waiting["input_wait"]),
                        (
                            "same_saved_receipts",
                            serde_json::to_value(&before.slots).ok()
                                == serde_json::to_value(&reopen_observation.slots).ok(),
                        ),
                    ],
                )?;
                let mark = self.begin_stage("owner_answer_and_resume")?;
                let answer = json!({"request_id":uuid::Uuid::new_v4(),"wait_request_id":reopened["input_wait"]["request_id"],"expected_revision":reopened["revision"],"value":fixture::CRITERION_ANSWER});
                let url = format!("/api/v1/missions/{id}/input");
                self.checked("POST", &url, answer.clone(), 200).await?;
                let final_view = self.mission_settled(&id).await?;
                let observation = self.mission_snapshot(&id)?;
                self.end_stage(
                    mark,
                    &[
                        ("resumed_original_run", observation.run_id == before.run_id),
                        ("semantic_review_retained", final_view["state"] == "partial"),
                    ],
                )?;
                let mark = self.begin_stage("reply_exact_replay")?;
                let replay = self.checked("POST", &url, answer, 200).await?;
                let conn = observe::open(self.dir.path())?;
                let replies: i64 = conn
                    .query_row(
                        "SELECT count(*) FROM mission_input_replies WHERE mission_id=?1",
                        [&id],
                        |r| r.get(0),
                    )
                    .map_err(|_| "everyday_reply_count")?;
                drop(conn);
                self.end_stage(
                    mark,
                    &[
                        ("one_reply", replies == 1),
                        ("same_final_state", replay["state"] == final_view["state"]),
                    ],
                )?;
                out.outcome = json!({"waiting":waiting,"reopened":reopened,"final":final_view,"before_slots":before.slots,"reply_count":replies});
                out.observations.push(observation);
                out.timings.push(TurnTiming {
                    admission_ms,
                    terminal_observed_ms: ms(started),
                });
            }
            Kind::EverydayReminder => {
                // A real second reminder prevents success through sole-row fallback.
                let decoy=self.checked("POST","/api/v1/schedules",json!({"request_id":uuid::Uuid::new_v4(),"draft":{"title":"Synthetic unrelated reminder","message":"Unrelated reminder","rule":{"kind":"interval","anchor":1800000000_i64,"seconds":86400},"valid_until":null}}),200).await?;
                let decoy_before = self.schedule_snapshot(&value_id(&decoy)?)?;
                let thread = self.new_thread().await?;
                let mark = self.begin_stage("create")?;
                let created = self
                    .reminder_turn(&thread, fixture::CREATE_REMINDER)
                    .await?;
                let receipt = created["run"]["schedule_receipts"][0].clone();
                let created_saved = self.schedule_snapshot(&value_id(&receipt)?)?;
                self.end_stage(mark, &[("creation_receipt", receipt["action"] == "create")])?;
                let mark = self.begin_stage("receipt_bound_change")?;
                let request = json!({"request_id":uuid::Uuid::new_v4(),"text":fixture::CHANGE_REMINDER,"schedule_context":{"timezone":"Asia/Seoul","target":null}});
                let url = format!("/api/v1/threads/{thread}/messages");
                let changed = self.checked("POST", &url, request.clone(), 201).await?;
                self.record_run_timing(&changed["run"]);
                let changed_receipt = changed["run"]["schedule_receipts"][0].clone();
                let changed_saved = self.schedule_snapshot(&value_id(&receipt)?)?;
                self.end_stage(
                    mark,
                    &[
                        ("same_target", changed_receipt["id"] == receipt["id"]),
                        ("edited", changed_receipt["action"] == "edit"),
                    ],
                )?;
                let mark = self.begin_stage("change_exact_replay")?;
                let replay = self.checked("POST", &url, request, 201).await?;
                let replayed_saved = self.schedule_snapshot(&value_id(&receipt)?)?;
                self.end_stage(
                    mark,
                    &[
                        ("same_run", replay["run"]["id"] == changed["run"]["id"]),
                        (
                            "same_receipt",
                            replay["run"]["schedule_receipts"][0] == changed_receipt
                                && replayed_saved == changed_saved,
                        ),
                    ],
                )?;
                let mark = self.begin_stage("receipt_bound_cancel")?;
                let cancelled = self
                    .reminder_turn(&thread, fixture::CANCEL_REMINDER)
                    .await?;
                let cancel_receipt = cancelled["run"]["schedule_receipts"][0].clone();
                self.end_stage(
                    mark,
                    &[
                        ("same_target", cancel_receipt["id"] == receipt["id"]),
                        ("revoked", cancel_receipt["state"] == "revoked"),
                    ],
                )?;
                let mark = self.begin_stage("reopen")?;
                self.restart().await?;
                let schedules = self
                    .checked("GET", "/api/v1/schedules", Value::Null, 200)
                    .await?;
                let conn = observe::open(self.dir.path())?;
                let notification_count: i64 = conn
                    .query_row(
                        "SELECT count(*) FROM local_notifications WHERE schedule_id=?1",
                        [receipt["id"].as_str().ok_or("everyday_schedule_id")?],
                        |r| r.get(0),
                    )
                    .map_err(|_| "everyday_notification_count")?;
                drop(conn);
                let reopened_saved = self.schedule_snapshot(&value_id(&receipt)?)?;
                let decoy_after = self.schedule_snapshot(&value_id(&decoy)?)?;
                let preserved = schedules.as_array().is_some_and(|rows| {
                    rows.iter().any(|s| {
                        s["id"] == decoy["id"]
                            && s["revision"] == decoy["revision"]
                            && s["state"] == decoy["state"]
                    })
                });
                self.end_stage(
                    mark,
                    &[
                        ("decoy_unchanged", preserved && decoy_before == decoy_after),
                        (
                            "revoked_target_persisted",
                            reopened_saved["state"] == "revoked" && reopened_saved["revision"] == 3,
                        ),
                        ("cancelled_no_notification", notification_count == 0),
                    ],
                )?;
                let run = cancelled["run"]["id"]
                    .as_str()
                    .ok_or("everyday_reminder_run")?;
                out.observations
                    .push(observe::snapshot(self.dir.path(), run, vec![])?);
                out.outcome = json!({"created":receipt,"changed":changed_receipt,"cancelled":cancel_receipt,"replay":replay["run"],"changed_run":changed["run"]["id"],"schedules":schedules,"decoy":decoy,"notifications":notification_count,"created_saved":created_saved,"changed_saved":changed_saved,"replayed_saved":replayed_saved,"reopened_saved":reopened_saved,"decoy_before":decoy_before,"decoy_after":decoy_after});
            }
            _ => return Err("everyday_kind"),
        }
        Ok(())
    }
}
