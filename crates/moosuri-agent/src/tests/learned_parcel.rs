//! Actual synthetic HTML -> parser -> durable typed receipts -> learned fresh IO.
use super::knowledge::{result_values, tool_call, tool_configured};
use super::*;
use axum::{extract::Query, http::HeaderMap};
use moosuri_connectors::web::{WebClient, WebConfig};
use moosuri_core::WebToolProfile;
const NUMBERS: [&str; 3] = ["AA123456789JP", "BB123456789JP", "CC123456789JP"];
#[tokio::test]
async fn learned_parcel_actual_provider_receipts_promote_then_dispatch_fresh_current_number() {
    let _g = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "http://{}/services/srv/search",
        listener.local_addr().unwrap()
    );
    let seen = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let calls = seen.clone();
    let provider=Router::new().route("/services/srv/search",axum::routing::get(move|headers:HeaderMap,Query(q):Query<std::collections::HashMap<String,String>>|{let calls=calls.clone();async move{
        assert!(headers.get("cookie").is_none()&&headers.get("authorization").is_none());
        let number=q["requestNo1"].clone();let mut seen=calls.lock().await;let at=seen.len();assert_eq!(number,NUMBERS[at]);seen.push(number.clone());
        let html=format!("<html><title>Japan Post synthetic</title><table><tr><th>Item number</th><th>Class of goods</th><th>Latest date</th><th>Latest status</th><th>Latest handling office</th><th>Prefecture</th></tr><tr><th>ZIP code（Postal code number）</th></tr><tr><td>{number}</td><td>Synthetic parcel</td><td>2026/11/15</td><td>FRESH_PROVIDER_STATUS_{at}</td><td>Synthetic office</td><td>Synthetic region</td></tr></table></html>");
        ([(axum::http::header::CONTENT_TYPE,"text/html; charset=UTF-8")],html)
    }}));
    let provider_task = tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let learned_calls = Arc::new(AtomicUsize::new(0));
    let counted = learned_calls.clone();
    let model=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let counted=counted.clone();async move {
        let tools=body["tools"].as_array().unwrap();
        let memory=tools.iter().any(|v|v["function"]["name"]=="memory_command_decision");let mission=tools.iter().any(|v|v["function"]["name"]=="mission_chat_decision");
        let (delta,reason)=if memory||mission {
            let (function,d)=if memory {("memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null}))}else{("mission_chat_decision",json!({"decision":"no_command","action":null,"target_kind":null,"intent_quote":null,"task_quote":null,"constraints_quote":null,"time_quote":null,"target_quote":null,"instruction_quote":null,"time":null,"clarification":null}))};
            (json!({"role":"assistant","tool_calls":[tool_call(0,"no-command".into(),function,d)]}),"tool_calls")
        }else{
            let user=body["messages"].as_array().unwrap().iter().rev().find(|v|v["role"]=="user").unwrap()["content"].as_str().unwrap();
            let number=*NUMBERS.iter().find(|n|user.contains(**n)).unwrap();
            let prior=result_values(&body);
            if let Some(value)=prior.last() {
                let p=&value["value"]["parcel"];assert_eq!(p["tracking_number"],number);assert_eq!(p["state"],"observed");
                let event=&p["events"][0];let answer=format!("{}: {} {} {} (latest summary only)",number,event["reported_time_text"].as_str().unwrap(),event["status_text"].as_str().unwrap(),p["source"]["url"].as_str().unwrap());
                (json!({"role":"assistant","content":answer}),"stop")
            }else if number==NUMBERS[2] {
                let system=body["messages"][0]["content"].as_str().unwrap();let metadata=system.lines().find(|v|v.starts_with("private:")).unwrap();let parts=metadata.split_whitespace().collect::<Vec<_>>();
                let pin=json!({"id":parts[0],"revision":parts[1].strip_prefix("revision=").unwrap(),"digest":parts[2].strip_prefix("digest=").unwrap().trim_end_matches(':')});
                counted.fetch_add(1,Ordering::SeqCst);
                (json!({"role":"assistant","tool_calls":[tool_call(0,"fresh-C".into(),"learned_skill_execute",json!({"pin":pin,"arguments":{"kind":"parcel","tracking_number":number,"text":null,"operands":null,"inputs":null}}))]}),"tool_calls")
            }else{(json!({"role":"assistant","tool_calls":[tool_call(0,"direct".into(),"parcel_track",json!({"carrier":"japan_post","tracking_number":number}))]}),"tool_calls")}
        };
        let stream=frame(json!({"id":"learned-parcel","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";
        Response::builder().header("content-type","text/event-stream").body(Body::from(stream)).unwrap()
    }}));
    let model_task = tokio::spawn(async move { axum::serve(listener, model).await.unwrap() });
    let (dir, store, u, _) = tool_configured(&base).await;
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
    let client = WebClient::new(WebConfig::default())
        .unwrap()
        .with_parcel_test_endpoint(&endpoint)
        .unwrap();
    let runtime = Runtime::start_with_web_service(
        store.clone(),
        None,
        crate::web::Service::new(Some(Arc::new(client))),
    );
    let mut final_thread = String::new();
    for (at, number) in NUMBERS.iter().enumerate() {
        let thread = store
            .create_thread(&u, format!("Synthetic parcel {at}"))
            .await
            .unwrap()
            .id;
        store
            .submit_turn(
                &u,
                thread.clone(),
                uuid::Uuid::new_v4().to_string(),
                format!("Japan Post {number} latest parcel status please"),
            )
            .await
            .unwrap();
        runtime.wake();
        let done = wait_for(&store, &u, &thread, |v| !v.run.state.active()).await;
        assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
        assert!(
            done.assistant
                .iter()
                .any(|v| v.text.contains(&format!("FRESH_PROVIDER_STATUS_{at}")))
        );
        assert_eq!(done.run.web_evidence.as_ref().unwrap().len(), 1);
        let cards = store.learned_skills(&u).await.unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0].readiness,
            if at == 0 { "candidate" } else { "ready" }
        );
        final_thread = thread;
    }
    assert_eq!(*seen.lock().await, NUMBERS);
    assert_eq!(learned_calls.load(Ordering::SeqCst), 1);
    let card = store.learned_skills(&u).await.unwrap().remove(0);
    store
        .mutate_learned_skill(
            &u,
            card.id,
            moosuri_core::CommunitySkillMutation {
                action: "disable".into(),
                expected_revision: card.revision,
                expected_digest: None,
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .conversation(&u, final_thread, None)
            .await
            .unwrap()
            .turns[0]
            .assistant
            .is_empty()
    );
    runtime.shutdown().await;
    model_task.abort();
    provider_task.abort();
}
