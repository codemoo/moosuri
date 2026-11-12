//! Scripted model uses the real runtime/store; the web IO seam is cfg(test) only.
//! Real public HTTP parsing and network policy have separate connector tests.
use super::knowledge::{result_values, tool_call, tool_configured};
use super::*;
use axum::{
    http::{StatusCode, header},
    response::IntoResponse,
};
use moosuri_core::{WebToolProfile, WebToolRequest};
use moosuri_store::{KnowledgeToolResult, ToolReadError};

async fn fixture_model() -> (String, JoinHandle<()>) {
    fixture_model_for(false).await
}
async fn fixture_model_for(browser: bool) -> (String, JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/chat/completions",post(move |axum::Json(input):axum::Json<Value>|async move {
        let planning=input["tools"][0]["function"]["name"]=="memory_command_decision";
        let values=result_values(&input);let finished=values.iter().any(|v|v.pointer("/kind").and_then(Value::as_str)==Some("external"));
        let delta=if planning {json!({"tool_calls":[tool_call(0,"decision".into(),"memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null}))]})}
        else if finished {assert!(input.to_string().contains("ACTUAL_FIXTURE_HTTP_TEXT"));json!({"content":if browser {"ACTUAL_FIXTURE_HTTP_TEXT came from the rendered browser fixture."}else{"The observed public page says ACTUAL_FIXTURE_HTTP_TEXT. It is an HTTP excerpt."}})}
        else {let tools=input["tools"].as_array().unwrap();if browser {assert!(tools.iter().any(|v|v["function"]["name"]=="browser_read"));}assert!(tools.iter().any(|v|v["function"]["name"]=="web_read"));assert!(!tools.iter().any(|v|v["function"]["name"]=="web_search"));json!({"tool_calls":[tool_call(0,"web-1".into(),if browser {"browser_read"}else{"web_read"},json!({"url":"https://example.com/observed"}))]})};
        let body=frame(json!({"id":"web-round","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":if finished{"stop"}else{"tool_calls"}}],"usage":{"prompt_tokens":11,"completion_tokens":3}}))+"data: [DONE]\n\n";
        (StatusCode::OK,[(header::CONTENT_TYPE,"text/event-stream")],body).into_response()
    }));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, task)
}
async fn configured_web(base: &str) -> (tempfile::TempDir, Store, UserContext, String) {
    let (dir, store, user, thread) = tool_configured(base).await;
    drop(store);
    let store = Store::open_with_web(
        dir.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    (dir, store, user, thread)
}
fn web_service(
    held: bool,
    calls: Arc<AtomicUsize>,
    entered: Arc<Semaphore>,
) -> crate::web::Service {
    crate::web::Service::fixture(Arc::new(move |request, cancel| {
        let calls = calls.clone();
        let entered = entered.clone();
        Box::pin(async move {
            assert!(
                matches!(request,WebToolRequest::Read{ref url} if url=="https://example.com/observed")
            );
            calls.fetch_add(1, Ordering::SeqCst);
            entered.add_permits(1);
            if held {
                cancel.cancelled().await;
                return KnowledgeToolResult::Error(ToolReadError::Unavailable);
            }
            KnowledgeToolResult::External(
                json!({"web_page":{"url":"https://example.com/observed","title":"Fixture public page","text":"ACTUAL_FIXTURE_HTTP_TEXT","retrieved_at_unix_ms":123456,"truncated":false,"warning":"http_only; no_javascript"}}),
            )
        })
    }))
}
#[tokio::test]
async fn scripted_model_external_permit_result_final_and_observed_links_roundtrip() {
    let _g = TEST_LOCK.lock().await;
    let (base, server) = fixture_model().await;
    let (_dir, store, user, thread) = configured_web(&base).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let runtime = Runtime::start_with_web_service(
        store.clone(),
        None,
        web_service(false, calls.clone(), Arc::new(Semaphore::new(0))),
    );
    store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "Read this public page.".into(),
        )
        .await
        .unwrap();
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |t| !t.run.state.active()).await;
    assert_eq!(
        done.run.state,
        RunState::Completed,
        "error={:?} web_calls={}",
        done.run.error,
        calls.load(Ordering::SeqCst)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        done.assistant
            .iter()
            .any(|v| v.text.contains("ACTUAL_FIXTURE_HTTP_TEXT"))
    );
    let refs = done.run.web_evidence.unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].url, "https://example.com/observed");
    assert_eq!(refs[0].title, "Fixture public page");
    runtime.shutdown().await;
    server.abort();
}
#[tokio::test]
async fn held_web_io_leaves_store_free_and_cancel_logout_or_thread_delete_reject_late_result() {
    let _g = TEST_LOCK.lock().await;
    for operation in ["cancel", "logout", "delete"] {
        let (base, server) = fixture_model().await;
        let (_dir, store, user, thread) = configured_web(&base).await;
        let entered = Arc::new(Semaphore::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let runtime = Runtime::start_with_web_service(
            store.clone(),
            None,
            web_service(true, calls.clone(), entered.clone()),
        );
        let turn = store
            .submit_turn(
                &user,
                thread.clone(),
                uuid::Uuid::new_v4().to_string(),
                "Read this public page.".into(),
            )
            .await
            .unwrap();
        runtime.wake();
        tokio::time::timeout(Duration::from_secs(5), entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        // This completes while the external future is still held: no SQLite actor lock spans IO.
        tokio::time::timeout(
            Duration::from_secs(1),
            store.create_thread(&user, "actor remains available".into()),
        )
        .await
        .unwrap()
        .unwrap();
        match operation {
            "cancel" => {
                store
                    .cancel_run(&user, turn.run.id.clone(), 1)
                    .await
                    .unwrap();
            }
            "logout" => {
                store.logout(&user).await.unwrap();
            }
            _ => {
                store.delete_thread(&user, thread.clone()).await.unwrap();
            }
        }
        runtime.wake();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if operation == "cancel" {
            let page = store
                .conversation(&user, thread.clone(), None)
                .await
                .unwrap();
            assert!(page.turns[0].run.web_evidence.is_none());
            assert!(page.turns[0].assistant.is_empty());
        }
        runtime.shutdown().await;
        server.abort();
    }
}

#[tokio::test]
async fn browser_v4_ordinary_model_loop_permit_evidence_and_cleanup_on_revocation() {
    let _g = TEST_LOCK.lock().await;
    for held in [false, true] {
        let (base, server) = fixture_model_for(true).await;
        let (dir, store, user, thread) = tool_configured(&base).await;
        drop(store);
        let store = Store::open_with_web(
            dir.path(),
            WebToolProfile {
                read: true,
                search: false,
                browser_pin: Some("synthetic-deployment-pin".into()),
            },
        )
        .unwrap();
        let entered = Arc::new(Semaphore::new(0));
        let cleaned = Arc::new(Semaphore::new(0));
        let service = crate::web::Service::fixture(Arc::new({
            let entered = entered.clone();
            let cleaned = cleaned.clone();
            move |request, cancel| {
                let entered = entered.clone();
                let cleaned = cleaned.clone();
                Box::pin(async move {
                    assert!(matches!(request, WebToolRequest::BrowserRead { .. }));
                    entered.add_permits(1);
                    if held {
                        cancel.cancelled().await;
                        tokio::time::sleep(Duration::from_millis(80)).await;
                    }
                    cleaned.add_permits(1);
                    KnowledgeToolResult::External(
                        json!({"browser_page":{"url":"https://example.com/rendered","title":"Rendered browser fixture","text":"ACTUAL_FIXTURE_HTTP_TEXT","retrieved_at_unix_ms":34567,"truncated":true,"warning":"isolated_browser; javascript_disabled"}}),
                    )
                })
            }
        }));
        let runtime = Runtime::start_with_web_service(store.clone(), None, service);
        let turn = store
            .submit_turn(
                &user,
                thread.clone(),
                uuid::Uuid::new_v4().to_string(),
                "Read rendered public page".into(),
            )
            .await
            .unwrap();
        runtime.wake();
        tokio::time::timeout(Duration::from_secs(5), entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        if held {
            store.cancel_run(&user, turn.run.id, 1).await.unwrap();
            runtime.wake();
        }
        let done = wait_for(&store, &user, &thread, |t| !t.run.state.active()).await;
        if held {
            assert!(done.run.web_evidence.is_none());
            assert!(done.assistant.is_empty());
            tokio::time::timeout(Duration::from_secs(2), cleaned.acquire())
                .await
                .unwrap()
                .unwrap()
                .forget();
        } else {
            assert_eq!(done.run.state, RunState::Completed);
            let refs = done.run.web_evidence.unwrap();
            assert_eq!(refs.len(), 1);
            assert!(matches!(
                refs[0].kind,
                moosuri_core::WebEvidenceKind::BrowserPage
            ));
            assert_eq!(refs[0].url, "https://example.com/rendered");
            assert!(refs[0].truncated);
        }
        runtime.shutdown().await;
        server.abort();
    }
}
