//! Coherent user workflows. Oracle reads stay evaluator-only.
use super::*;
use rusqlite::params;

impl World {
    async fn record_turn(&self, out: &mut Execution, text: &str) -> EvalResult<()> {
        let (observed, timing, _) = self.turn(text, false).await?;
        out.observations.push(observed);
        out.timings.push(timing);
        Ok(())
    }
    fn learned_state(&self) -> EvalResult<Value> {
        let conn = observe::open(self.dir.path())?;
        let account = &self.login().user.account().id;
        let cards = conn.prepare("SELECT id,state,route FROM learned_skills WHERE account_id=?1 ORDER BY id").map_err(|_|"observer_learned")?.query_map([account],|r|Ok(json!({"id":r.get::<_,String>(0)?,"state":r.get::<_,String>(1)?,"route":r.get::<_,String>(2)?}))).map_err(|_|"observer_learned")?.collect::<Result<Vec<_>,_>>().map_err(|_|"observer_learned")?;
        let proofs = conn.prepare("SELECT p.skill_id,p.input_digest,p.run_id,p.receipt_digest,r.account_id,r.state,a.invalidated FROM learned_skill_proofs p JOIN chat_runs r ON r.tenant_id=p.tenant_id AND r.id=p.run_id JOIN agent_execution a ON a.tenant_id=r.tenant_id AND a.run_id=r.id WHERE p.account_id=?1 ORDER BY p.run_id").map_err(|_|"observer_proofs")?.query_map([account],|r|Ok(json!({"skill_id":r.get::<_,String>(0)?,"input_digest":r.get::<_,String>(1)?,"run_id":r.get::<_,String>(2)?,"receipt_digest":r.get::<_,String>(3)?,"account":r.get::<_,String>(4)?,"state":r.get::<_,String>(5)?,"invalidated":r.get::<_,bool>(6)?}))).map_err(|_|"observer_proofs")?.collect::<Result<Vec<_>,_>>().map_err(|_|"observer_proofs")?;
        let uses = conn.prepare("SELECT run_id,skill_id FROM learned_skill_uses WHERE tenant_id=(SELECT tenant_id FROM accounts WHERE id=?1)").map_err(|_|"observer_uses")?.query_map([account],|r|Ok(json!({"run_id":r.get::<_,String>(0)?,"skill_id":r.get::<_,String>(1)?}))).map_err(|_|"observer_uses")?.collect::<Result<Vec<_>,_>>().map_err(|_|"observer_uses")?;
        Ok(json!({"cards":cards,"proofs":proofs,"uses":uses}))
    }
    pub(super) async fn execute_outcome(
        &mut self,
        kind: Kind,
        out: &mut Execution,
    ) -> EvalResult<()> {
        match kind {
            Kind::ChatMemory => {
                self.record_turn(out, &format!("Remember: {}", world::PREFERENCE_A))
                    .await?;
                self.record_turn(out, "What is my preference?").await?;
                self.record_turn(
                    out,
                    &format!("Correct my preference: {}", world::PREFERENCE_B),
                )
                .await?;
                self.record_turn(out, "What is my preference?").await?;
                self.restart().await?;
                self.record_turn(out, "What is my preference?").await?;
            }
            Kind::TransitiveForget => {
                let source = self
                    .source(
                        self.login(),
                        "UNRELATED_SOURCE independent record: My preference is synthetic amber.",
                    )
                    .await?;
                out.source = Some(source.clone());
                self.record_turn(out, "Read the unrelated source.").await?;
                self.record_turn(out, &format!("Remember: {}", world::PREFERENCE_A))
                    .await?;
                self.record_turn(out, "What is my preference? history-chain-anchor")
                    .await?;
                self.record_turn(
                    out,
                    "Find the earlier assistant preference through owned conversation history.",
                )
                .await?;
                let memory = out.observations[1]
                    .command
                    .as_ref()
                    .and_then(|c| c["outcome"]["memory_id"].as_str())
                    .ok_or("command_memory_id")?
                    .to_owned();
                self.record_turn(out, "Forget my preference.").await?;
                self.restart().await?;
                self.record_turn(out, "What is my preference?").await?;
                self.record_turn(
                    out,
                    "Find the earlier assistant preference through owned conversation history.",
                )
                .await?;
                let conn = observe::open(self.dir.path())?;
                let (status,versions,index): (String,i64,i64) = conn.query_row("SELECT status,(SELECT count(*) FROM knowledge_memory_versions WHERE memory_id=?1),(SELECT count(*) FROM knowledge_fts WHERE object_id=?1) FROM knowledge_memories WHERE id=?1",[&memory],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|_|"observer_forget")?;
                let mut purged = vec![];
                for o in &out.observations[1..4] {
                    let row = conn.query_row("SELECT (SELECT count(*) FROM chat_output WHERE run_id=?1),(SELECT count(*) FROM agent_tool_slots WHERE run_id=?1),(SELECT count(*) FROM agent_steps WHERE run_id=?1 AND checkpoint IS NOT NULL),(SELECT count(*) FROM chat_events WHERE thread_id=(SELECT thread_id FROM chat_runs WHERE id=?1) AND change LIKE '%synthetic amber%')",[&o.run_id],|r|Ok(json!({"output":r.get::<_,i64>(0)?,"slots":r.get::<_,i64>(1)?,"checkpoints":r.get::<_,i64>(2)?,"events_with_canary":r.get::<_,i64>(3)?}))).map_err(|_|"observer_transitive_purge")?;
                    purged.push(row);
                }
                let independent = conn.query_row("SELECT v.text,(SELECT count(*) FROM knowledge_fts WHERE object_id=s.id),(SELECT group_concat(text,'') FROM chat_output WHERE run_id=?2) FROM knowledge_sources s JOIN knowledge_source_versions v ON v.tenant_id=s.tenant_id AND v.source_id=s.id AND v.revision=s.revision WHERE s.id=?1 AND s.deleted=0",params![source,out.observations[0].run_id],|r|Ok(json!({"text":r.get::<_,String>(0)?,"index":r.get::<_,i64>(1)?,"output":r.get::<_,String>(2)?}))).map_err(|_|"observer_unrelated_source")?;
                out.outcome = json!({"status":status,"versions":versions,"index":index,"purged":purged,"independent":independent});
            }
            Kind::CancelMission => {
                let source = self.source(self.login(), world::SOURCE).await?;
                out.source = Some(source.clone());
                let started = Instant::now();
                let admitted=self.checked("POST","/api/v1/missions",json!({"request_id":uuid::Uuid::new_v4(),"task":"Calculate the saved quote amount plus shipping in USD. Do not purchase anything.","constraints":"","checks":[{"kind":"decimal_equals","expected":"24.75"},{"kind":"source_quote","id":source,"revision":1,"quote":world::SOURCE}],"execution_mode":"durable"}),200).await?;
                self.admitted_timing();
                let admission_ms = ms(started);
                let id = value_id(&admitted)?;
                tokio::time::timeout(Duration::from_secs(8), async {
                    while self.provider.ledger().is_empty() {
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                })
                .await
                .map_err(|_| "cancel_dispatch_barrier_timeout")?;
                let current = self
                    .checked("GET", "/api/v1/missions", Value::Null, 200)
                    .await?;
                let revision = current
                    .as_array()
                    .and_then(|ms| ms.iter().find(|m| m["id"] == id))
                    .ok_or("cancel_mission_view")?["revision"]
                    .clone();
                let control = json!({"request_id":uuid::Uuid::new_v4(),"expected_revision":revision,"action":{"kind":"cancel"}});
                let path = format!("/api/v1/missions/{id}/control");
                let cancelled = self.checked("POST", &path, control.clone(), 200).await?;
                let replay = self.checked("POST", &path, control, 200).await?;
                self.provider.release();
                self.restart().await?;
                let conn = observe::open(self.dir.path())?;
                let (run,grant,uncertain,controls):(String,bool,bool,i64)=conn.query_row("SELECT run_id,grant_enabled,uncertain,(SELECT count(*) FROM mission_controls WHERE mission_id=?1) FROM missions WHERE id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|_|"observer_cancel")?;
                let observation = observe::snapshot(self.dir.path(), &run, self.provider.ledger())?;
                let usage = self.usage_totals();
                out.outcome = json!({"grant":grant,"uncertain":uncertain,"controls":controls,"replay_same_revision":cancelled["revision"]==replay["revision"],"usage_preserved":usage.is_some()});
                out.observations.push(observation);
                out.timings.push(TurnTiming {
                    admission_ms,
                    terminal_observed_ms: ms(started),
                });
            }
            Kind::LearnedWorkflow => {
                let mut stages = vec![];
                for (label, inputs) in [
                    ("A", json!(["12.50", "2", "4", "0.8"])),
                    ("B", json!(["7.25", "4", "6", "0.6"])),
                    ("C", json!(["9.50", "3", "5", "0.9"])),
                ] {
                    self.record_turn(out,&format!("Workflow {label} calculate (first times second plus third) times fourth. inputs={inputs}")).await?;
                    stages.push(self.learned_state()?);
                }
                let conn = observe::open(self.dir.path())?;
                let thread: String = conn
                    .query_row(
                        "SELECT thread_id FROM chat_runs WHERE id=?1",
                        [&out.observations[1].run_id],
                        |r| r.get(0),
                    )
                    .map_err(|_| "observer_proof_thread")?;
                drop(conn);
                self.checked(
                    "DELETE",
                    &format!("/api/v1/threads/{thread}"),
                    Value::Null,
                    204,
                )
                .await?;
                self.restart().await?;
                let withdrawn = self.learned_state()?;
                let c = observe::snapshot(self.dir.path(), &out.observations[2].run_id, vec![])?;
                self.record_turn(out,"Workflow D calculate (first times second plus third) times fourth. inputs=[\"3\",\"8\",\"7\",\"0.5\"]").await?;
                out.outcome = json!({"stages":stages,"withdrawn":withdrawn,"c_output_after_withdrawal":c.output,"c_state_after_withdrawal":c.state});
            }
            _ => return Err("outcome_kind"),
        }
        Ok(())
    }
}
