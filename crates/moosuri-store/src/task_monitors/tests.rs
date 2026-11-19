use super::*;
use crate::tests::TEST_LOCK;
async fn seed(s: &Store, u: &UserContext) -> SourceSummary {
    let col = s
        .create_collection(u, "Synthetic thread".into(), Uuid::new_v4().to_string())
        .await
        .unwrap();
    s.ingest_text(
        u,
        col.id.clone(),
        IngestBatch {
            request_id: Uuid::new_v4().to_string(),
            expected_cursor: 0,
            next_cursor: 1,
            complete_snapshot: false,
            entries: vec![SourceInput {
                external_id: "first".into(),
                version: "1".into(),
                expected_revision: None,
                title: "First saved message".into(),
                text: "Synthetic support reply".into(),
                speaker: None,
                observed_at: 100,
                valid_from: None,
                valid_until: None,
            }],
        },
    )
    .await
    .unwrap();
    s.sources(u, col.id, None).await.unwrap().items.remove(0)
}
async fn finish(s: &Store, u: &UserContext, src: &SourceSummary) -> MissionView {
    let m = s
        .create_mission(
            u,
            MissionCreate {
                request_id: Uuid::new_v4().to_string(),
                task: "Read the saved support reply".into(),
                constraints: String::new(),
                checks: vec![MissionCheck::SourceQuote {
                    id: src.id.clone(),
                    revision: src.revision,
                    quote: "Synthetic support reply".into(),
                }],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    assert_eq!(review(s, src).await, 1);

    m
}
// Execute a real current child's read/verification path. A partial multi-source
// review consumes the bounded repair rounds; its saved receipts remain current.
async fn review(s: &Store, src: &SourceSummary) -> usize {
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let read = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &read,
        crate::PrivateCheckpoint {
            bytes: b"{}".to_vec(),
        },
        vec![crate::PendingKnowledgeCall {
            call_id: "read".into(),
            input: crate::KnowledgeToolInput::ReadSource {
                id: src.id.clone(),
                revision: src.revision,
                start_byte: 0,
                max_bytes: 4096,
            },
        }],
    )
    .await
    .unwrap();
    let crate::KnowledgeToolResult::Source(document) = s
        .execute_knowledge_tool(&read, "read".into())
        .await
        .unwrap()
    else {
        panic!("actual source receipt required")
    };
    for attempt in 0..3 {
        let ctx = s.agent_context(&claim.lease).await.unwrap();
        let end = s
            .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
            .await
            .unwrap();
        s.commit_agent_step(
            &end,
            crate::PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
        let done = s
            .mission_verify(
                &end,
                MissionCandidate {
                    text: document.text.clone(),
                    evidence: vec![MissionEvidence {
                        step: read.step(),
                        call_id: "read".into(),
                    }],
                },
            )
            .await
            .unwrap();
        if done {
            return attempt + 1;
        }
        assert!(attempt < 2, "bounded verification must settle");
    }
    unreachable!("bounded verification returned without settlement")
}
async fn revise_source(
    s: &Store,
    u: &UserContext,
    source: &SourceSummary,
    text: &str,
) -> SourceSummary {
    s.ingest_text(
        u,
        source.collection_id.clone(),
        IngestBatch {
            request_id: Uuid::new_v4().to_string(),
            expected_cursor: source.revision,
            next_cursor: source.revision + 1,
            complete_snapshot: false,
            entries: vec![SourceInput {
                external_id: source.external_id.clone(),
                version: (source.revision + 1).to_string(),
                expected_revision: Some(source.revision),
                title: source.title.clone(),
                text: text.into(),
                speaker: None,
                observed_at: 100,
                valid_from: None,
                valid_until: None,
            }],
        },
    )
    .await
    .unwrap();
    s.source(u, source.id.clone(), None).await.unwrap().source
}
fn accept(v: &TaskMonitorView) -> TaskMonitorAccept {
    TaskMonitorAccept {
        request_id: Uuid::new_v4().to_string(),
        expected_revision: v.revision,
        cadence_seconds: 60,
        expires_at: now().unwrap() + 3600,
        reuse: true,
        read: true,
        check: true,
        notify: true,
    }
}
async fn connect(s: &Store, u: &UserContext, src: &SourceSummary) {
    let (t, a, id, epoch) = (
        u.tenant.0.clone(),
        u.account.id.clone(),
        src.id.clone(),
        u.epoch,
    );
    s.call(move|db|{
 db.conn.execute("INSERT INTO source_connections(tenant_id,id,account_id,collection_id,kind,name,scope,remote_account,auth_epoch,sync_enabled,poll_interval) SELECT tenant_id,'synthetic-connection',?2,collection_id,'slack','Synthetic connection','exact','synthetic',?4,1,60 FROM knowledge_sources WHERE tenant_id=?1 AND id=?3",params![t,a,id,epoch])?;
 db.conn.execute("INSERT INTO source_provenance SELECT tenant_id,id,revision,'synthetic-connection','request','run',100,'support-thread',NULL FROM knowledge_sources WHERE tenant_id=?1 AND id=?2",params![t,id])?;Ok(())}).await.unwrap();
}
async fn add_message(s: &Store, u: &UserContext, src: &SourceSummary, n: i64, thread: &str) {
    let (t, id, thread) = (u.tenant.0.clone(), src.id.clone(), thread.to_string());
    s.call(move|db|{let collection:String=db.conn.query_row("SELECT collection_id FROM knowledge_sources WHERE tenant_id=?1 AND id=?2",params![t,id],|r|r.get(0))?;
 let id=Uuid::from_u128(n as u128).to_string();db.conn.execute("INSERT INTO knowledge_sources VALUES(?1,?2,?3,?2,1,0)",params![t,id,collection])?;
 db.conn.execute("INSERT INTO knowledge_source_versions VALUES(?1,?2,1,'1','Later message','Synthetic later reply',NULL,101,NULL,NULL,?3)",params![t,id,Sha256::digest(id.as_bytes()).as_slice()])?;
 db.conn.execute("INSERT INTO source_provenance VALUES(?1,?2,1,'synthetic-connection','request','run',101,?3,NULL)",params![t,id,thread])?;Ok(())}).await.unwrap();
}
#[tokio::test]
async fn proposal_receipt_accept_retry_scope_coalesces_and_stop_fences() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let src = seed(&s, &u).await;
    let first = finish(&s, &u, &src).await;
    let proposal = s.task_monitors(&u).await.unwrap().remove(0);
    assert!(matches!(
        proposal.scope,
        TaskMonitorScope::ExactSources { .. }
    ));
    let input = accept(&proposal);
    let active = s
        .accept_task_monitor(&u, proposal.id.clone(), input.clone())
        .await
        .unwrap();
    assert_eq!(active.state, "active");
    assert_eq!(
        s.accept_task_monitor(&u, proposal.id.clone(), input.clone())
            .await
            .unwrap()
            .id,
        active.id
    );
    let mut changed = input;
    changed.reuse = false;
    assert!(matches!(
        s.accept_task_monitor(&u, proposal.id.clone(), changed)
            .await,
        Err(StoreError::Conflict)
    ));
    let second = finish(&s, &u, &src).await;
    let all = s.task_monitors(&u).await.unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].linked_mission_ids.len(), 2);
    assert!(all[0].linked_mission_ids.contains(&first.id));
    assert!(all[0].linked_mission_ids.contains(&second.id));
    let stopped = s
        .stop_task_monitor(
            &u,
            active.id.clone(),
            TaskMonitorControl {
                expected_revision: active.revision,
                revoke_policy: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(stopped.reason.as_deref(), Some("policy_revoked"));
    assert!(matches!(
        s.stop_task_monitor(
            &u,
            active.id,
            TaskMonitorControl {
                expected_revision: active.revision,
                revoke_policy: false
            }
        )
        .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn thread_new_reply_gates_before_inference_and_changed_subset_is_bounded() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let src = seed(&s, &u).await;
    connect(&s, &u, &src).await;
    finish(&s, &u, &src).await;
    let proposal = s.task_monitors(&u).await.unwrap().remove(0);
    assert!(matches!(
        proposal.scope,
        TaskMonitorScope::ConnectedThread { .. }
    ));
    let active = s
        .accept_task_monitor(&u, proposal.id.clone(), accept(&proposal))
        .await
        .unwrap();
    let (t, id) = (u.tenant.0.clone(), active.id.clone());
    s.call(move |db| {
        let schedule: String = db.conn.query_row(
            "SELECT schedule_id FROM task_monitors WHERE tenant_id=?1 AND id=?2",
            params![t, id],
            |r| r.get(0),
        )?;
        assert!(!ready(&db.conn, &t, &schedule, now()?)?);
        Ok(())
    })
    .await
    .unwrap();
    let (t, id, monitor) = (u.tenant.0.clone(), src.id.clone(), active.id.clone());
    s.call(move|db|{
 db.conn.execute("INSERT INTO knowledge_source_versions SELECT tenant_id,source_id,2,'version-only-change',title,text,speaker,observed_at,valid_from,valid_until,zeroblob(32) FROM knowledge_source_versions WHERE tenant_id=?1 AND source_id=?2 AND revision=1",params![t,id])?;
 db.conn.execute("INSERT INTO source_provenance SELECT tenant_id,source_id,2,connection_id,request_id,run_id,received_at,thread_id,parent_id FROM source_provenance WHERE tenant_id=?1 AND source_id=?2 AND source_revision=1",params![t,id])?;
 db.conn.execute("UPDATE knowledge_sources SET revision=2 WHERE tenant_id=?1 AND id=?2",params![t,id])?;
 db.conn.execute("UPDATE missions SET state='quarantined',grant_enabled=0 WHERE tenant_id=?1 AND id=(SELECT parent_mission_id FROM task_monitors WHERE tenant_id=?1 AND id=?2)",params![t,monitor])?;
 let schedule:String=db.conn.query_row("SELECT schedule_id FROM task_monitors WHERE tenant_id=?1 AND id=?2",params![t,monitor],|r|r.get(0))?;
 assert!(!ready(&db.conn,&t,&schedule,now()?)?);assert!(policy_current(&db.conn,&t,&monitor,now()?)?);Ok(())}).await.unwrap();
    add_message(&s, &u, &src, 1, "unrelated-thread").await;
    let (t, id) = (u.tenant.0.clone(), active.id.clone());
    s.call(move |db| {
        let schedule: String = db.conn.query_row(
            "SELECT schedule_id FROM task_monitors WHERE tenant_id=?1 AND id=?2",
            params![t, id],
            |r| r.get(0),
        )?;
        assert!(!ready(&db.conn, &t, &schedule, now()?)?);
        Ok(())
    })
    .await
    .unwrap();
    add_message(&s, &u, &src, 2, "support-thread").await;
    let (t, id) = (u.tenant.0.clone(), active.id.clone());
    s.call(move |db| {
        let schedule: String = db.conn.query_row(
            "SELECT schedule_id FROM task_monitors WHERE tenant_id=?1 AND id=?2",
            params![t, id],
            |r| r.get(0),
        )?;
        assert!(
            ready(&db.conn, &t, &schedule, now()?)?,
            "{}",
            enc(&view(&db.conn, &t, &id)?).unwrap()
        );
        Ok(())
    })
    .await
    .unwrap();
    let at = now().unwrap() + 61;
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    let admitted = s.poll_recurring_missions_at(at).await.unwrap();
    assert_eq!(
        admitted,
        1,
        "{}",
        enc(&s.task_monitors(&u).await.unwrap()).unwrap()
    );
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 0);
    let all = s.task_monitors(&u).await.unwrap();
    assert_eq!(all[0].observations.len(), 1);
    let child = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == all[0].observations[0].mission_id)
        .unwrap();
    assert_eq!(child.request.checks.len(), 1);
    assert!(
        matches!(&child.request.checks[0],MissionCheck::SourceQuote{id,..} if id==&Uuid::from_u128(2).to_string())
    );
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let st = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    assert!(matches!(
        s.commit_agent_step(
            &st,
            crate::PrivateCheckpoint {
                bytes: b"{}".to_vec()
            },
            vec![crate::PendingKnowledgeCall {
                call_id: "outside".into(),
                input: crate::KnowledgeToolInput::ReadSource {
                    id: src.id.clone(),
                    revision: 1,
                    start_byte: 0,
                    max_bytes: 4096
                }
            }]
        )
        .await,
        Err(StoreError::Forbidden)
    ));
    // Already-admitted fingerprints are not retried after a failure/unknown.
    let (t, id, mission) = (u.tenant.0.clone(), active.id.clone(), child.id);
    s.call(move |db| {
        db.conn.execute(
            "UPDATE missions SET state='failed' WHERE tenant_id=?1 AND id=?2",
            params![t, mission],
        )?;
        let schedule: String = db.conn.query_row(
            "SELECT schedule_id FROM task_monitors WHERE tenant_id=?1 AND id=?2",
            params![t, id],
            |r| r.get(0),
        )?;
        assert!(!ready(&db.conn, &t, &schedule, now()?)?);
        Ok(())
    })
    .await
    .unwrap();
    for n in 3..8 {
        add_message(&s, &u, &src, n, "support-thread").await;
    }
    let (t, id) = (u.tenant.0.clone(), active.id.clone());
    s.call(move |db| {
        let schedule: String = db.conn.query_row(
            "SELECT schedule_id FROM task_monitors WHERE tenant_id=?1 AND id=?2",
            params![t, id],
            |r| r.get(0),
        )?;
        assert!(!ready(&db.conn, &t, &schedule, now()?)?);
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(
        s.task_monitors(&u).await.unwrap()[0].reason.as_deref(),
        Some("coverage_limited_needs_review")
    );
}
#[tokio::test]
async fn monitor_connection_withdrawal_fences_queued_child_and_publication_owner_isolated() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let src = seed(&s, &u).await;
    connect(&s, &u, &src).await;
    finish(&s, &u, &src).await;
    let proposal = s.task_monitors(&u).await.unwrap().remove(0);
    let other = crate::tests::member(&s, &u, "monitor-other@example.test")
        .await
        .user;
    assert!(s.task_monitors(&other).await.unwrap().is_empty());
    assert!(matches!(
        s.accept_task_monitor(&other, proposal.id.clone(), accept(&proposal))
            .await,
        Err(StoreError::NotFound)
    ));
    let active = s
        .accept_task_monitor(&u, proposal.id.clone(), accept(&proposal))
        .await
        .unwrap();
    add_message(&s, &u, &src, 2, "support-thread").await;
    let at = now().unwrap() + 61;
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 1);
    let observation = s
        .task_monitors(&u)
        .await
        .unwrap()
        .remove(0)
        .observations
        .remove(0);
    s.resolve_task_monitor(
        &u,
        active.id.clone(),
        observation.ordinal,
        TaskMonitorResolve {
            note: "Resolved locally".into(),
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        s.resolve_task_monitor(
            &u,
            active.id.clone(),
            observation.ordinal,
            TaskMonitorResolve {
                note: "Different replay".into()
            }
        )
        .await,
        Err(StoreError::Conflict)
    ));
    let (t, id, child) = (
        u.tenant.0.clone(),
        active.id.clone(),
        observation.mission_id,
    );
    s.call(move|db|{
      let schedule:String=db.conn.query_row("SELECT schedule_id FROM task_monitors WHERE tenant_id=?1 AND id=?2",params![t,id],|r|r.get(0))?;
      db.conn.execute("UPDATE source_connections SET config_revision=config_revision+1 WHERE tenant_id=?1 AND id='synthetic-connection'",params![t])?;
      assert!(!publication_current(&db.conn,&t,&schedule,&child,now()?)?);
      assert!(!ready(&db.conn,&t,&schedule,now()?)?);
      let state:String=db.conn.query_row("SELECT state FROM missions WHERE tenant_id=?1 AND id=?2",params![t,child],|r|r.get(0))?;assert_eq!(state,"cancelled");
      assert_eq!(view(&db.conn,&t,&id)?.state,"withdrawn");
      let active:i64=db.conn.query_row("SELECT count(*) FROM task_monitor_policies WHERE tenant_id=?1 AND state='active'",params![t],|r|r.get(0))?;assert_eq!(active,0);
      let memories:i64=db.conn.query_row("SELECT count(*) FROM knowledge_memories WHERE tenant_id=?1",params![t],|r|r.get(0))?;assert_eq!(memories,0);Ok(())
    }).await.unwrap();
}
#[tokio::test]
async fn published_monitor_notice_rechecks_connection_and_expiry_at_push_and_read_without_poll() {
    let _g = TEST_LOCK.lock().await;
    for revoke in ["connection", "expiry", "stop"] {
        let (_d, s, u) = crate::missions::tests::setup().await;
        let src = seed(&s, &u).await;
        connect(&s, &u, &src).await;
        finish(&s, &u, &src).await;
        let proposal = s.task_monitors(&u).await.unwrap().remove(0);
        let active = s
            .accept_task_monitor(&u, proposal.id.clone(), accept(&proposal))
            .await
            .unwrap();
        let (_, p256dh) = moosuri_connectors::push::generate_vapid().unwrap();
        let device = s
            .enroll_push(
                &u,
                crate::push::PushEnrollment {
                    operation_revision: 1,
                    request_id: Uuid::new_v4().to_string(),
                    device_id: Uuid::new_v4().to_string(),
                    subscription: moosuri_connectors::push::PushSubscription {
                        endpoint: format!("https://fcm.googleapis.com/fcm/send/{}", Uuid::new_v4()),
                        p256dh,
                        auth: "_ordMnz7uTCmrpBTeUV4Bw".into(), // gitleaks:allow -- synthetic fixture
                    },
                },
                "https://push-owner.example.test".into(),
            )
            .await
            .unwrap();
        add_message(&s, &u, &src, 2, "support-thread").await;
        let at = now().unwrap() + 61;
        s.enqueue_scheduled_reminders_at(at).await.unwrap();
        assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 1);
        let child = s
            .task_monitors(&u)
            .await
            .unwrap()
            .remove(0)
            .observations
            .remove(0)
            .mission_id;
        let (t, child) = (u.tenant.0.clone(), child);
        s.call(move |db| {
            db.conn.execute(
                "UPDATE missions SET state='achieved' WHERE tenant_id=?1 AND id=?2",
                params![t, child],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        s.poll_recurring_missions_at(at).await.unwrap();
        let notice = s
            .notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .remove(0);
        s.poll_push_queue().await.unwrap();
        let queued = s
            .claim_push()
            .await
            .unwrap()
            .expect("published monitor push is initially deliverable");
        if revoke == "stop" {
            let control = TaskMonitorControl {
                expected_revision: active.revision,
                revoke_policy: true,
            };
            let stopped = s
                .stop_task_monitor(&u, active.id.clone(), control.clone())
                .await
                .unwrap();
            assert_eq!(stopped.state, "stopped");
            assert_eq!(stopped.reason.as_deref(), Some("policy_revoked"));
            assert_eq!(
                s.stop_task_monitor(&u, active.id.clone(), control)
                    .await
                    .unwrap()
                    .revision,
                stopped.revision
            );
            let (t, id, notification) = (u.tenant.0.clone(), active.id.clone(), notice.id.clone());
            s.call(move |db| {
                assert_eq!(db.conn.query_row("SELECT state FROM task_monitor_policies WHERE tenant_id=?1 AND id=(SELECT policy_id FROM task_monitors WHERE tenant_id=?1 AND id=?2)", params![t,id], |r| r.get::<_,String>(0))?, "revoked");
                let flags:(i64,i64,i64)=db.conn.query_row("SELECT source_suppressed,push_suppressed,dismissed FROM local_notifications WHERE tenant_id=?1 AND id=?2", params![t,notification], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
                assert_eq!(flags,(0,1,1));
                Ok(())
            }).await.unwrap();
        } else {
            let (t, id, mode) = (u.tenant.0.clone(), active.id.clone(), revoke.to_string());
            s.call(move|db|{
            if mode=="connection" {
                db.conn.execute("UPDATE source_connections SET credential_epoch=credential_epoch+1 WHERE tenant_id=?1 AND id='synthetic-connection'",params![t])?;
            }else{
                db.conn.execute("UPDATE task_monitor_policies SET expires_at=?3 WHERE tenant_id=?1 AND id=(SELECT policy_id FROM task_monitors WHERE tenant_id=?1 AND id=?2)",params![t,id,now()?-1])?;
            }
            // No scheduler poll or explicit stop has run to suppress this notice.
            let state:String=db.conn.query_row("SELECT state FROM task_monitors WHERE tenant_id=?1 AND id=?2",params![t,id],|r|r.get(0))?;
            assert_eq!(state,"active");Ok(())
        }).await.unwrap();
        }
        assert!(matches!(
            s.commit_push_dispatch(&queued.lease).await,
            Err(StoreError::Conflict)
        ));
        assert!(
            s.notifications(&u, None, false)
                .await
                .unwrap()
                .items
                .is_empty()
        );
        // A push already accepted by transport still cannot display private content.
        let (t, notification) = (u.tenant.0.clone(), notice.id.clone());
        s.call(move |db| {
            db.conn.execute(
                "UPDATE push_queue SET state='accepted' WHERE tenant_id=?1 AND notification_id=?2",
                params![t, notification],
            )?;
            Ok(())
        })
        .await
        .unwrap();
        assert!(matches!(
            s.authorize_push_display(&u, device.binding_id, notice.id.clone())
                .await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            s.mark_notification(&u, notice.id, false).await,
            Err(StoreError::NotFound)
        ));
    }
}

#[tokio::test]
async fn invalidated_monitor_observation_cannot_publish_or_deliver_a_saved_success_notice() {
    let _guard = TEST_LOCK.lock().await;
    for boundary in ["before_publication", "after_publication", "deleted"] {
        let (_dir, store, user) = crate::missions::tests::setup().await;
        let source = seed(&store, &user).await;
        finish(&store, &user, &source).await;
        let proposed = store.task_monitors(&user).await.unwrap().remove(0);
        let active = store
            .accept_task_monitor(&user, proposed.id.clone(), accept(&proposed))
            .await
            .unwrap();
        let (_, p256dh) = moosuri_connectors::push::generate_vapid().unwrap();
        let device = store
            .enroll_push(
                &user,
                crate::push::PushEnrollment {
                    operation_revision: 1,
                    request_id: Uuid::new_v4().to_string(),
                    device_id: Uuid::new_v4().to_string(),
                    subscription: moosuri_connectors::push::PushSubscription {
                        endpoint: format!("https://fcm.googleapis.com/fcm/send/{}", Uuid::new_v4()),
                        p256dh,
                        auth: "_ordMnz7uTCmrpBTeUV4Bw".into(), // gitleaks:allow -- synthetic fixture
                    },
                },
                "https://push-owner.example.test".into(),
            )
            .await
            .unwrap();
        let source = revise_source(&store, &user, &source, "Synthetic support reply revised").await;
        let at = now().unwrap() + 61;
        store.enqueue_scheduled_reminders_at(at).await.unwrap();
        assert_eq!(store.poll_recurring_missions_at(at).await.unwrap(), 1);
        assert_eq!(review(&store, &source).await, 1);
        let observation = store
            .task_monitors(&user)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.id == active.id)
            .unwrap()
            .observations
            .remove(0);
        assert_eq!(observation.state, "achieved");
        let saved = if boundary != "before_publication" {
            store.poll_recurring_missions_at(at).await.unwrap();
            let notice = store
                .notifications(&user, None, false)
                .await
                .unwrap()
                .items
                .remove(0);
            assert_eq!(
                notice.mission_id.as_deref(),
                Some(observation.mission_id.as_str())
            );
            store.poll_push_queue().await.unwrap();
            let queued = store.claim_push().await.unwrap().unwrap();
            Some((notice, queued))
        } else {
            None
        };
        let fresh = if boundary == "deleted" {
            store
                .delete_mission(&user, observation.mission_id.clone())
                .await
                .unwrap();
            None
        } else {
            Some(revise_source(&store, &user, &source, "Synthetic support reply newest").await)
        };
        // No scheduler pass is needed to fence an already saved/claimed notice.
        assert!(
            store
                .notifications(&user, None, false)
                .await
                .unwrap()
                .items
                .is_empty(),
            "{boundary}"
        );
        if let Some((notice, queued)) = saved {
            assert!(
                matches!(
                    store.commit_push_dispatch(&queued.lease).await,
                    Err(StoreError::Conflict)
                ),
                "{boundary}"
            );
            let (tenant, notification) = (user.tenant.0.clone(), notice.id.clone());
            store.call(move |db| {
                db.conn.execute("UPDATE push_queue SET state='accepted' WHERE tenant_id=?1 AND notification_id=?2", params![tenant,notification])?;
                Ok(())
            }).await.unwrap();
            assert!(matches!(
                store
                    .authorize_push_display(&user, device.binding_id, notice.id.clone())
                    .await,
                Err(StoreError::NotFound)
            ));
            assert!(matches!(
                store.mark_notification(&user, notice.id, false).await,
                Err(StoreError::NotFound)
            ));
        }
        store.poll_recurring_missions_at(at).await.unwrap();
        let (tenant, child) = (user.tenant.0.clone(), observation.mission_id.clone());
        store.call(move |db| {
            let (state, publication): (String, String) = db.conn.query_row(
                "SELECT m.state,o.publication_state FROM missions m JOIN recurring_mission_occurrences o ON o.tenant_id=m.tenant_id AND o.mission_id=m.id WHERE m.tenant_id=?1 AND m.id=?2",
                params![tenant,child], |row| Ok((row.get(0)?,row.get(1)?)),
            )?;
            assert!(matches!(state.as_str(), "quarantined" | "deleted"));
            if boundary == "before_publication" { assert_eq!(publication, "fenced"); }
            Ok(())
        }).await.unwrap();
        let current = store
            .task_monitors(&user)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.id == active.id)
            .unwrap();
        assert_eq!(
            current.state, "active",
            "child withdrawal does not revoke monitor permission"
        );
        if let Some(fresh) = fresh {
            store.enqueue_scheduled_reminders_at(at + 61).await.unwrap();
            assert_eq!(store.poll_recurring_missions_at(at + 61).await.unwrap(), 1);
            assert_eq!(review(&store, &fresh).await, 1);
            store.poll_recurring_missions_at(at + 61).await.unwrap();
            store.poll_recurring_missions_at(at + 61).await.unwrap();
            assert_eq!(
                store
                    .notifications(&user, None, false)
                    .await
                    .unwrap()
                    .items
                    .len(),
                1
            );
            assert_eq!(
                store
                    .task_monitors(&user)
                    .await
                    .unwrap()
                    .into_iter()
                    .find(|v| v.id == active.id)
                    .unwrap()
                    .observations
                    .len(),
                2
            );
        }
    }
}

#[tokio::test]
async fn monitor_partial_current_review_remains_visible_with_its_actual_source_receipt() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user) = crate::missions::tests::setup().await;
    let source = seed(&store, &user).await;
    connect(&store, &user, &source).await;
    finish(&store, &user, &source).await;
    let proposed = store.task_monitors(&user).await.unwrap().remove(0);
    let active = store
        .accept_task_monitor(&user, proposed.id.clone(), accept(&proposed))
        .await
        .unwrap();
    add_message(&store, &user, &source, 2, "support-thread").await;
    add_message(&store, &user, &source, 3, "support-thread").await;
    let at = now().unwrap() + 61;
    store.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert_eq!(store.poll_recurring_missions_at(at).await.unwrap(), 1);
    let changed = store
        .source(&user, Uuid::from_u128(2).to_string(), None)
        .await
        .unwrap()
        .source;
    assert_eq!(review(&store, &changed).await, 3);
    let observation = store
        .task_monitors(&user)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == active.id)
        .unwrap()
        .observations
        .remove(0);
    assert_eq!(observation.state, "partial");
    store.poll_recurring_missions_at(at).await.unwrap();
    let notices = store.notifications(&user, None, false).await.unwrap().items;
    assert_eq!(notices.len(), 1);
    assert_eq!(
        notices[0].mission_id.as_deref(),
        Some(observation.mission_id.as_str())
    );
    assert_eq!(notices[0].message, "실행 상태: partial");
}

