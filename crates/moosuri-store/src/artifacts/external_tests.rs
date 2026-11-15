//! Synthetic owner outcomes through real admission, slots, IO permits and SQLite.
//! No provider/model network request or credential is used.
use super::*;
use crate::mail::ExternalToolStart;
use crate::{
    agent_execution::*,
    tests::{TEST_LOCK, admin, directory, member},
};
use moosuri_connectors::gmail::SecretString;

async fn setup() -> (tempfile::TempDir, Store, UserContext, String) {
    let dir = directory();
    let store = Store::open_with_web(
        dir.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    let u = admin(&store).await.user;
    let p = store
        .create_provider(
            &u,
            ProviderChange {
                name: "Synthetic reports".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
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
    let thread = store
        .create_thread(&u, "Synthetic report".into())
        .await
        .unwrap()
        .id;
    (dir, store, u, thread)
}
fn checkpoint() -> PrivateCheckpoint {
    PrivateCheckpoint {
        bytes: br#"{"v":1,"synthetic":"report"}"#.to_vec(),
    }
}
async fn start(s: &Store, u: &UserContext, t: &str, write: bool) -> AgentClaim {
    let text = if write {
        "외부 자료를 읽고 보고서 문서로 저장해 줘"
    } else {
        "공개 자료 읽어 줘"
    };
    s.submit_turn(u, t.into(), Uuid::new_v4().to_string(), text.into())
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = s
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(&step, checkpoint(), vec![])
        .await
        .unwrap();
    s.prepare_artifact_intent(
        &step,
        write.then(|| ArtifactIntent {
            action: "create".into(),
            kind: ArtifactKind::Document,
            intent_quote: text.into(),
            target_quote: None,
        }),
    )
    .await
    .unwrap();
    claim
}
async fn step(s: &Store, claim: &AgentClaim, input: KnowledgeToolInput, id: &str) -> StepLease {
    let context = s.agent_context(&claim.lease).await.unwrap();
    let step = s
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        checkpoint(),
        vec![PendingKnowledgeCall {
            call_id: id.into(),
            input,
        }],
    )
    .await
    .unwrap();
    step
}
async fn external(
    s: &Store,
    claim: &AgentClaim,
    request: ExternalToolRequest,
    result: serde_json::Value,
) -> StepLease {
    let step = step(s, claim, KnowledgeToolInput::External { request }, "read").await;
    let ExternalToolStart::Dispatch(p) =
        s.prepare_external_tool(&step, "read".into()).await.unwrap()
    else {
        panic!("read not admitted")
    };
    assert!(matches!(
        s.complete_external_tool(&p, KnowledgeToolResult::External(result))
            .await
            .unwrap(),
        KnowledgeToolResult::External(_)
    ));
    step
}
fn web_request() -> ExternalToolRequest {
    ExternalToolRequest::Web(WebToolRequest::Read {
        url: "https://example.test/report-source".into(),
    })
}
fn web_page() -> serde_json::Value {
    serde_json::json!({"web_page":{"url":"https://example.test/report-source","title":"Synthetic source","text":"SYNTHETIC-EXTERNAL-FACT","retrieved_at_unix_ms":12345,"truncated":false}})
}
async fn save(s: &Store, claim: &AgentClaim) -> ArtifactReceipt {
    let step = step(
        s,
        claim,
        KnowledgeToolInput::Artifact {
            request: ArtifactToolRequest::Write {
                title: "Synthetic external report".into(),
                body: "SYNTHETIC-EXTERNAL-FACT · https://example.test/report-source".into(),
                payload: ArtifactPayload::Document,
                anchor: None,
            },
        },
        "save",
    )
    .await;
    let KnowledgeToolResult::External(v) = s
        .execute_knowledge_tool(&step, "save".into())
        .await
        .unwrap()
    else {
        panic!("no receipt")
    };
    serde_json::from_value(v).unwrap()
}
async fn withdrawn(s: &Store, u: &UserContext, id: &str) {
    let v = s.artifact(u, id.into(), None).await.unwrap();
    assert!(v.draft.is_none());
    assert!(v.external_evidence.is_empty());
    assert!(
        s.artifact_history(u, id.into(), None)
            .await
            .unwrap()
            .items
            .iter()
            .all(|v| v.draft.is_none())
    );
    assert!(
        s.export_artifact(u, id.into(), v.artifact.revision, false)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn external_report_write_edit_reopen_lost_ack_and_origin_withdrawal_are_one_durable_outcome()
{
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, t) = setup().await;
    let claim = start(&s, &u, &t, true).await;
    external(&s, &claim, web_request(), web_page()).await;
    let r = save(&s, &claim).await; // Deliberately discard any worker finalization/response.
    let other = member(&s, &u, "other-report@example.test").await.user;
    assert!(s.artifact(&other, r.id.clone(), None).await.is_err());
    assert!(
        s.write_artifact(
            &other,
            ArtifactWrite {
                request_id: Uuid::new_v4().to_string(),
                id: None,
                expected_revision: None,
                draft: ArtifactDraft {
                    title: "Wrong owner copy".into(),
                    body: "must not copy".into(),
                    payload: ArtifactPayload::Document,
                    references: vec![ArtifactDependency {
                        kind: ArtifactDependencyKind::Artifact,
                        id: r.id.clone(),
                        revision: 1
                    }]
                }
            }
        )
        .await
        .is_err()
    );

    let v = s.artifact(&u, r.id.clone(), None).await.unwrap();
    assert_eq!(v.external_evidence.len(), 1);
    assert!(v.external_evidence[0].request.contains("report-source"));
    assert_eq!(v.external_evidence[0].result_sha256.len(), 64);
    assert!(
        s.export_artifact(&u, r.id.clone(), 1, false)
            .await
            .unwrap()
            .contains("result SHA256")
    );
    // A CURRENT-user anchored edit can make fresh reads without the old writer
    // invalidating the independent sources used by the replacement revision.
    s.submit_turn_with_artifact(
        &u,
        t.clone(),
        Uuid::new_v4().to_string(),
        "보고서 문서를 새 외부 자료로 수정해 줘".into(),
        Some(ArtifactTarget {
            id: r.id.clone(),
            revision: 1,
        }),
    )
    .await
    .unwrap();
    let editing = s.claim_agent_run().await.unwrap().unwrap();
    let planning = s
        .begin_agent_step(&editing.lease, editing.context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(&planning, checkpoint(), vec![])
        .await
        .unwrap();
    s.prepare_artifact_intent(
        &planning,
        Some(ArtifactIntent {
            action: "edit".into(),
            kind: ArtifactKind::Document,
            intent_quote: "보고서 문서를 새 외부 자료로 수정해 줘".into(),
            target_quote: None,
        }),
    )
    .await
    .unwrap();
    external(&s, &editing, web_request(), web_page()).await;
    let read = step(
        &s,
        &editing,
        KnowledgeToolInput::Artifact {
            request: ArtifactToolRequest::Read {
                id: r.id.clone(),
                revision: 1,
                start_byte: 0,
                max_bytes: 8192,
            },
        },
        "target",
    )
    .await;
    s.execute_knowledge_tool(&read, "target".into())
        .await
        .unwrap();
    let edit = step(
        &s,
        &editing,
        KnowledgeToolInput::Artifact {
            request: ArtifactToolRequest::Write {
                title: v.artifact.title.clone(),
                body: "SYNTHETIC-EXTERNAL-FACT · refreshed current request".into(),
                payload: ArtifactPayload::Document,
                anchor: Some(v.draft.as_ref().unwrap().body.clone()),
            },
        },
        "edit",
    )
    .await;
    let KnowledgeToolResult::External(edited) = s
        .execute_knowledge_tool(&edit, "edit".into())
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(edited["revision"], 2, "{edited}");
    let v = s.artifact(&u, r.id.clone(), None).await.unwrap();
    assert_eq!(v.external_evidence.len(), 2);
    // An editor revision also preserves exact provenance without a source cycle.
    let mut draft = v.draft.unwrap();
    draft.body.push_str(" · explicit owner edit");
    let edited = s
        .write_artifact(
            &u,
            ArtifactWrite {
                request_id: Uuid::new_v4().to_string(),
                id: Some(r.id.clone()),
                expected_revision: Some(2),
                draft,
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.revision, 3);
    assert_eq!(
        s.artifact(&u, r.id.clone(), None)
            .await
            .unwrap()
            .external_evidence
            .len(),
        2
    );
    assert!(
        s.artifact(&u, r.id.clone(), None)
            .await
            .unwrap()
            .external_evidence
            .iter()
            .all(|e| !e.request.is_empty())
    );

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
    assert_eq!(
        s.artifact(&u, r.id.clone(), None)
            .await
            .unwrap()
            .artifact
            .revision,
        3
    );
    assert!(s.claim_agent_run().await.unwrap().is_none()); // no model rerun after lost ACK.
    let child = s
        .write_artifact(
            &u,
            ArtifactWrite {
                request_id: Uuid::new_v4().to_string(),
                id: None,
                expected_revision: None,
                draft: ArtifactDraft {
                    title: "Derived report".into(),
                    body: "SYNTHETIC-EXTERNAL-FACT derived".into(),
                    payload: ArtifactPayload::Document,
                    references: vec![ArtifactDependency {
                        kind: ArtifactDependencyKind::Artifact,
                        id: r.id.clone(),
                        revision: 3,
                    }],
                },
            },
        )
        .await
        .unwrap();
    // Explicit deletion clears private query metadata while preserving receipt hashes.
    let deleted = s
        .write_artifact(
            &u,
            ArtifactWrite {
                request_id: Uuid::new_v4().to_string(),
                id: None,
                expected_revision: None,
                draft: ArtifactDraft {
                    title: "Delete this derived report".into(),
                    body: "SYNTHETIC-EXTERNAL-FACT".into(),
                    payload: ArtifactPayload::Document,
                    references: vec![ArtifactDependency {
                        kind: ArtifactDependencyKind::Artifact,
                        id: r.id.clone(),
                        revision: 3,
                    }],
                },
            },
        )
        .await
        .unwrap();
    s.delete_artifact(
        &u,
        deleted.id.clone(),
        ArtifactDelete {
            request_id: Uuid::new_v4().to_string(),
            expected_revision: 1,
        },
    )
    .await
    .unwrap();
    let tenant = u.tenant.0.clone();
    let deleted_id = deleted.id;
    s.call(move |db| {
        let rows = super::external::evidence(&db.conn, &tenant, &deleted_id, 1)?;
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|e| e.request.is_empty()
            && e.request_sha256.len() == 64
            && e.result_sha256.len() == 64));
        Ok(())
    })
    .await
    .unwrap();
    let tenant = u.tenant.0.clone();
    let origin = claim.lease.run_id().to_string();
    s.call(move |db| {
        let tx = db.conn.transaction()?;
        agent_execution::purge_runs(&tx, &tenant, vec![origin])?;
        tx.commit()?;
        Ok(())
    })
    .await
    .unwrap();
    withdrawn(&s, &u, &r.id).await;
    withdrawn(&s, &u, &child.id).await;
    let tenant = u.tenant.0.clone();
    let parent = r.id.clone();
    let child = child.id;
    s.call(move |db|{
        let rows=db.conn.prepare("SELECT request,request_sha256,result_sha256 FROM artifact_external_evidence WHERE tenant_id=?1 AND artifact_id IN(?2,?3)")?.query_map(params![tenant,parent,child],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|(request,input,result)|request.is_empty() && input.len()==64 && result.len()==64));
        Ok(())
    }).await.unwrap();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    withdrawn(&s, &u, &r.id).await;
}
#[tokio::test]
async fn external_report_prior_completed_read_is_selected_but_thread_delete_withdraws_saved_output()
{
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, t) = setup().await;
    let source = start(&s, &u, &t, false).await;
    external(&s, &source, web_request(), web_page()).await;
    let context = s.agent_context(&source.lease).await.unwrap();
    let st = s
        .begin_agent_step(&source.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.append_agent_output(
        &st,
        vec![crate::OutputPatch::Append {
            item: "read-answer".into(),
            text: "SYNTHETIC-EXTERNAL-FACT from the observed page".into(),
            phase: None,
            refusal: false,
        }],
    )
    .await
    .unwrap();
    s.commit_agent_step(&st, checkpoint(), vec![])
        .await
        .unwrap();
    s.finish_agent_run(&st, RunState::Completed, None)
        .await
        .unwrap();
    let writer = start(&s, &u, &t, true).await;
    let r = save(&s, &writer).await;
    let v = s.artifact(&u, r.id.clone(), None).await.unwrap();
    assert_eq!(v.external_evidence[0].run_id, source.lease.run_id());
    s.delete_thread(&u, t).await.unwrap();
    withdrawn(&s, &u, &r.id).await;
}
fn tokens(scopes: &[&str]) -> moosuri_connectors::gmail::TokenSet {
    moosuri_connectors::gmail::TokenSet {
        access_token: SecretString::new("synthetic-report-access".into()).unwrap(),
        refresh_token: Some(SecretString::new("synthetic-report-refresh".into()).unwrap()),
        expires_in: 3600,
        scopes: Some(scopes.iter().map(|v| v.to_string()).collect()),
    }
}
#[tokio::test]
async fn external_report_connected_reads_are_revoked_by_actual_disconnect_and_late_cancel_cannot_write()
 {
    let _g = TEST_LOCK.lock().await;
    for calendar in [false, true] {
        let (_dir, s, u, t) = setup().await;
        let app = GoogleAppPin {
            revision: None,
            client_id: "synthetic-report.apps.googleusercontent.com".into(),
            redirect_uri: "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
        };
        let (id, request, result) = if calendar {
            let start = s.start_calendar_oauth(&u, app.clone()).await.unwrap();
            let handle = s
                .capture_calendar_oauth(
                    start.state,
                    Some(SecretString::new("synthetic-code".into()).unwrap()),
                )
                .await
                .unwrap()
                .unwrap();
            let ex = s.claim_calendar_oauth(&u, handle, app).await.unwrap();
            let c = s
                .complete_calendar_oauth(
                    ex,
                    tokens(&moosuri_connectors::calendar::SCOPES),
                    CalendarInfo {
                        id: "owner@example.test".into(),
                        title: "Synthetic primary".into(),
                        time_zone: "UTC".into(),
                        primary: true,
                        access_role: "owner".into(),
                        email_notifications: false,
                    },
                )
                .await
                .unwrap();
            let id = c.id;
            (
                id.clone(),
                ExternalToolRequest::Calendar(CalendarReadRequest::List {
                    range: CalendarRange {
                        connection_id: id,
                        start: "2026-12-01T00:00:00Z".into(),
                        end: "2026-12-02T00:00:00Z".into(),
                        page_token: None,
                    },
                }),
                serde_json::to_value(CalendarEvents {
                    events: vec![],
                    partial: false,
                    next_page_token: None,
                    primary_id: "owner@example.test".into(),
                })
                .unwrap(),
            )
        } else {
            let start = s.start_mail_oauth(&u).await.unwrap();
            let handle = s
                .capture_mail_oauth(
                    start.state,
                    SecretString::new("synthetic-code".into()).unwrap(),
                )
                .await
                .unwrap();
            let ex = s.claim_mail_oauth(&u, handle).await.unwrap();
            let c = s
                .complete_mail_oauth(
                    ex,
                    tokens(&moosuri_connectors::gmail::MINIMAL_SCOPES),
                    "owner@example.test".into(),
                )
                .await
                .unwrap();
            let id = c.id;
            (
                id.clone(),
                ExternalToolRequest::Mail(MailToolRequest::Search {
                    connection_id: id,
                    query: "subject:synthetic".into(),
                    page_token: None,
                }),
                serde_json::json!({"messages":[],"next_page_token":null}),
            )
        };
        let claim = start(&s, &u, &t, true).await;
        external(&s, &claim, request.clone(), result.clone()).await;
        let r = save(&s, &claim).await;
        if calendar {
            s.disconnect_calendar(&u, id.clone()).await.unwrap()
        } else {
            s.disconnect_mail(&u, id.clone()).await.unwrap()
        }
        withdrawn(&s, &u, &r.id).await;
        // A new explicit request may be planned, but a held read after Cancel cannot
        // settle a tool receipt or reach the write even without another provider call.
        let late = start(&s, &u, &t, true).await;
        let st = step(
            &s,
            &late,
            KnowledgeToolInput::External {
                request: web_request(),
            },
            "late",
        )
        .await;
        let ExternalToolStart::Dispatch(p) =
            s.prepare_external_tool(&st, "late".into()).await.unwrap()
        else {
            panic!()
        };
        s.cancel_run(&u, late.lease.run_id().into(), 1)
            .await
            .unwrap();
        assert!(
            s.complete_external_tool(&p, KnowledgeToolResult::External(web_page()))
                .await
                .is_err()
        );
        assert_eq!(s.artifacts(&u, None, None).await.unwrap().items.len(), 0);
    }
}

#[tokio::test]
async fn external_report_restore_quarantine_erases_derived_versions_before_reopen() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, t) = setup().await;
    let claim = start(&s, &u, &t, true).await;
    external(&s, &claim, web_request(), web_page()).await;
    let r = save(&s, &claim).await;
    s.call(|db| {
        let tx = db.conn.transaction()?;
        super::external::quarantine_restore(&tx)?;
        tx.commit()?;
        Ok(())
    })
    .await
    .unwrap();
    withdrawn(&s, &u, &r.id).await;
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    withdrawn(&s, &u, &r.id).await;
    assert!(
        s.conversation(&u, t, None)
            .await
            .unwrap()
            .turns
            .iter()
            .all(|v| v.assistant.is_empty())
    );
}
