//! Adopt two foreground proofs, reuse a fresh task, then withdraw an actual dependency.
use super::*;
pub(super) fn template(task: &str, expected: &str) -> MissionCreate {
    MissionCreate {
        request_id: Uuid::new_v4().to_string(),
        task: task.into(),
        constraints: String::new(),
        checks: vec![MissionCheck::DecimalEquals {
            expected: expected.into(),
        }],
        execution_mode: MissionMode::Durable,
    }
}
pub(super) async fn reuse(
    s: &Store,
    c: &AgentClaim,
    pin: SkillPin,
    arguments: LearnedSkillArguments,
) -> (StepLease, KnowledgeToolResult) {
    let context = s.agent_context(&c.lease).await.unwrap();
    let step = s
        .begin_agent_step(&c.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        capsule("current mission recipe invocation"),
        vec![PendingKnowledgeCall {
            call_id: "fresh-reuse".into(),
            input: KnowledgeToolInput::LearnedSkill {
                invocation: LearnedSkillInvocation { pin, arguments },
            },
        }],
    )
    .await
    .unwrap();
    let accepted = acceptance_revision(s, c).await;
    s.mission_save_plan_at(
        &step,
        accepted,
        vec!["Use the learned local procedure with this task's fresh inputs".into()],
    )
    .await
    .unwrap();
    let result = s
        .execute_knowledge_tool(&step, "fresh-reuse".into())
        .await
        .unwrap();
    let again = s
        .execute_knowledge_tool(&step, "fresh-reuse".into())
        .await
        .unwrap();
    assert_eq!(encoded(&result).unwrap(), encoded(&again).unwrap());
    (step, result)
}
async fn acceptance_revision(s: &Store, c: &AgentClaim) -> i64 {
    let view = s.mission_execution(&c.lease).await.unwrap().unwrap().view;
    view.acceptance_history
        .as_ref()
        .and_then(|h| h.last())
        .map_or(1, |v| v.revision)
}
pub(super) async fn verify(s: &Store, c: &AgentClaim, step: &StepLease, text: &str) {
    let context = s.agent_context(&c.lease).await.unwrap();
    let final_step = s
        .begin_agent_step(&c.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(&final_step, capsule("final mission receipt"), vec![])
        .await
        .unwrap();
    let accepted = acceptance_revision(s, c).await;
    assert!(
        s.mission_verify_at(
            &final_step,
            accepted,
            MissionCandidate {
                text: text.into(),
                evidence: vec![MissionEvidence {
                    step: step.step(),
                    call_id: "fresh-reuse".into()
                }]
            }
        )
        .await
        .unwrap()
    );
}
#[tokio::test]
async fn mission_learned_fresh_reuse_and_recurring_dependency_pause_resume() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    learned_math_example(&s, &u, &["1", "2"], Some("3")).await;
    learned_math_example(&s, &u, &["4", "5"], Some("9")).await;
    let card = s.learned_skills(&u).await.unwrap().remove(0);
    let pin = SkillPin {
        id: card.id.clone(),
        revision: card.version.clone(),
        digest: card.digest.clone(),
    };
    let m = s
        .create_mission(&u, template("Use my learned sum for 10.25 and 0.75", "11"))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let (step, value) = reuse(
        &s,
        &c,
        pin.clone(),
        LearnedSkillArguments::Decimal {
            operands: vec!["10.25".into(), "0.75".into()],
        },
    )
    .await;
    assert!(matches!(value,KnowledgeToolResult::Decimal{exact} if exact=="11"));
    verify(&s, &c, &step, "Exact task result 11").await;
    assert_eq!(
        s.missions(&u)
            .await
            .unwrap()
            .iter()
            .find(|v| v.id == m.id)
            .unwrap()
            .state,
        "achieved"
    );
    let run = c.lease.run_id().to_owned();
    s.call(move |db| {
        let count: i64 = db.conn.query_row(
            "SELECT tool_calls FROM agent_execution WHERE run_id=?1",
            [run],
            |row| row.get(0),
        )?;
        assert_eq!(count, 1);
        Ok(())
    })
    .await
    .unwrap();
    let at = now().unwrap() + 60;
    let saved = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: template("Use the learned sum for 7 and 8", "15"),
                schedule: ScheduleDraft {
                    title: "Learned recurring total".into(),
                    message: "Synthetic".into(),
                    rule: ScheduleRule::Interval {
                        anchor: at,
                        seconds: 60,
                    },
                    valid_until: None,
                },
                changes_only: true,
            },
        )
        .await
        .unwrap();
    let unrelated = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: template("Compute 3 and 4", "7"),
                schedule: ScheduleDraft {
                    title: "Unrelated task".into(),
                    message: "Synthetic".into(),
                    rule: ScheduleRule::Interval {
                        anchor: at + 30,
                        seconds: 60,
                    },
                    valid_until: None,
                },
                changes_only: true,
            },
        )
        .await
        .unwrap();
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 1);
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let (step, value) = reuse(
        &s,
        &c,
        pin,
        LearnedSkillArguments::Decimal {
            operands: vec!["7".into(), "8".into()],
        },
    )
    .await;
    assert!(matches!(value,KnowledgeToolResult::Decimal{exact} if exact=="15"));
    // The direct execution itself records dependency; no prior skill_read is required.
    let revision = s.learned_skills(&u).await.unwrap().remove(0).revision;
    s.mutate_learned_skill(
        &u,
        card.id.clone(),
        CommunitySkillMutation {
            action: "disable".into(),
            expected_revision: revision,
            expected_digest: None,
        },
    )
    .await
    .unwrap();
    assert!(
        s.execute_knowledge_tool(&step, "fresh-reuse".into())
            .await
            .is_err()
    );
    let states = s.recurring_missions(&u).await.unwrap();
    let paused = states
        .iter()
        .find(|v| v.schedule.id == saved.schedule.id)
        .unwrap();
    assert_eq!(paused.schedule.state, "paused");
    assert_eq!(paused.schedule.reason.as_deref(), Some("needs_skill"));
    assert_eq!(
        paused.missing_skills.as_ref().unwrap(),
        &vec!["학습한 개인 절차".to_owned()]
    );
    assert_eq!(
        states
            .iter()
            .find(|v| v.schedule.id == unrelated.schedule.id)
            .unwrap()
            .schedule
            .state,
        "active"
    );
    let rev = paused.schedule.revision;
    let revision = s.learned_skills(&u).await.unwrap().remove(0).revision;
    s.mutate_learned_skill(
        &u,
        card.id.clone(),
        CommunitySkillMutation {
            action: "enable".into(),
            expected_revision: revision,
            expected_digest: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        s.recurring_missions(&u)
            .await
            .unwrap()
            .iter()
            .find(|v| v.schedule.id == saved.schedule.id)
            .unwrap()
            .schedule
            .state,
        "paused"
    );
    s.control_recurring_mission(
        &u,
        saved.schedule.id,
        ScheduleControl {
            expected_revision: rev,
            action: "resume".into(),
        },
    )
    .await
    .unwrap();
    // Instruction consumption alone also fences a one-off task; the model can
    // otherwise follow that recipe with ordinary native tools.
    let instruction = s
        .create_mission(&u, template("Follow my learned sum for 6 and 7", "13"))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let instruction_pin = SkillPin {
        id: card.id.clone(),
        revision: card.version,
        digest: card.digest,
    };
    let step = tool_step(&s, &c, skill_call("instructions", instruction_pin)).await;
    assert!(matches!(
        s.execute_knowledge_tool(&step, "instructions".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Skill(_)
    ));
    let revision = s.learned_skills(&u).await.unwrap().remove(0).revision;
    s.mutate_learned_skill(
        &u,
        card.id,
        CommunitySkillMutation {
            action: "disable".into(),
            expected_revision: revision,
            expected_digest: None,
        },
    )
    .await
    .unwrap();
    assert!(s.agent_context(&c.lease).await.is_err());
    assert!(
        s.append_agent_output(
            &step,
            patch("Must not publish after instruction withdrawal")
        )
        .await
        .is_err()
    );
    let withdrawn = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == instruction.id)
        .unwrap();
    assert_eq!(withdrawn.state, "quarantined");
    assert!(withdrawn.result.is_none());
}