#[tokio::test]
async fn expired_reusable_monitor_does_not_swallow_the_next_completed_tasks_proposal() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user) = crate::missions::tests::setup().await;
    let source = seed(&store, &user).await;
    let first = finish(&store, &user, &source).await;
    let proposed = store.task_monitors(&user).await.unwrap().remove(0);
    let active = store
        .accept_task_monitor(&user, proposed.id.clone(), accept(&proposed))
        .await
        .unwrap();
    let (tenant, monitor) = (user.tenant.0.clone(), active.id.clone());
    store.call(move |db| {
        db.conn.execute("UPDATE task_monitor_policies SET expires_at=?3 WHERE tenant_id=?1 AND id=(SELECT policy_id FROM task_monitors WHERE tenant_id=?1 AND id=?2)", params![tenant,monitor,now()? - 1])?;
        Ok(())
    }).await.unwrap();
    let second = finish(&store, &user, &source).await;
    let all = store.task_monitors(&user).await.unwrap();
    let old = all.iter().find(|v| v.id == active.id).unwrap();
    assert_eq!(old.state, "expired");
    assert_eq!(old.linked_mission_ids, [first.id]);
    let replacement = all
        .iter()
        .find(|v| v.parent_mission_id == second.id)
        .unwrap();
    assert_eq!(replacement.state, "proposed");
    assert_eq!(replacement.linked_mission_ids, [second.id]);
    assert!(replacement.policy_id.is_none());
    assert!(replacement.existing_monitor_id.is_none());
    assert!(replacement.observations.is_empty());
    let renewed = store
        .accept_task_monitor(&user, replacement.id.clone(), accept(replacement))
        .await
        .unwrap();
    assert_eq!(renewed.state, "active");
    assert_ne!(renewed.policy_id, active.policy_id);
    assert!(renewed.observations.is_empty());
}

