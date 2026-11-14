//! Real Runtime timing observations with no real provider or personal content.
use super::*;
#[tokio::test]
async fn run_timings_runtime_stream_tools_truncation_and_cancel_keep_real_denominators() {
    let _g = TEST_LOCK.lock().await;
    // The compatible protocol has no authoritative phase: even successful text
    // must stay outside explicit-final TTFT samples.
    let f = fixture().await;
    f.state.release.add_permits(1);
    let (_d, s, u, thread) = configured(&f.base).await;
    let runtime = Runtime::start(s.clone());
    submit(&s, &u, &thread).await;
    runtime.wake();
    let done = wait_for(&s, &u, &thread, |v| !v.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    let timing = done.run.timings.unwrap();
    assert!(timing.model_ms.is_some() && timing.terminal_ms.is_some());
    assert_eq!(timing.first_final_body_ms, None);
    assert_eq!(timing.model_calls, 1);
    assert_eq!(done.run.usage.output_tokens, Some(4));
    runtime.shutdown().await;
    // A real truncated unphased compatible stream is not a final-answer sample.
    let f = fixture().await;
    f.state.mode.store(6, Ordering::SeqCst);
    f.state.release.add_permits(1);
    let (_d, s, u, thread) = configured(&f.base).await;
    let runtime = Runtime::start(s.clone());
    submit(&s, &u, &thread).await;
    runtime.wake();
    let failed = wait_for(&s, &u, &thread, |v| !v.run.state.active()).await;
    assert_eq!(failed.run.state, RunState::OutcomeUnknown);
    assert!(failed.run.timings.as_ref().unwrap().model_ms.is_some());
    assert_eq!(
        failed.run.timings.as_ref().unwrap().first_final_body_ms,
        None
    );
    let aggregate = s.usage_summary(&u).await.unwrap().timings.unwrap();
    assert_eq!(aggregate.outcomes.get("outcome_unknown"), Some(&1));
    assert_eq!(aggregate.first_final_body.missing, 1);
    runtime.shutdown().await;
    // Actual local tool work contributes its own span without making an unphased
    // compatible answer into a final-body sample.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/chat/completions",post(|axum::Json(body):axum::Json<Value>|async move {
        let output=if body["messages"].as_array().unwrap().last().unwrap()["role"]=="tool" {
            json!({"id":"tool-timing","model":"fixture-model","choices":[{"index":0,"delta":{"content":"3"},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":1}})
        }else{
            json!({"id":"tool-timing","model":"fixture-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"sum","type":"function","function":{"name":"decimal_calculate","arguments":"{\"operation\":\"sum\",\"operands\":[\"1\",\"2\"]}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":8,"completion_tokens":1}})
        };
        Response::builder().header("content-type","text/event-stream").body(Body::from(frame(output)+"data: [DONE]\n\n")).unwrap()
    }));
    let provider = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_d, s, u, thread) = super::knowledge::tool_configured(&base).await;
    let runtime = Runtime::start(s.clone());
    submit(&s, &u, &thread).await;
    runtime.wake();
    let done = wait_for(&s, &u, &thread, |v| !v.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    let timing = done.run.timings.unwrap();
    assert_eq!(timing.tool_calls, 1);
    assert_eq!(timing.measured_tool_calls, 1);
    assert!(timing.tool_ms.is_some());
    assert_eq!(timing.model_calls, 2);
    assert_eq!(timing.first_final_body_ms, None);
    assert_eq!(
        s.usage_summary(&u)
            .await
            .unwrap()
            .timings
            .unwrap()
            .tool
            .samples,
        1
    );
    runtime.shutdown().await;
    provider.abort();
    // Cancellation retains an outcome denominator even if its stream was not measured to completion.
    let f = fixture().await;
    let (_d, s, u, thread) = configured(&f.base).await;
    let runtime = Runtime::start(s.clone());
    let admitted = submit(&s, &u, &thread).await;
    runtime.wake();
    wait_for(&s, &u, &thread, |v| !v.assistant.is_empty()).await;
    s.cancel_run(&u, admitted.run.id, 1).await.unwrap();
    f.state.release.add_permits(1);
    runtime.shutdown().await;
    let summary = s.usage_summary(&u).await.unwrap().timings.unwrap();
    assert_eq!(summary.outcomes.get("cancelled"), Some(&1));
    assert_eq!(summary.first_final_body.samples, 0);
    assert_eq!(summary.first_final_body.missing, 1);
}
