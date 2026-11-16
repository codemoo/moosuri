//! One saved-source lifecycle; HTTP owner operations and public scheduler clock.
use super::*;
use crate::followup as fixture;
use std::time::{SystemTime, UNIX_EPOCH};

impl World {
    async fn followup_ingest(
        &self,
        collection: &str,
        cursor: i64,
        text: &str,
    ) -> EvalResult<Value> {
        self.checked("POST",&format!("/api/v1/knowledge/collections/{collection}/batches"),json!({"request_id":uuid::Uuid::new_v4(),"expected_cursor":cursor,"next_cursor":cursor+1,"complete_snapshot":false,"entries":[{"external_id":"return-rma-104","version":format!("v{}",cursor+1),"expected_revision":if cursor==0 {Value::Null}else{json!(cursor)},"title":"Synthetic return status","text":text,"speaker":"Synthetic shop status","observed_at":world::FIXTURE_CLOCK+cursor,"valid_from":null,"valid_until":null}]}),201).await
    }
    async fn monitor_view(&self, id: &str) -> EvalResult<Value> {
        self.checked("GET", "/api/v1/task-monitors", Value::Null, 200)
            .await?
            .as_array()
            .and_then(|ms| ms.iter().find(|m| m["id"] == id))
            .cloned()
            .ok_or("followup_monitor_view")
    }
    // Scheduler has no HTTP tick endpoint. Advance only via public Store API;
    // never write SQLite to force due work, child success or publication.
    async fn followup_tick(
        &self,
        at: i64,
        last: &mut i64,
        end: i64,
        ticks: &mut Vec<Value>,
    ) -> EvalResult<u32> {
        if at < *last || at >= end {
            return Err("followup_clock_boundary");
        }
        self.store()
            .enqueue_scheduled_reminders_at(at)
            .await
            .map_err(|_| "followup_enqueue")?;
        let admitted = self
            .store()
            .poll_recurring_missions_at(at)
            .await
            .map_err(|_| "followup_poll")?;
        self.runtime.as_ref().ok_or("followup_runtime")?.wake();
        *last = at;
        ticks.push(json!({"at":at,"admitted":admitted}));
        Ok(admitted)
    }
    async fn followup_inbox(&self) -> EvalResult<Value> {
        self.checked("GET", "/api/v1/notifications", Value::Null, 200)
            .await
    }
    pub(super) async fn execute_followup(&mut self, out: &mut Execution) -> EvalResult<()> {
        *self.stages.lock().unwrap_or_else(|e| e.into_inner()) = stage_plan(Kind::FollowupMonitor);
        let mark = self.begin_stage("read_status_and_propose")?;
        let collection = value_id(
            &self
                .checked(
                    "POST",
                    "/api/v1/knowledge/collections",
                    json!({"name":"Synthetic return updates","scope":"return-status"}),
                    201,
                )
                .await?,
        )?;
        let ingested = self
            .followup_ingest(&collection, 0, fixture::INITIAL)
            .await?;
        let source = ingested["source_ids"]
            .as_array()
            .and_then(|ids| ids.first())
            .and_then(Value::as_str)
            .ok_or("followup_source_id")?
            .to_string();
        out.source = Some(source.clone());
        // A second saved source makes scope expansion independently observable.
        let decoy = self
            .source(
                self.login(),
                "Synthetic unrelated warranty status: closed. Never part of return RMA-104.",
            )
            .await?;
        let started = Instant::now();
        let parent=self.checked("POST","/api/v1/missions",json!({"request_id":uuid::Uuid::new_v4(),"task":fixture::TASK,"constraints":"","checks":[{"kind":"source_quote","id":source,"revision":1,"quote":fixture::INITIAL}],"execution_mode":"durable"}),200).await?;
        self.admitted_timing();
        let admission_ms = ms(started);
        let parent_id = value_id(&parent)?;
        let parent_view = self.mission_settled(&parent_id).await?;
        let parent_observation = self.mission_snapshot(&parent_id)?;
        let proposed = self
            .checked("GET", "/api/v1/task-monitors", Value::Null, 200)
            .await?;
        let proposal = proposed
            .as_array()
            .and_then(|ms| ms.iter().find(|m| m["parent_mission_id"] == parent_id))
            .cloned()
            .ok_or("followup_proposal")?;
        let id = value_id(&proposal)?;
        self.end_stage(
            mark,
            &[
                ("parent_achieved", parent_view["state"] == "achieved"),
                ("real_proposal", proposal["state"] == "proposed"),
            ],
        )?;
        out.observations.push(parent_observation);
        out.timings.push(TurnTiming {
            admission_ms,
            terminal_observed_ms: ms(started),
        });
        let mark = self.begin_stage("accept_and_exact_replay")?;
        let base = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "followup_clock")?
            .as_secs()
            .try_into()
            .map_err(|_| "followup_clock")?;
        let end = base + 3600_i64;
        let mut last = base;
        let mut ticks = vec![];
        let accept = json!({"request_id":uuid::Uuid::new_v4(),"expected_revision":proposal["revision"],"cadence_seconds":60,"expires_at":end,"reuse":true,"read":true,"check":true,"notify":true});
        let accept_url = format!("/api/v1/task-monitors/{id}/accept");
        let active = self
            .checked("POST", &accept_url, accept.clone(), 200)
            .await?;
        let replay = self.checked("POST", &accept_url, accept, 200).await?;
        let accepted = observe::followup::snapshot(self.dir.path(), &id)?;
        self.end_stage(
            mark,
            &[
                ("same_accept_receipt", active == replay),
                ("one_policy", accepted["policy_count"] == 1),
                ("active", active["state"] == "active"),
            ],
        )?;
        let mark = self.begin_stage("unchanged_no_inference")?;
        let unchanged_admitted = self
            .followup_tick(base + 61, &mut last, end, &mut ticks)
            .await?;
        let unchanged = observe::followup::snapshot(self.dir.path(), &id)?;
        self.end_stage(
            mark,
            &[
                (
                    "no_child",
                    unchanged_admitted == 0
                        && unchanged["children"].as_array().is_some_and(Vec::is_empty),
                ),
                (
                    "unchanged_observed",
                    unchanged["unchanged_occurrences"]
                        .as_i64()
                        .is_some_and(|n| n >= 1),
                ),
            ],
        )?;
        let mark = self.begin_stage("changed_source_one_child")?;
        let changed_ingest = self
            .followup_ingest(&collection, 1, fixture::CHANGED)
            .await?;
        let changed_admitted = self
            .followup_tick(base + 121, &mut last, end, &mut ticks)
            .await?;
        let view = self.monitor_view(&id).await?;
        let child_id = view["observations"]
            .as_array()
            .and_then(|os| os.first())
            .and_then(|o| o["mission_id"].as_str())
            .ok_or("followup_child")?
            .to_string();

