//! Actual loopback provider HTML -> bounded connector parser -> typed Store/tool
//! loop -> final chat/source receipt. All parcel values are synthetic, never live.
use super::knowledge::{result_values, tool_call, tool_configured};
use super::*;
use axum::{extract::Query, http::HeaderMap};
use moosuri_connectors::web::{WebClient, WebConfig};
use moosuri_core::WebToolProfile;
const NUMBER: &str = "00000000000";
const NUMBER_B: &str = "00000000001";
fn html(state: usize, number: &str) -> String {
    if state == 2 {
        return "<html><title>Provider changed</title><p>new layout, no established table</p></html>".into();
    }
    let row = if state == 0 || state == 3 {
        format!(
            "<td>{number}</td><td>Synthetic parcel</td><td>2026/11/14</td><td>SYNTHETIC_OFFICIAL_VISIBLE_STATUS</td><td>Synthetic office</td><td>Synthetic prefecture</td>"
        )
    } else {
        format!(
            "<td>{number}</td><td colspan='5'>** Your item was not found. Confirm your item number and ask at your local office.</td>"
        )
    };
    format!(
        "<html><title>Individual Search Results - Japan Post</title><table><tr><th>Item number</th><th>Class of goods</th><th>Latest date</th><th>Latest status</th><th>Latest handling office</th><th>Prefecture</th></tr><tr><th>ZIP code（Postal code number）</th></tr><tr>{row}</tr></table></html>"
    )
}
#[tokio::test]
async fn parcel_tracking_actual_provider_tool_loop_reuses_only_same_thread_receipt_and_reports_changed_notfound()
 {
    let _g = TEST_LOCK.lock().await;
    let provider_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/services/srv/search",
        provider_listener.local_addr().unwrap()
    );
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let counted = provider_calls.clone();
    let provider = Router::new().route(
        "/services/srv/search",
        axum::routing::get(
            move |headers: HeaderMap,
                  Query(q): Query<std::collections::HashMap<String, String>>| {
                let count = counted.clone();
                async move {
                    assert!(headers.get("authorization").is_none());
                    assert!(headers.get("cookie").is_none());
                    assert_eq!(q.get("locale").unwrap(), "en");
                    let n = count.fetch_add(1, Ordering::SeqCst);
                    assert!(n < 4);
                    let expected = if n == 3 { NUMBER_B } else { NUMBER };
                    assert_eq!(
                        q.get("requestNo1").unwrap(),
                        expected,
                        "a current explicit B must never dispatch prior A"
                    );
                    (
                        [(axum::http::header::CONTENT_TYPE, "text/html; charset=UTF-8")],
                        html(n, expected),
                    )
                }
            },
        ),
    );
    let provider_task =
        tokio::spawn(async move { axum::serve(provider_listener, provider).await.unwrap() });
    let model_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", model_listener.local_addr().unwrap());
    let model_calls = Arc::new(AtomicUsize::new(0));
    let calls = model_calls.clone();
    let results = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
    let saved = results.clone();
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(input):axum::Json<Value>|{let calls=calls.clone();let saved=saved.clone();async move {
        let names=input["tools"].as_array().unwrap();
        let memory=names.iter().any(|v|v["function"]["name"]=="memory_command_decision");let mission=names.iter().any(|v|v["function"]["name"]=="mission_chat_decision");
        if memory || mission {
            let (function,d)=if memory {("memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null}))}else{("mission_chat_decision",json!({"decision":"no_command","action":null,"target_kind":null,"intent_quote":null,"task_quote":null,"constraints_quote":null,"time_quote":null,"target_quote":null,"instruction_quote":null,"time":null,"clarification":null}))};
            let body=frame(json!({"id":"parcel-no-command","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[tool_call(0,"classification".into(),function,d)]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":5,"completion_tokens":2}}))+"data: [DONE]\n\n";
            return Response::builder().header("content-type","text/event-stream").body(Body::from(body)).unwrap()
        }
        let n=calls.fetch_add(1,Ordering::SeqCst);assert!(n<10,"one ordinary typed tool round and final per turn");
        assert!(input["tools"].as_array().unwrap().iter().any(|v|v["function"]["name"]=="parcel_track"));
        let (delta,reason)=if n%2==0 {(json!({"role":"assistant","tool_calls":[tool_call(0,format!("parcel-{n}"),"parcel_track",json!({"carrier":"japan_post","tracking_number":if n==0{Some(NUMBER)}else{None}}))]}),"tool_calls")}
        else {let values=result_values(&input);let value=values.last().unwrap().clone();saved.lock().await.push(value.clone());let text=match n {1=>{assert!(value.to_string().contains("SYNTHETIC_OFFICIAL_VISIBLE_STATUS"),"actual synthetic tool result: {value}");"SYNTHETIC_OFFICIAL_VISIBLE_STATUS is the latest visible Japan Post summary, dated 2026/11/14. Full history and timezone are unavailable."},3=>{assert!(value.to_string().contains("not_found"));"Japan Post currently reports not found for that same tracking number."},5=>{assert!(value.to_string().contains("provider_changed"));"Japan Post returned an unrecognized layout; I could not establish a current parcel status."},7=>{assert_eq!(value.pointer("/value/parcel/tracking_number").unwrap(),NUMBER_B);assert_eq!(value.pointer("/value/parcel/state").unwrap(),"observed");"The fresh official summary is for the newly supplied number B, not the old parcel A."},_=>{assert!(value.to_string().contains("invalid_input"));"Which Japan Post tracking number should I check in this new conversation?"}};(json!({"role":"assistant","content":text}),"stop")};
        let body=frame(json!({"id":format!("parcel-round-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";
        Response::builder().header("content-type","text/event-stream").body(Body::from(body)).unwrap()
    }}));
    let model_task = tokio::spawn(async move { axum::serve(model_listener, app).await.unwrap() });
    let (_dir, store, user, thread) = tool_configured(&base).await;
    drop(store);
    let store = Store::open_with_web(
        _dir.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    let client = WebClient::new(WebConfig::default())
        .unwrap()
        .with_parcel_test_endpoint(&endpoint)
        .unwrap();
    let runtime = Runtime::start_with_web_service(
        store.clone(),
        None,
        crate::web::Service::new(Some(Arc::new(client))),
    );
    let mut requests = vec![];
    for text in [
        format!("일본우편 {NUMBER} 배송 조회해줘"),
        "그 택배 지금 어디야?".into(),
        "그 택배 다시 조회해줘".into(),
        format!("일본우편 {NUMBER_B} 배송 조회해줘"),
    ] {
        let request = uuid::Uuid::new_v4().to_string();
        requests.push(request.clone());
        store
            .submit_turn_with_schedule_context(&user, thread.clone(), request, text, None, None)
            .await
            .unwrap();
        runtime.wake();
        let turn = wait_for(&store, &user, &thread, |v| !v.run.state.active()).await;
        assert_eq!(turn.run.state, RunState::Completed, "{:?}", turn.run.error);
        let evidence = turn.run.web_evidence.as_ref().unwrap();
        assert_eq!(evidence.len(), 1);
        assert_eq!(
            evidence[0].url,
            moosuri_connectors::parcel::url(if requests.len() == 4 {
                NUMBER_B
            } else {
                NUMBER
            })
            .unwrap()
            .to_string()
        );
    }
    assert_eq!(provider_calls.load(Ordering::SeqCst), 4);
    let observed = results.lock().await.clone();
    for (n, v) in observed.iter().enumerate() {
        let provenance = v.pointer("/value/parcel/provenance").unwrap();
        assert_eq!(
            provenance["number_source_request_id"],
            requests[if n == 3 { 3 } else { 0 }]
        );
        assert_eq!(provenance["current_request_id"], requests[n]);
        assert_eq!(provenance["used_previous_number"], matches!(n, 1 | 2));
    }
    let foreign = store
        .create_thread(&user, "Synthetic separate tracking thread".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &user,
            foreign.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            "그 택배 지금 어디야?".into(),
        )
        .await
        .unwrap();
    runtime.wake();
    let done = wait_for(&store, &user, &foreign.id, |v| !v.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    assert!(done.run.web_evidence.is_none());
    assert_eq!(
        provider_calls.load(Ordering::SeqCst),
        4,
        "no number leaks across threads"
    );
    runtime.shutdown().await;
    provider_task.abort();
    model_task.abort();
}
