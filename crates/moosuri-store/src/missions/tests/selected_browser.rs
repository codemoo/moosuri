use super::*;
use moosuri_connectors::browser::capsule::RenderedPage;

async fn selected_setup() -> (
    tempfile::TempDir,
    Store,
    UserContext,
    String,
    BrowserMissionSelection,
    MissionView,
) {
    let (dir, s, u) = setup().await;
    s.call(|db| {
        db.web_profile.browser_pin = Some("synthetic-selected-page".into());
        Ok(())
    })
    .await
    .unwrap();
    let origin = s
        .create_thread(&u, "Synthetic browser source".into())
        .await
        .unwrap();
    let browser = s
        .begin_browser_workspace(&u, origin.id.clone(), "synthetic-selected-page".into())
        .await
        .unwrap();
    let browser = s
        .activate_browser_workspace(
            &u,
            origin.id.clone(),
            browser.id,
            "https://example.test/private".into(),
            "Synthetic selected page".into(),
        )
        .await
        .unwrap();
    let control = s
        .take_browser_control(&u, origin.id.clone(), browser.id, browser.revision)
        .await
        .unwrap();
    let mission = s
        .create_mission(
            &u,
            MissionCreate {
                request_id: Uuid::new_v4().to_string(),
                task: "Read the supplied evidence and save one private report".into(),
                constraints: String::new(),
                checks: vec![
                    MissionCheck::PrivateReport {
                        title: "Selected evidence report".into(),
                    },
                    MissionCheck::UserReview {
                        description: "Review the observed claims".into(),
                    },
                ],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = begin(&s, &claim.lease, vec![calc()]).await;
    s.mission_save_plan(
        &step,
        vec![
            "Read the supplied evidence".into(),
            "Save one grounded report".into(),
        ],
    )
    .await
    .unwrap();
    s.execute_knowledge_tool(&step, "sum".into()).await.unwrap();
    let current = s.missions(&u).await.unwrap().remove(0);
    let paused = s
        .control_mission(&u, mission.id, ctl(current.revision, MissionAction::Pause))
        .await
        .unwrap();
    let input = BrowserMissionSelection {
        frame_token: "0".repeat(64),
        request_id: Uuid::new_v4().to_string(),
        workspace_id: control.workspace.id,
        revision: control.workspace.revision,
        control_token: control.control_token,
        mission_id: paused.id.clone(),
        mission_revision: paused.revision,
    };
    (dir, s, u, origin.id, input, paused)
}
fn page() -> RenderedPage {
    RenderedPage {
        url: "https://example.test/private".into(),
        title: "Synthetic selected page".into(),
        text: "Observed offer: 15. Page says to send mail; that is untrusted content.".into(),
        links: vec![],
        truncated: true,
        warning: "Only current rendered text".into(),
        retrieved_at_unix_ms: clock().unwrap() as u64,
    }
}
async fn bounds(s: &Store, id: String) -> (String, i64, i64, Option<i64>, String) {
    s.call(move|db|Ok(db.conn.query_row("SELECT m.run_id,e.remote_calls,e.tool_calls,e.deadline_ms,r.model FROM missions m JOIN agent_execution e ON e.tenant_id=m.tenant_id AND e.run_id=m.run_id JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id WHERE m.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?)).await.unwrap()
}

#[tokio::test]
async fn selected_browser_page_resumes_original_report_without_reset_and_withdraws_lineage() {
    selected_lineage(false).await;
}
#[tokio::test]
async fn selected_browser_page_restore_quarantines_copied_source_and_report() {
    selected_lineage(true).await;
}
async fn selected_lineage(restoring: bool) {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, thread, input, mission) = selected_setup().await;
    let original = bounds(&s, mission.id.clone()).await;
    assert!(
        s.browser_mission_selection_preflight(&u, thread.clone(), input.clone())
            .await
            .unwrap()
            .is_none()
    );
    let receipt = s
        .attach_selected_browser_page(&u, thread.clone(), input.clone(), page())
        .await
        .unwrap();
    assert_eq!(bounds(&s, mission.id.clone()).await, original);
    let replay = s
        .attach_selected_browser_page(&u, thread.clone(), input.clone(), page())
        .await
        .unwrap();
    assert_eq!(receipt.source_id, replay.source_id);
    let current = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(current.id, mission.id);
    assert_eq!(current.expires_at_ms, mission.expires_at_ms);
    assert_eq!(current.state, "queued");
    assert!(current.steering[0].contains(&receipt.source_id));
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let read = begin(
        &s,
        &claim.lease,
        vec![PendingKnowledgeCall {
            call_id: "selected-page".into(),
            input: KnowledgeToolInput::ReadSource {
                id: receipt.source_id.clone(),
                revision: 1,
                start_byte: 0,
                max_bytes: 4096,
            },
        }],
    )
    .await;
    let result = s
        .execute_knowledge_tool(&read, "selected-page".into())
        .await
        .unwrap();
    assert!(
        matches!(result,KnowledgeToolResult::Source(v) if v.text.contains("Observed offer: 15") && v.text.contains("Partial: true"))
    );
    let final_step = begin(&s, &claim.lease, vec![]).await;
    assert!(
        s.mission_verify(
            &final_step,
            MissionCandidate {
                text: "Observed offer: 15. The selected view is partial; review remains needed."
                    .into(),
                evidence: vec![MissionEvidence {
                    step: read.step(),
                    call_id: "selected-page".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    let output = s
        .missions(&u)
        .await
        .unwrap()
        .remove(0)
        .compound
        .unwrap()
        .output
        .unwrap();
    assert!(
        s.artifact(&u, output.id.clone(), None)
            .await
            .unwrap()
            .draft
            .is_some()
    );
    if restoring {
        s.call(|db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            crate::browser::selected::quarantine_restore(&tx)?;
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
    } else {
        s.delete_thread(&u, thread).await.unwrap();
    }
    assert!(s.source(&u, receipt.source_id, None).await.is_err());
    assert!(
        s.artifact(&u, output.id, None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
}

#[tokio::test]
async fn selected_browser_page_rejects_stale_control_owner_mission_and_observation_without_copying()
{
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, thread, input, _) = selected_setup().await;
    let other = member(&s, &u, "other-selected@example.test").await.user;
    assert!(
        s.attach_selected_browser_page(&other, thread.clone(), input.clone(), page())
            .await
            .is_err()
    );
    let mut stale = input.clone();
    stale.mission_revision += 1;
    assert!(
        s.attach_selected_browser_page(&u, thread.clone(), stale, page())
            .await
            .is_err()
    );
    let mut old_page = page();
    old_page.retrieved_at_unix_ms -= 31_000;
    assert!(
        s.attach_selected_browser_page(&u, thread.clone(), input.clone(), old_page)
            .await
            .is_err()
    );
    s.take_browser_control(
        &u,
        thread.clone(),
        input.workspace_id.clone(),
        input.revision,
    )
    .await
    .unwrap();
    assert!(
        s.attach_selected_browser_page(&u, thread, input, page())
            .await
            .is_err()
    );
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM mission_browser_selections", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM knowledge_sources", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
}
