use super::*;
use crate::tests::TEST_LOCK;
use moosuri_core::*;
use moosuri_credential_broker::{auth::SigningKey, google::GoogleIdentity, policy::LocalGrant};
use moosuri_vault_protocol::broker::Command;
async fn command(s: &Store, d: &Device, k: &SigningKey, command: Command) -> Result<Reply> {
    let challenge = s.broker_auth_challenge(d.id).await?;
    let signature = k.sign(
        &moosuri_credential_broker::auth::authenticated_bytes(
            &challenge,
            "POST",
            DEVICE_PATH,
            &command,
        )
        .unwrap(),
    );
    s.broker_command(SignedCommand {
        challenge: challenge.nonce,
        signature,
        command,
    })
    .await
}
async fn grant(s: &Store, u: &UserContext, bg: bool) -> (Device, SigningKey, LocalGrant) {
    let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let pair = s
        .broker_pair_challenge(u, "https://assistant.example.test".into(), key.public_key())
        .await
        .unwrap();
    let device = s
        .broker_pair(PairProof {
            challenge: pair.nonce,
            signature: key.sign(&pairing_bytes(&pair).unwrap()),
        })
        .await
        .unwrap();
    let grant = LocalGrant::select_google_model_read(
        device.clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        GoogleIdentity {
            subject: "synthetic-native-task-sub".into(),
            email: "private-native@example.test".into(),
        },
        1,
        1,
        now().unwrap() + 3600,
        bg,
    )
    .unwrap();
    command(
        s,
        &device,
        &key,
        Command::RegisterGoogleModelRead {
            binding: grant.binding.clone(),
            background: bg,
        },
    )
    .await
    .unwrap();
    (device, key, grant)
}
async fn step(s: &Store, l: &crate::RunLease, read: MailModelRead, call: &str) -> crate::StepLease {
    let c = s.agent_context(l).await.unwrap();
    let st = s.begin_agent_step(l, c.checkpoint_seq).await.unwrap();
    s.commit_agent_step(
        &st,
        crate::PrivateCheckpoint {
            bytes: br#"{"kind":"synthetic-native-tool-checkpoint"}"#.to_vec(),
        },
        vec![crate::PendingKnowledgeCall {
            call_id: call.into(),
            input: KnowledgeToolInput::NativeMail { read },
        }],
    )
    .await
    .unwrap();
    st
}
fn search() -> MailModelRead {
    MailModelRead::Search {
        query: "newer_than:7d".into(),
        cursor: None,
    }
}
fn result() -> MailModelResult {
    MailModelResult {
        version: 1,
        status: MailModelStatus::Ready,
        messages: vec![MailModelMessage {
            selection: Uuid::new_v4(),
            thread: Uuid::new_v4(),
            observation: Uuid::new_v4(),
            subject: "Synthetic invoice".into(),
            from: "research@example.test".into(),
            text: "Synthetic amount 42".into(),
            partial: false,
            content_digest: [7; 32],
            attachments: vec![],
        }],
        next_cursor: None,
        partial: false,
        attachment: None,
    }
}
async fn finish(
    s: &Store,
    d: &Device,
    k: &SigningKey,
    request: &Request,
    result: MailModelResult,
) -> SignedReceipt {
    let Reply::ModelMailPayload { payload } = command(
        s,
        d,
        k,
        Command::ModelMailPayload {
            invocation: request.invocation.clone(),
            request_digest: moosuri_credential_broker::auth::hash(&request_bytes(request)),
        },
    )
    .await
    .unwrap() else {
        panic!("model payload")
    };
    assert!(payload.valid());
    let Reply::Commitment { commitment } = command(
        s,
        d,
        k,
        Command::CommitCurrent {
            invocation: request.invocation.clone(),
            request_digest: moosuri_credential_broker::auth::hash(&request_bytes(request)),
        },
    )
    .await
    .unwrap() else {
        panic!("commitment")
    };
    let projection = Projection::GoogleMailModelRead {
        result: Box::new(result),
    };
    let receipt = Receipt {
        commitment: *commitment,
        outcome: Outcome::ProjectedSuccess,
        projection_digest: moosuri_credential_broker::auth::projection_digest(projection.clone()),
        projection,
    };
    let signed = SignedReceipt {
        signature: k.sign(&receipt_bytes(&receipt)),
        receipt,
    };
    assert!(matches!(
        command(
            s,
            d,
            k,
            Command::Settle {
                receipt: Box::new(signed.clone())
            }
        )
        .await
        .unwrap(),
        Reply::Settled
    ));
    signed
}
async fn poll(s: &Store, d: &Device, k: &SigningKey) -> Vec<Request> {
    let Reply::Waiting { requests, .. } = command(s, d, k, Command::Poll).await.unwrap() else {
        panic!("poll")
    };
    requests
}
async fn counts(s: &Store, run: &str) -> (i64, i64, i64) {
    let run = run.to_owned();
    s.call(move |db| {
        Ok(db.conn.query_row(
            "SELECT remote_calls,tool_calls,deadline_ms FROM agent_execution WHERE run_id=?1",
            [run],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn native_task_mail_twelfth_tool_preserves_final_answer_capacity_without_resetting_limits() {
    let _lock = TEST_LOCK.lock().await;
    for exhaust_models in [false, true] {
        let (_dir, s, u) = crate::missions::tests::setup().await;
        let (d, k, _) = grant(&s, &u, false).await;
        let thread = s
            .create_thread(&u, "Native budget boundary".into())
            .await
            .unwrap();
        let turn = s
            .submit_turn(
                &u,
                thread.id.clone(),
                Uuid::new_v4().to_string(),
                "Read native Gmail recent invoice after checking the totals".into(),
            )
            .await
            .unwrap();
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        for width in [6, 5] {
            let c = s.agent_context(&claim.lease).await.unwrap();
            let st = s
                .begin_agent_step(&claim.lease, c.checkpoint_seq)
                .await
                .unwrap();
            let calls: Vec<_> = (0..width)
                .map(|i| crate::PendingKnowledgeCall {
                    call_id: format!("sum-{}-{i}", st.step()),
                    input: KnowledgeToolInput::DecimalCalculate {
                        calculation: DecimalCalculation {
                            operation: DecimalOperation::Sum,
                            operands: vec!["0.1".into(), "0.2".into()],
                        },
                    },
                })
                .collect();
            s.commit_agent_step(
                &st,
                crate::PrivateCheckpoint {
                    bytes: br#"{"kind":"synthetic-budget-tools"}"#.to_vec(),
                },
                calls.clone(),
            )
            .await
            .unwrap();
            for call in calls {
                assert!(
                    matches!(s.execute_knowledge_tool(&st, call.call_id).await.unwrap(), KnowledgeToolResult::Decimal { exact } if exact == "0.3")
                );
            }
        }
        if exhaust_models {
            for _ in 0..3 {
                let c = s.agent_context(&claim.lease).await.unwrap();
                let st = s
                    .begin_agent_step(&claim.lease, c.checkpoint_seq)
                    .await
                    .unwrap();
                s.commit_agent_step(
                    &st,
                    crate::PrivateCheckpoint {
                        bytes: br#"{"kind":"synthetic-budget-no-calls"}"#.to_vec(),
                    },
                    vec![],
                )
                .await
                .unwrap();
            }
        }
        let st = step(&s, &claim.lease, search(), "twelfth-native-tool").await;
        assert!(
            s.wait_for_native_mail_tool(&st, "twelfth-native-tool".into())
                .await
                .unwrap()
        );
        let before = counts(&s, claim.lease.run_id()).await;
        assert_eq!(
            (before.0, before.1),
            (if exhaust_models { 6 } else { 3 }, 12)
        );
        let requests = poll(&s, &d, &k).await;
        assert_eq!(requests.len(), 1);
        finish(&s, &d, &k, &requests[0], result()).await;
        s.settle_native_task_mail().await.unwrap();
        let resumed = s.claim_agent_run().await.unwrap();
        if exhaust_models {
            assert!(resumed.is_none());
            let saved = s.conversation(&u, thread.id, None).await.unwrap();
            assert_eq!(saved.turns[0].run.state, RunState::Incomplete);
            assert_eq!(
                saved.turns[0].run.error.as_deref(),
                Some("budget_exhausted")
            );
            continue;
        }
        let resumed =
            resumed.expect("a completed twelfth tool still permits the final model answer");
        assert_eq!(resumed.lease.run_id(), turn.run.id);
        assert_eq!(counts(&s, resumed.lease.run_id()).await, before);
        let c = s.agent_context(&resumed.lease).await.unwrap();
        assert!(c.calls.iter().all(|call| call.result.is_some()));
        let final_step = s
            .begin_agent_step(&resumed.lease, c.checkpoint_seq)
            .await
            .unwrap();
        assert_eq!(final_step.step(), 4);
        let extra = crate::PendingKnowledgeCall {
            call_id: "must-not-be-thirteenth".into(),
            input: KnowledgeToolInput::NativeMail { read: search() },
        };
        assert!(matches!(
            s.commit_agent_step(
                &final_step,
                crate::PrivateCheckpoint {
                    bytes: b"{}".to_vec()
                },
                vec![extra]
            )
            .await,
            Err(StoreError::BudgetExhausted)
        ));
        s.commit_agent_step(
            &final_step,
            crate::PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
        s.append_agent_output(
            &final_step,
            vec![crate::OutputPatch::Append {
                item: "final-answer".into(),
                text: "The stored invoice total is 42.".into(),
                phase: Some(MessagePhase::FinalAnswer),
                refusal: false,
            }],
        )
        .await
        .unwrap();
        s.finish_agent_run(&final_step, RunState::Completed, None)
            .await
            .unwrap();
        let saved = s.conversation(&u, thread.id, None).await.unwrap();
        assert_eq!(saved.turns[0].run.state, RunState::Completed);
        assert_eq!(
            saved.turns[0].assistant[0].text,
            "The stored invoice total is 42."
        );
        let after = counts(&s, resumed.lease.run_id()).await;
        assert_eq!((after.0, after.1, after.2), (4, 12, before.2));
    }
}

#[tokio::test]
async fn native_task_mail_background_signed_result_resumes_actual_mission_after_logout_without_reset()
 {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (d, k, g) = grant(&s, &u, true).await;
    let mut input = crate::missions::tests::input(MissionMode::Durable);
    input.task = "Read native Gmail recent invoice".into();
    let mission = s.create_mission(&u, input).await.unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &claim.lease, search(), "real-provider-call").await;
    assert!(
        s.wait_for_native_mail_tool(&st, "real-provider-call".into())
            .await
            .unwrap()
    );
    let original = counts(&s, claim.lease.run_id()).await;
    assert_eq!((original.0, original.1), (1, 1));
    assert!(s.check_run(&claim.lease).await.is_err());
    assert!(s.claim_agent_run().await.unwrap().is_none());
    s.logout(&u).await.unwrap();
    drop(s);
    let s = Store::open_with_web(
        _dir.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    let requests = poll(&s, &d, &k).await;
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.invocation.run.to_string(), claim.lease.run_id());
    assert_eq!(request.invocation.step, st.step() as u32);
    assert!(request.binding == g.binding);
    let signed = finish(&s, &d, &k, request, result()).await;
    s.settle_native_task_mail().await.unwrap();
    s.settle_native_task_mail().await.unwrap();
    let resumed = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(resumed.lease.run_id(), claim.lease.run_id());
    assert_eq!(counts(&s, resumed.lease.run_id()).await, original);
    let c = s.agent_context(&resumed.lease).await.unwrap();
    let Some(KnowledgeToolResult::External(output)) = &c.calls[0].result else {
        panic!("native result")
    };
    assert_eq!(
        output["native_mail"]["messages"][0]["text"],
        "Synthetic amount 42"
    );
    assert_eq!(
        output["native_mail_provenance"]["run_id"],
        claim.lease.run_id()
    );
    assert_eq!(
        output["native_mail_provenance"]["call_id"],
        "real-provider-call"
    );
    assert!(matches!(
        command(
            &s,
            &d,
            &k,
            Command::Settle {
                receipt: Box::new(signed)
            }
        )
        .await
        .unwrap(),
        Reply::Settled
    ));
    assert_eq!(counts(&s, resumed.lease.run_id()).await, original);
    let next = s
        .begin_agent_step(&resumed.lease, c.checkpoint_seq)
        .await
        .unwrap();
    assert_eq!(next.step(), 2);
    let _ = mission;
}
#[tokio::test]
async fn native_task_mail_foreground_chooses_once_rejects_other_device_and_keeps_same_receipt_slot()
{
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (a, ka, _) = grant(&s, &u, false).await;
    let (b, kb, _) = grant(&s, &u, false).await;
    let thread = s
        .create_thread(&u, "Native foreground".into())
        .await
        .unwrap();
    let turn = s
        .submit_turn(
            &u,
            thread.id.clone(),
            Uuid::new_v4().to_string(),
            "Read native Gmail recent invoice".into(),
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &claim.lease, search(), "native-call").await;
    s.wait_for_native_mail_tool(&st, "native-call".into())
        .await
        .unwrap();
    let page = s.chat_events(&u, thread.id.clone(), 0).await.unwrap();
    assert!(page.events.iter().any(|event|matches!(&event.change,ChatChange::State{run}if run.id==turn.run.id&&run.connection_wait.is_some())));
    let tenant = u.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    let wait = s
        .call(move |db| view(&db.conn, &tenant, &run))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(wait.choices.len(), 2);
    assert!(
        !enc(&wait.choices)
            .unwrap()
            .contains("private-native@example.test")
    );
    let selected = wait
        .choices
        .iter()
        .find(|c| c.device_id == a.id.to_string())
        .unwrap();
    let tenant = u.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    let revision = s
        .call(move |db| {
            Ok(db.conn.query_row(
                "SELECT version FROM chat_runs WHERE tenant_id=?1 AND id=?2",
                params![tenant, run],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap();
    let choice = MissionConnectionSelection {
        request_id: Uuid::new_v4().to_string(),
        wait_request_id: wait.request_id.clone(),
        expected_revision: revision,
        slot_id: SLOT.into(),
        binding_id: selected.binding_id.clone(),
    };
    s.select_native_mail_connection(&u, claim.lease.run_id().into(), choice.clone())
        .await
        .unwrap();
    s.select_native_mail_connection(&u, claim.lease.run_id().into(), choice)
        .await
        .unwrap();
    assert!(poll(&s, &b, &kb).await.is_empty());
    let requests = poll(&s, &a, &ka).await;
    assert_eq!(requests.len(), 1);
    let before = counts(&s, claim.lease.run_id()).await;
    finish(&s, &a, &ka, &requests[0], result()).await;
    s.settle_native_task_mail().await.unwrap();
    let resumed = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(resumed.lease.run_id(), turn.run.id);
    assert!(s.native_mail_continuation(&resumed.lease).await.unwrap());
    let c = s.agent_context(&resumed.lease).await.unwrap();
    let old = s
        .native_mail_step_lease(&resumed.lease, c.calls[0].step)
        .await
        .unwrap();
    assert!(
        !s.wait_for_native_mail_tool(&old, "native-call".into())
            .await
            .unwrap()
    );
    assert_eq!(counts(&s, resumed.lease.run_id()).await, before);
    s.broker_revoke(&u, a.id).await.unwrap();
    assert!(s.check_run(&resumed.lease).await.is_err());
    let tenant = u.tenant.0.clone();
    let run = resumed.lease.run_id().to_owned();
    s.call(move |db| {
        let n: i64 = db.conn.query_row(
            "SELECT count(*) FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, run],
            |r| r.get(0),
        )?;
        assert_eq!(n, 0);
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn native_task_mail_expiry_before_dispatch_and_after_commit_never_restart_inference() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (d, k, _) = grant(&s, &u, false).await;
    for committed in [false, true] {
        let thread = s.create_thread(&u, "Bounded native".into()).await.unwrap();
        s.submit_turn(
            &u,
            thread.id,
            Uuid::new_v4().to_string(),
            "Read native Gmail invoice".into(),
        )
        .await
        .unwrap();
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        let st = step(&s, &claim.lease, search(), "bounded").await;
        s.wait_for_native_mail_tool(&st, "bounded".into())
            .await
            .unwrap();
        if committed {
            let requests = poll(&s, &d, &k).await;
            assert_eq!(requests.len(), 1);
            command(
                &s,
                &d,
                &k,
                Command::CommitCurrent {
                    invocation: requests[0].invocation.clone(),
                    request_digest: moosuri_credential_broker::auth::hash(&request_bytes(
                        &requests[0],
                    )),
                },
            )
            .await
            .unwrap();
        }
        let original = counts(&s, claim.lease.run_id()).await;
        let run = claim.lease.run_id().to_owned();
        let tenant = u.tenant.0.clone();
        s.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute(
                "UPDATE native_task_mail_reads SET expires_ms=?3 WHERE tenant_id=?1 AND run_id=?2",
                params![tenant, run, millis()? - 1],
            )?;
            settle(&tx, millis()?)?;
            let state: String = tx.query_row(
                "SELECT state FROM chat_runs WHERE tenant_id=?1 AND id=?2",
                params![tenant, run],
                |r| r.get(0),
            )?;
            assert_eq!(
                state,
                if committed {
                    "outcome_unknown"
                } else {
                    "incomplete"
                }
            );
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(s.claim_agent_run().await.unwrap().is_none());
        assert_eq!(counts(&s, claim.lease.run_id()).await, original);
    }
}
#[tokio::test]
async fn native_task_mail_legacy_selectors_and_old_profiles_cannot_acquire_model_read_authority() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (d, k, _) = grant(&s, &u, false).await;
    let legacy = LocalGrant::select_google_body(
        d.clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        GoogleIdentity {
            subject: "display-sub".into(),
            email: "display@example.test".into(),
        },
        1,
        1,
        now().unwrap() + 600,
    )
    .unwrap();
    command(
        &s,
        &d,
        &k,
        Command::RegisterGoogleBody {
            binding: legacy.binding,
        },
    )
    .await
    .unwrap();
    let thread = s
        .create_thread(&u, "Legacy selector exclusion".into())
        .await
        .unwrap();
    s.submit_turn(
        &u,
        thread.id,
        Uuid::new_v4().to_string(),
        "Read native Gmail message".into(),
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(
        &s,
        &claim.lease,
        MailModelRead::Message {
            selection: Uuid::new_v4(),
        },
        "legacy-untrusted-selection",
    )
    .await;
    s.wait_for_native_mail_tool(&st, "legacy-untrusted-selection".into())
        .await
        .unwrap();
    s.settle_native_task_mail().await.unwrap();
    assert!(poll(&s, &d, &k).await.is_empty());
    let resumed = s.claim_agent_run().await.unwrap().unwrap();
    let context = s.agent_context(&resumed.lease).await.unwrap();
    assert!(matches!(
        context.calls[0].result,
        Some(KnowledgeToolResult::Error(
            crate::ToolReadError::Unavailable
        ))
    ));
    s.finish_agent_execution(
        &resumed.lease,
        RunState::Cancelled,
        Some(crate::RunFailure::Interrupted),
    )
    .await
    .unwrap();
    let thread = s.create_thread(&u, "Old profile".into()).await.unwrap();
    let turn = s
        .submit_turn(
            &u,
            thread.id,
            Uuid::new_v4().to_string(),
            "Read native Gmail".into(),
        )
        .await
        .unwrap();
    let id = turn.run.id.clone();
    s.call(move|db|{db.conn.execute("UPDATE chat_runs SET model=json_remove(json_set(model,'$.agent_profile.native_mail_model_read',json('false')),'$.agent_profile.native_mail_scope') WHERE id=?1",[id])?;Ok(())}).await.unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &claim.lease, search(), "old-profile-call").await;
    assert!(matches!(
        s.wait_for_native_mail_tool(&st, "old-profile-call".into())
            .await,
        Err(StoreError::Forbidden)
    ));
}
#[tokio::test]
async fn native_task_mail_search_then_body_uses_two_real_slots_and_quarantine_withdraws_provenance()
{
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (d, k, _) = grant(&s, &u, false).await;
    let thread = s
        .create_thread(&u, "Native source provenance".into())
        .await
        .unwrap();
    s.submit_turn(
        &u,
        thread.id,
        Uuid::new_v4().to_string(),
        "Read native Gmail invoice body".into(),
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let first = step(&s, &claim.lease, search(), "search-call").await;
    s.wait_for_native_mail_tool(&first, "search-call".into())
        .await
        .unwrap();
    let requests = poll(&s, &d, &k).await;
    let source = result();
    let selected = source.messages[0].selection;
    finish(&s, &d, &k, &requests[0], source.clone()).await;
    s.settle_native_task_mail().await.unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let second = step(
        &s,
        &claim.lease,
        MailModelRead::Message {
            selection: selected,
        },
        "body-call",
    )
    .await;
    s.wait_for_native_mail_tool(&second, "body-call".into())
        .await
        .unwrap();
    let requests = poll(&s, &d, &k).await;
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].invocation.step, 2);
    assert_eq!(requests[0].invocation.run.to_string(), claim.lease.run_id());
    finish(&s, &d, &k, &requests[0], source).await;
    s.settle_native_task_mail().await.unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let tenant = u.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    s.call(move |db| {
        let r = crate::chat::load_run(&db.conn, &tenant, &run)?;
        assert!(!report_receipt(&db.conn, &r, 1, "search-call")?);
        assert!(report_receipt(&db.conn, &r, 2, "body-call")?);
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        crate::credential_broker::quarantine(&tx)?;
        assert_eq!(
            tx.query_row(
                "SELECT count(*) FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2",
                params![tenant, run],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        assert_eq!(
            tx.query_row(
                "SELECT count(*) FROM credential_model_mail_grants",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        assert!(!report_receipt(&tx, &r, 2, "body-call")?);
        tx.commit()?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.check_run(&claim.lease).await.is_err());
}
#[tokio::test]
async fn native_task_mail_signed_unknown_keeps_original_commitment_and_does_not_reenter_model() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (d, k, _) = grant(&s, &u, false).await;
    let thread = s.create_thread(&u, "Unknown read".into()).await.unwrap();
    let turn = s
        .submit_turn(
            &u,
            thread.id,
            Uuid::new_v4().to_string(),
            "Read native Gmail invoice".into(),
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = step(&s, &claim.lease, search(), "unknown-call").await;
    s.wait_for_native_mail_tool(&step, "unknown-call".into())
        .await
        .unwrap();
    let requests = poll(&s, &d, &k).await;
    let request = &requests[0];
    let Reply::Commitment { commitment } = command(
        &s,
        &d,
        &k,
        Command::CommitCurrent {
            invocation: request.invocation.clone(),
            request_digest: moosuri_credential_broker::auth::hash(&request_bytes(request)),
        },
    )
    .await
    .unwrap() else {
        panic!("commit")
    };
    let receipt = Receipt {
        commitment: *commitment,
        outcome: Outcome::Unknown,
        projection: Projection::None,
        projection_digest: moosuri_credential_broker::auth::projection_digest(Projection::None),
    };
    let signed = SignedReceipt {
        signature: k.sign(&receipt_bytes(&receipt)),
        receipt,
    };
    let before = counts(&s, &turn.run.id).await;
    command(
        &s,
        &d,
        &k,
        Command::Settle {
            receipt: Box::new(signed),
        },
    )
    .await
    .unwrap();
    s.settle_native_task_mail().await.unwrap();
    s.settle_native_task_mail().await.unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
    assert_eq!(counts(&s, &turn.run.id).await, before);
    let id = turn.run.id;
    let tenant = u.tenant.0.clone();
    s.call(move|db|{let state:String=db.conn.query_row("SELECT state FROM chat_runs WHERE tenant_id=?1 AND id=?2",params![tenant,id],|r|r.get(0))?;assert_eq!(state,"outcome_unknown");let status:String=db.conn.query_row("SELECT status FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2",params![tenant,id],|r|r.get(0))?;assert_eq!(status,"unknown");let receipt:bool=db.conn.query_row("SELECT receipt IS NOT NULL FROM credential_broker_commitments WHERE tenant_id=?1 AND run_id=?2",params![tenant,id],|r|r.get(0))?;assert!(receipt);Ok(())}).await.unwrap();
}
#[tokio::test]
async fn native_task_mail_parallel_calls_wait_for_all_original_slots_and_continue_once() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (d, k, _) = grant(&s, &u, false).await;
    let thread = s
        .create_thread(&u, "Two native reads".into())
        .await
        .unwrap();
    let turn = s
        .submit_turn(
            &u,
            thread.id,
            Uuid::new_v4().to_string(),
            "Read native Gmail invoice and quote".into(),
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let c = s.agent_context(&claim.lease).await.unwrap();
    let st = s
        .begin_agent_step(&claim.lease, c.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &st,
        crate::PrivateCheckpoint {
            bytes: br#"{"kind":"parallel-native"}"#.to_vec(),
        },
        vec![
            crate::PendingKnowledgeCall {
                call_id: "invoice".into(),
                input: KnowledgeToolInput::NativeMail { read: search() },
            },
            crate::PendingKnowledgeCall {
                call_id: "quote".into(),
                input: KnowledgeToolInput::NativeMail {
                    read: MailModelRead::Search {
                        query: "quote".into(),
                        cursor: None,
                    },
                },
            },
        ],
    )
    .await
    .unwrap();
    s.wait_for_native_mail_tool(&st, "invoice".into())
        .await
        .unwrap();
    assert_eq!(counts(&s, &turn.run.id).await.1, 2);
    let requests = poll(&s, &d, &k).await;
    assert_eq!(requests.len(), 2);
    assert_ne!(requests[0].invocation.call, requests[1].invocation.call);
    assert_eq!(requests[0].invocation.run, requests[1].invocation.run);
    finish(&s, &d, &k, &requests[0], result()).await;
    s.settle_native_task_mail().await.unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
    finish(&s, &d, &k, &requests[1], result()).await;
    s.settle_native_task_mail().await.unwrap();
    let resumed = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(resumed.lease.run_id(), turn.run.id);
    assert_eq!(resumed.lease.attempt(), 2);
    let c = s.agent_context(&resumed.lease).await.unwrap();
    assert!(c.calls.iter().all(|c| c.result.is_some()));
    assert_eq!(
        s.begin_agent_step(&resumed.lease, c.checkpoint_seq)
            .await
            .unwrap()
            .step(),
        2
    );
}
#[tokio::test]
async fn native_task_mail_explicit_mailbox_suffix_never_autobinds_an_opaque_different_grant() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (d, k, _) = grant(&s, &u, false).await;
    for text in [
        "기기 Gmail에서 target@example.test 계정 메일을 읽어 줘",
        "기기 Gmail 계정은 target@example.test 이고 최근 메일 읽어 줘",
        "Read native Gmail target@example.test account inbox",
        "Read native Gmail account: (target@example.test), inbox",
    ] {
        let thread = s
            .create_thread(&u, "Explicit native account".into())
            .await
            .unwrap();
        let turn = s
            .submit_turn(&u, thread.id, Uuid::new_v4().to_string(), text.into())
            .await
            .unwrap();
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        let st = step(&s, &claim.lease, search(), "named-account-call").await;
        assert!(matches!(
            s.wait_for_native_mail_tool(&st, "named-account-call".into())
                .await,
            Err(StoreError::Forbidden)
        ));
        let run = turn.run.id;
        let tenant = u.tenant.0.clone();
        s.call(move|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM native_task_mail_bindings WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|row|row.get::<_,i64>(0))?,0);assert_eq!(db.conn.query_row("SELECT count(*) FROM native_task_mail_reads WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|row|row.get::<_,i64>(0))?,0);Ok(())}).await.unwrap();
        assert!(poll(&s, &d, &k).await.is_empty());
        s.finish_agent_execution(
            &claim.lease,
            RunState::Cancelled,
            Some(crate::RunFailure::Interrupted),
        )
        .await
        .unwrap();
    }
    // A sender address is data, not account selection. A fresh actual native read
    // still uses the one valid grant and receives its authenticated result.
    let thread = s
        .create_thread(&u, "Native sender filter".into())
        .await
        .unwrap();
    let turn = s
        .submit_turn(
            &u,
            thread.id,
            Uuid::new_v4().to_string(),
            "Read native Gmail from:target@example.test".into(),
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &claim.lease, search(), "legitimate-call").await;
    s.wait_for_native_mail_tool(&st, "legitimate-call".into())
        .await
        .unwrap();
    let requests = poll(&s, &d, &k).await;
    assert_eq!(requests.len(), 1);
    finish(&s, &d, &k, &requests[0], result()).await;
    s.settle_native_task_mail().await.unwrap();
    let resumed = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(resumed.lease.run_id(), turn.run.id);
    assert!(
        s.agent_context(&resumed.lease).await.unwrap().calls[0]
            .result
            .is_some()
    );
}
#[tokio::test]
async fn native_task_mail_fair_sweep_passes_seventy_two_older_missing_connections_and_survives_restart()
 {
    let _lock = TEST_LOCK.lock().await;
    let (dir, s, u) = crate::missions::tests::setup().await;
    let mut old_runs = Vec::new();
    for owner in 0..6 {
        let user = crate::tests::member(&s, &u, &format!("native-fair-{owner}@example.test"))
            .await
            .user;
        let mut input = crate::missions::tests::input(MissionMode::Durable);
        input.task = "Read native Gmail recent messages".into();
        s.create_mission(&user, input).await.unwrap();
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        let context = s.agent_context(&claim.lease).await.unwrap();
        let st = s
            .begin_agent_step(&claim.lease, context.checkpoint_seq)
            .await
            .unwrap();
        let calls = (0..12)
            .map(|call| crate::PendingKnowledgeCall {
                call_id: format!("old-{call}"),
                input: KnowledgeToolInput::NativeMail { read: search() },
            })
            .collect();
        s.commit_agent_step(
            &st,
            crate::PrivateCheckpoint {
                bytes: br#"{"kind":"fair-native-pending"}"#.to_vec(),
            },
            calls,
        )
        .await
        .unwrap();
        s.wait_for_native_mail_tool(&st, "old-0".into())
            .await
            .unwrap();
        old_runs.push(claim.lease.run_id().to_owned());
    }
    let (d, k, _) = grant(&s, &u, true).await;
    let mut input = crate::missions::tests::input(MissionMode::Durable);
    input.task = "Read native Gmail invoice and quote".into();
    let mission = s.create_mission(&u, input).await.unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let context = s.agent_context(&claim.lease).await.unwrap();
    let st = s
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &st,
        crate::PrivateCheckpoint {
            bytes: br#"{"kind":"fair-late-native"}"#.to_vec(),
        },
        vec![
            crate::PendingKnowledgeCall {
                call_id: "later-invoice".into(),
                input: KnowledgeToolInput::NativeMail { read: search() },
            },
            crate::PendingKnowledgeCall {
                call_id: "later-quote".into(),
                input: KnowledgeToolInput::NativeMail {
                    read: MailModelRead::Search {
                        query: "quote".into(),
                        cursor: None,
                    },
                },
            },
        ],
    )
    .await
    .unwrap();
    s.call(|db|{db.conn.execute("UPDATE native_mail_settlement_cursor SET read_rowid=0,dependency_rowid=0 WHERE singleton=1",[])?;Ok(())}).await.unwrap();
    s.wait_for_native_mail_tool(&st, "later-invoice".into())
        .await
        .unwrap();
    let before = counts(&s, claim.lease.run_id()).await;
    assert_eq!((before.0, before.1), (1, 2));
    let run = claim.lease.run_id().to_owned();
    let verify = run.clone();
    s.call(move|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM native_task_mail_reads WHERE run_id=?1 AND request IS NOT NULL",[verify],|row|row.get::<_,i64>(0))?,0);Ok(())}).await.unwrap();
    // The next bounded sweep reaches both late dispatches despite72 older waits.
    let requests = poll(&s, &d, &k).await;
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .iter()
            .all(|request| request.invocation.run.to_string() == run)
    );
    finish(&s, &d, &k, &requests[0], result()).await;
    s.call(|db| {
        db.conn.execute(
            "UPDATE native_mail_settlement_cursor SET read_rowid=0 WHERE singleton=1",
            [],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    s.settle_native_task_mail().await.unwrap();
    let verify = run.clone();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM agent_tool_slots WHERE run_id=?1 AND result IS NOT NULL",
                [verify],
                |row| row.get::<_, i64>(0)
            )?,
            0
        );
        assert_eq!(
            db.conn.query_row(
                "SELECT read_rowid FROM native_mail_settlement_cursor WHERE singleton=1",
                [],
                |row| row.get::<_, i64>(0)
            )?,
            64
        );
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let s = Store::open_with_web(
        dir.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    s.settle_native_task_mail().await.unwrap();
    let verify = run.clone();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM agent_tool_slots WHERE run_id=?1 AND result IS NOT NULL",
                [verify],
                |row| row.get::<_, i64>(0)
            )?,
            1
        );
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
    finish(&s, &d, &k, &requests[1], result()).await;
    for _ in 0..2 {
        s.settle_native_task_mail().await.unwrap();
    }
    let resumed = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(resumed.lease.run_id(), run);
    assert_eq!(counts(&s, &run).await, before);
    assert_eq!(
        s.missions(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|m| m.id == mission.id)
            .unwrap()
            .state,
        "planning"
    );
    s.call(move|db|{for run in old_runs {assert_eq!(db.conn.query_row("SELECT count(*) FROM native_task_mail_reads WHERE run_id=?1 AND status='waiting' AND request IS NULL",[run],|row|row.get::<_,i64>(0))?,12);}Ok(())}).await.unwrap();
}