        self.admitted_timing();
        let child_view = self.mission_settled(&child_id).await?;
        let mut child_observation = self.mission_snapshot(&child_id)?;
        // Parent dispatches are not child receipts. Snapshot stores run-specific
        // receipts; the ledger split uses the known completed parent boundary.
        let parent_calls = out.observations[0].ledger.len();
        child_observation.ledger = self
            .provider
            .ledger()
            .into_iter()
            .skip(parent_calls)
            .collect();

        self.followup_tick(base + 121, &mut last, end, &mut ticks)
            .await?;
        let published = observe::followup::snapshot(self.dir.path(), &id)?;
        let inbox = self.followup_inbox().await?;
        self.end_stage(
            mark,
            &[
                (
                    "one_changed_child",
                    changed_admitted == 1
                        && published["children"]
                            .as_array()
                            .is_some_and(|cs| cs.len() == 1),
                ),
                ("child_achieved", child_view["state"] == "achieved"),
                (
                    "notification_published",
                    published["notifications"]
                        .as_array()
                        .is_some_and(|ns| ns.len() == 1),
                ),
            ],
        )?;
        out.observations.push(child_observation);
        let mark = self.begin_stage("unchanged_no_duplicate")?;
        let duplicate_admitted = self
            .followup_tick(base + 181, &mut last, end, &mut ticks)
            .await?;
        let duplicate = observe::followup::snapshot(self.dir.path(), &id)?;
        self.end_stage(
            mark,
            &[
                (
                    "no_duplicate_child",
                    duplicate_admitted == 0 && duplicate["children"] == published["children"],
                ),
                (
                    "no_duplicate_notification",
                    duplicate["notifications"] == published["notifications"],
                ),
            ],
        )?;
        let mark = self.begin_stage("owner_stop_and_replay")?;
        let control = json!({"expected_revision":active["revision"],"revoke_policy":true});
        let stop_url = format!("/api/v1/task-monitors/{id}/stop");
        let stopped = self
            .checked("POST", &stop_url, control.clone(), 200)
            .await?;
        let stopped_replay = self.checked("POST", &stop_url, control, 200).await?;
        let stopped_inbox = self.followup_inbox().await?;
        self.end_stage(
            mark,
            &[
                ("same_stop_receipt", stopped == stopped_replay),
                ("stopped", stopped["state"] == "stopped"),
                (
                    "inbox_hidden",
                    stopped_inbox["items"].as_array().is_some_and(Vec::is_empty),
                ),
            ],
        )?;
        let mark = self.begin_stage("stopped_reopen")?;
        self.restart().await?;
        let stopped_admitted = self
            .followup_tick(base + 241, &mut last, end, &mut ticks)
            .await?;
        let reopened = observe::followup::snapshot(self.dir.path(), &id)?;
        let reopened_inbox = self.followup_inbox().await?;
        let reopened_view = self.monitor_view(&id).await?;
        self.end_stage(
            mark,
            &[
                (
                    "no_stopped_child",
                    stopped_admitted == 0 && reopened["children"] == published["children"],
                ),
                (
                    "inbox_still_hidden",
                    reopened_inbox["items"]
                        .as_array()
                        .is_some_and(Vec::is_empty),
                ),
                ("policy_revoked", reopened["policy"]["state"] == "revoked"),
            ],
        )?;
        out.outcome = json!({"clock":{"method":fixture::FIXTURE.scheduler_method,"base":base,"expires_at":end,"ticks":ticks},"source":source,"decoy":decoy,"changed_ingest":changed_ingest,"parent":parent_view,"proposal":proposal,"active":active,"accept_replay":replay,"accepted":accepted,"unchanged":unchanged,"published":published,"inbox":inbox,"duplicate":duplicate,"stopped":stopped,"stop_replay":stopped_replay,"stopped_inbox":stopped_inbox,"reopened":reopened,"reopened_inbox":reopened_inbox,"reopened_view":reopened_view});
        Ok(())
    }
}
