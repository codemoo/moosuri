//! Ambiguous owner selection, correction and cancellation on the original run.
use super::*;
use crate::repair_choice::FIXTURE;
impl World {
    async fn seed_repair_estimates(&self) -> EvalResult<()> {
        let collection = value_id(
            &self
                .checked(
                    "POST",
                    "/api/v1/knowledge/collections",
                    json!({"name":"Synthetic appliance estimates","scope":"household-estimates"}),
                    201,
                )
                .await?,
        )?;
        let entries=FIXTURE.sources.iter().enumerate().map(|(i,s)|json!({"external_id":format!("estimate-{i}"),"version":"v1","expected_revision":null,"title":s.title,"text":s.text,"speaker":"Synthetic repair shop","observed_at":world::FIXTURE_CLOCK,"valid_from":null,"valid_until":null})).collect::<Vec<_>>();
        self.checked("POST",&format!("/api/v1/knowledge/collections/{collection}/batches"),json!({"request_id":uuid::Uuid::new_v4(),"expected_cursor":0,"next_cursor":1,"complete_snapshot":false,"entries":entries}),201).await?;
        Ok(())
    }
    pub(super) async fn execute_repair_choice(&mut self, out: &mut Execution) -> EvalResult<()> {
        *self.stages.lock().unwrap_or_else(|e| e.into_inner()) = stage_plan(Kind::RepairChoice);
        let mark = self.begin_stage("observe_estimates_and_ask")?;
        self.seed_repair_estimates().await?;
        let started = Instant::now();
        let admitted=self.checked("POST","/api/v1/missions",json!({"request_id":uuid::Uuid::new_v4(),"task":FIXTURE.task,"constraints":"","checks":[{"kind":"user_review","description":"Review the selected appliance estimate before any further action."}],"execution_mode":"durable"}),200).await?;
        self.admitted_timing();
        let admission_ms = ms(started);
        let id = value_id(&admitted)?;
        let waiting_view = self.mission_settled(&id).await?;
        let waiting = observe::repair_choice::snapshot(self.dir.path(), &id)?;
        let before = self.mission_snapshot(&id)?;
        self.end_stage(
            mark,
            &[
                (
                    "source_choice_wait",
                    waiting_view["state"] == "waiting"
                        && waiting_view["input_wait"]["field"] == "source_choice",
                ),
                (
                    "two_candidates",
                    waiting_view["input_wait"]["candidates"]
                        .as_array()
                        .is_some_and(|cs| cs.len() == 2),
                ),
                (
                    "completed_work",
                    before.slots.len() == 3 && before.slots.iter().all(|s| s.result.is_some()),
                ),
            ],
        )?;
        let mark = self.begin_stage("ambiguous_answer_rejected")?;
        let input_url = format!("/api/v1/missions/{id}/input");
        let ambiguous_request = uuid::Uuid::new_v4().to_string();
        let (invalid_status,invalid_response)=self.request("POST",&input_url,json!({"request_id":ambiguous_request,"wait_request_id":waiting_view["input_wait"]["request_id"],"expected_revision":waiting_view["revision"],"value":FIXTURE.ambiguous_answer})).await?;
        let rejected = observe::repair_choice::snapshot(self.dir.path(), &id)?;
        self.end_stage(
            mark,
            &[
                ("invalid_input_response", invalid_status == 400),
                ("same_wait_work_notice", waiting == rejected),
            ],
        )?;
        let mark = self.begin_stage("correct_selection_and_replay")?;
        // Pause the already waiting Runtime before the valid reply so its ACK and
        // exact replay can be measured independently of the resume dispatch.
        self.shutdown().await;
        let selected = waiting_view["input_wait"]["candidates"]
            .as_array()
            .and_then(|cs| cs.iter().find(|c| c["label"] == FIXTURE.selected_label))
            .cloned()
            .ok_or("repair_choice_owner_card")?;
        let answer = json!({"request_id":uuid::Uuid::new_v4(),"wait_request_id":waiting_view["input_wait"]["request_id"],"expected_revision":waiting_view["revision"],"value":selected["id"]});
        let corrected = self
            .checked("POST", &input_url, answer.clone(), 200)
            .await?;
        let replay = self.checked("POST", &input_url, answer, 200).await?;
        let corrected_saved = observe::repair_choice::snapshot(self.dir.path(), &id)?;
        self.end_stage(
            mark,
            &[
                (
                    "one_reply",
                    corrected_saved["replies"]
                        .as_array()
                        .is_some_and(|rs| rs.len() == 1),
                ),
                ("same_reply_result", corrected == replay),
                (
                    "same_run",
                    corrected_saved["mission"]["run"] == before.run_id,
                ),
            ],
        )?;
        let mark = self.begin_stage("reopen_resume_original_work")?;
        self.restart().await?;
        self.wait_dispatches(4).await?;
        let resumed = observe::repair_choice::snapshot(self.dir.path(), &id)?;
        let resumed_work = self.mission_snapshot(&id)?;
        self.end_stage(
            mark,
            &[
                ("original_run", resumed_work.run_id == before.run_id),
                (
                    "same_completed_slots",
                    serde_json::to_value(&before.slots).ok()
                        == serde_json::to_value(&resumed_work.slots).ok(),
                ),
                (
                    "selected_input_resumed",
                    resumed["waits"][0]["state"] == "resumed"
                        && resumed["waits"][0]["value"] == selected["id"],
                ),
            ],
        )?;
        let mark = self.begin_stage("cancel_before_publication")?;
        let current = self.mission_view(&id).await?;
        let control = json!({"request_id":uuid::Uuid::new_v4(),"expected_revision":current["revision"],"action":{"kind":"cancel"}});
        let control_url = format!("/api/v1/missions/{id}/control");
        let cancelled = self
            .checked("POST", &control_url, control.clone(), 200)
            .await?;
        let cancel_replay = self.checked("POST", &control_url, control, 200).await?;
        let cancelled_saved = observe::repair_choice::snapshot(self.dir.path(), &id)?;
        self.end_stage(
            mark,
            &[
                ("cancelled", cancelled["state"] == "cancelled"),
                ("same_cancel_receipt", cancelled == cancel_replay),
                (
                    "no_result",
                    cancelled_saved["mission"]["candidate"].is_null(),
                ),
            ],
        )?;
        let mark = self.begin_stage("late_response_and_reopen")?;
        let runtime = self.runtime.as_ref().ok_or("repair_choice_runtime")?;
        let mut activity = runtime.subscribe_run_activity();
        let before_release = *activity.borrow_and_update();
        if before_release != 1 || activity.has_changed().is_err() || runtime.is_stopping() {
            return Err("repair_choice_no_live_worker_before_release");
        }
        self.provider.release();
        // The provider ledger records a planned response before its hold. Only
        // actual supervisor reap plus local retirement settlement proves that
        // the worker is finished. Keep this Runtime alive throughout the wait.
        tokio::time::timeout(Duration::from_secs(8), async {
            while *activity.borrow_and_update() != 0 {
                activity
                    .changed()
                    .await
                    .map_err(|_| "repair_choice_activity_closed")?;
            }
            Ok::<_, &'static str>(())
        })
        .await
        .map_err(|_| "repair_choice_live_settlement_timeout")??;
        let after_release = *activity.borrow_and_update();
        let runtime_alive = !runtime.is_stopping();
        let watcher_open = activity.has_changed().is_ok();
        if after_release != 0 || !runtime_alive || !watcher_open {
            return Err("repair_choice_settlement_requires_live_runtime");
        }
        let live_settled = observe::repair_choice::snapshot(self.dir.path(), &id)?;
        let live_observation = self.mission_snapshot(&id)?;
        let live_inbox = self
            .checked("GET", "/api/v1/notifications", Value::Null, 200)
            .await?;
        if live_settled["mission"]["state"] != "cancelled"
            || !live_settled["mission"]["candidate"].is_null()
            || live_observation.state != "cancelled"
            || !live_observation.output.is_empty()
            || live_inbox["items"]
                .as_array()
                .is_none_or(|items| !items.is_empty())
        {
            return Err("repair_choice_late_publication_before_shutdown");
        }
        let runtime_settlement = json!({"before_release":before_release,"after_release":after_release,"runtime_alive":runtime_alive,"watcher_open":watcher_open});
        // Restart is a separate persistence check after the live cancellation
        // fence and worker-settlement observation have already succeeded.
        self.restart().await?;
        let final_saved = observe::repair_choice::snapshot(self.dir.path(), &id)?;
        let final_observation = self.mission_snapshot(&id)?;
        let inbox = self
            .checked("GET", "/api/v1/notifications", Value::Null, 200)
            .await?;
        self.end_stage(
            mark,
            &[
                (
                    "cancelled_after_late_response",
                    final_saved["mission"]["state"] == "cancelled"
                        && final_observation.state == "cancelled",
                ),
                (
                    "no_final_candidate",
                    final_saved["mission"]["candidate"].is_null(),
                ),
                (
                    "notice_hidden",
                    inbox["items"].as_array().is_some_and(Vec::is_empty),
                ),
                ("no_new_dispatch", self.provider.ledger().len() == 4),
                (
                    "worker_settled_while_runtime_alive",
                    runtime_alive && watcher_open && before_release == 1 && after_release == 0,
                ),
            ],
        )?;
        out.outcome = json!({"waiting":waiting,"rejected":rejected,"invalid_status":invalid_status,"invalid_response":invalid_response,"ambiguous_request_id":ambiguous_request,"before_slots":before.slots,"before_evidence":before.evidence,"before_candidates":before.returned_candidates,"selected":selected,"corrected":corrected_saved,"corrected_view":corrected,"reply_replay":replay,"resumed":resumed,"resumed_slots":resumed_work.slots,"cancelled":cancelled_saved,"cancelled_view":cancelled,"cancel_replay":cancel_replay,"runtime_settlement":runtime_settlement,"live_settled":live_settled,"live_inbox":live_inbox,"final":final_saved,"inbox":inbox});
        out.observations.push(final_observation);
        out.timings.push(TurnTiming {
            admission_ms,
            terminal_observed_ms: ms(started),
        });
        Ok(())
    }
}