#[tokio::test]
async fn monitor_reuse_requires_the_original_connection_and_owner_rebinds_future_reviews() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user) = crate::missions::tests::setup().await;
    let source = seed(&store, &user).await;
    finish(&store, &user, &source).await;
    let proposal = store.task_monitors(&user).await.unwrap().remove(0);
    let active = store
        .accept_task_monitor(&user, proposal.id.clone(), accept(&proposal))
        .await
        .unwrap();
    let stop = |expected_revision| TaskMonitorControl {
        expected_revision,
        revoke_policy: false,
    };
    store
        .stop_task_monitor(&user, active.id.clone(), stop(active.revision))
        .await
        .unwrap();

    let original = store
        .model_settings(&user)
        .await
        .unwrap()
        .installation_default
        .unwrap();
    let another_pair = ModelSelection {
        model: "same-connection".into(),
        ..original.clone()
    };
    let verify = async |model: &ModelSelection| {
        store
            .record_model_probe(
                &user,
                model.provider_id.clone(),
                model.provider_revision,
                model.model.clone(),
                model.effort.clone(),
            )
            .await
            .unwrap();
        store
            .record_tool_probe(
                &user,
                model.provider_id.clone(),
                model.provider_revision,
                model.model.clone(),
                model.effort.clone(),
            )
            .await
            .unwrap();
        store
            .set_default_model(&user, Some(model.clone()))
            .await
            .unwrap();
    };
    verify(&another_pair).await;
    let same_provider = finish(&store, &user, &source).await;
    let reused = store
        .task_monitors(&user)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.parent_mission_id == same_provider.id)
        .unwrap();
    assert_eq!(reused.state, "active");
    assert_eq!(reused.policy_id, active.policy_id);
    store
        .stop_task_monitor(&user, reused.id.clone(), stop(reused.revision))
        .await
        .unwrap();

    let provider = store
        .create_provider(
            &user,
            crate::ProviderChange {
                name: "Different authorized destination".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:10/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: crate::CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    let another_provider = ModelSelection {
        provider_id: provider.id,
        provider_revision: 1,
        model: "different-connection".into(),
        effort: None,
        ..original.clone()
    };
    verify(&another_provider).await;
    let new_task = finish(&store, &user, &source).await;
    let pending = store
        .task_monitors(&user)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.parent_mission_id == new_task.id)
        .unwrap();
    assert_eq!(pending.state, "proposed");
    assert!(pending.policy_id.is_none());
    let monitor = store
        .accept_task_monitor(&user, pending.id.clone(), accept(&pending))
        .await
        .unwrap();
    assert_ne!(monitor.policy_id, active.policy_id);

    store
        .set_default_model(&user, Some(original.clone()))
        .await
        .unwrap();
    revise_source(&store, &user, &source, "The checked answer changed.").await;
    let due = now().unwrap() + 61;
    store.enqueue_scheduled_reminders_at(due).await.unwrap();
    assert_eq!(store.poll_recurring_missions_at(due).await.unwrap(), 0);
    let paused = store
        .task_monitors(&user)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == monitor.id)
        .unwrap();
    assert_eq!(paused.state, "paused");
    assert_eq!(
        paused.reason.as_deref(),
        Some("model_reauthorization_required")
    );
    assert!(
        store
            .resume_task_monitor(&user, monitor.id.clone(), paused.revision, false)
            .await
            .is_err()
    );
    let resumed = store
        .resume_task_monitor(&user, monitor.id.clone(), paused.revision, true)
        .await
        .unwrap();
    assert_eq!(resumed.state, "active");
    assert_eq!(resumed.policy_id, paused.policy_id);
    assert_eq!(resumed.expires_at, paused.expires_at);
    assert_eq!(resumed.cadence_seconds, paused.cadence_seconds);
    assert_eq!(resumed.sources[0].id, paused.sources[0].id);
    assert_eq!(resumed.observations.len(), paused.observations.len());
    store
        .enqueue_scheduled_reminders_at(due + 60)
        .await
        .unwrap();
    assert_eq!(store.poll_recurring_missions_at(due + 60).await.unwrap(), 1);
    let next = store.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(next.binding.id, original.provider_id);
    assert_eq!(next.binding.model, original.model);
}
