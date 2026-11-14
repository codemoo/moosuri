use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use crate::{PendingKnowledgeCall, PrivateCheckpoint};
use moosuri_core::{MessagePhase, ModelSelection, ProviderConfig, ProviderKind};
async fn configured(s: &Store) -> UserContext {
    let u = admin(s).await.user;
    let p = s
        .create_provider(
            &u,
            crate::ProviderChange {
                name: "Synthetic numeric timing".into(),
                enabled: true,
                credential: crate::CredentialChange::Clear,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
            },
        )
        .await
        .unwrap();
    s.record_model_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.set_default_model(
        &u,
        Some(ModelSelection {
            source: Default::default(),
            provider_id: p.id,
            provider_revision: 1,
            model: "synthetic".into(),
            effort: None,
        }),
    )
    .await
    .unwrap();
    u
}
async fn submit(s: &Store, u: &UserContext) -> (String, String) {
    let thread = s
        .create_thread(u, "Synthetic latency".into())
        .await
        .unwrap()
        .id;
    let turn = s
        .submit_turn(
            u,
            thread.clone(),
            Uuid::new_v4().to_string(),
            "PRIVATE_TIMING_CONTENT_CANARY".into(),
        )
        .await
        .unwrap();
    (thread, turn.run.id)
}
fn patch(item: &str, text: &str, phase: Option<MessagePhase>) -> crate::OutputPatch {
    crate::OutputPatch::Append {
        item: item.into(),
        text: text.into(),
        phase,
        refusal: false,
    }
}
#[tokio::test]
async fn run_timings_retries_reopen_absence_outcomes_and_owner_aggregates_are_honest() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    let other = member(&s, &u, "timing-other@example.test").await.user;
    let (thread, _) = submit(&s, &u).await;
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let step = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: b"{}".to_vec(),
        },
        Vec::<PendingKnowledgeCall>::new(),
    )
    .await
    .unwrap();
    s.append_agent_output(
        &step,
        vec![patch(
            "commentary",
            "Checking",
            Some(MessagePhase::Commentary),
        )],
    )
    .await
    .unwrap();
    let original = timing_mark();
    s.append_agent_output_timed(
        &step,
        vec![patch("answer", "Actual final body", None)],
        vec![("answer".into(), original)],
    )
    .await
    .unwrap();
    s.append_agent_output(
        &step,
        vec![crate::OutputPatch::Phase {
            item: "answer".into(),
            phase: Some(MessagePhase::FinalAnswer),
        }],
    )
    .await
    .unwrap();
    s.record_model_duration(&step, 7).await.unwrap();
    s.finish_agent_run(&step, RunState::Completed, None)
        .await
        .unwrap();
    let done = s
        .conversation(&u, thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    let observed = done.run.timings.unwrap();
    assert!(
        observed.queue_ms.is_some()
            && observed.context_ms.is_some()
            && observed.first_dispatch_ms.is_some()
    );
    assert_eq!(observed.model_ms, Some(7));
    assert_eq!(observed.measured_model_calls, 1);
    assert!(observed.first_final_body_ms.unwrap() <= observed.terminal_ms.unwrap());
    let (cancel_thread, cancel_id) = submit(&s, &u).await;
    s.cancel_run(&u, cancel_id.clone(), 1).await.unwrap();
    s.retry_run(&u, cancel_id.clone(), 1).await.unwrap();
    s.cancel_run(&u, cancel_id, 2).await.unwrap();
    let retried = s
        .conversation(&u, cancel_thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(retried.run.timings.unwrap().attempt, 2);
    let (restart_thread, _) = submit(&s, &u).await;
    s.claim_agent_run().await.unwrap().unwrap();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    let after = s
        .conversation(&u, restart_thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(after.run.state, RunState::OutcomeUnknown);
    let t = after.run.timings.unwrap();
    assert!(t.clock_discontinuity);
    assert_eq!(t.terminal_ms, None);
    assert_eq!(t.first_final_body_ms, None);
    let aggregate = s.usage_summary(&u).await.unwrap().timings.unwrap();
    assert_eq!(aggregate.attempts, 4);
    assert_eq!(aggregate.outcomes.get("cancelled"), Some(&2));
    assert_eq!(aggregate.outcomes.get("outcome_unknown"), Some(&1));
    assert_eq!(aggregate.first_final_body.samples, 1);
    assert_eq!(aggregate.first_final_body.missing, 3);
    assert!(
        !serde_json::to_string(&aggregate)
            .unwrap()
            .contains("PRIVATE_TIMING_CONTENT_CANARY")
    );
    let empty = s.usage_summary(&other).await.unwrap().timings.unwrap();
    assert_eq!(empty.runs, 0);
    assert_eq!(empty.terminal.p50_ms, None);
    let accounts = s.account_usage(&u).await.unwrap();
    assert_eq!(
        accounts
            .accounts
            .iter()
            .find(|a| a.account_id == other.account.id)
            .unwrap()
            .usage
            .timings
            .as_ref()
            .unwrap()
            .attempts,
        0
    );
    // A historical row with no extension stays absent; never infer old timings from created_at.
    let tenant = u.tenant.0.clone();
    s.call(move|db|{db.conn.execute("UPDATE chat_runs SET model=json_remove(model,'$.run_observations_v1') WHERE tenant_id=?1",[tenant])?;Ok(())}).await.unwrap();
    let legacy = s.usage_summary(&u).await.unwrap().timings.unwrap();
    assert_eq!(legacy.unmeasured_runs, 3);
    assert_eq!(legacy.first_final_body.samples, 0);
    assert_eq!(legacy.first_final_body.p50_ms, None);
}
