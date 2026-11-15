//! Real supervisor/model protocol/Store, synthetic trusted HTTP-read seam only.
use super::knowledge::{result_values, tool_call, tool_configured};
use super::*;
use moosuri_core::{MissionCheck, MissionCreate, MissionMode, WebToolProfile, WebToolRequest};
use moosuri_store::KnowledgeToolResult;
fn urls(label: &str) -> Vec<String> {
    vec![
        format!("https://example.com/{label}-one"),
        format!("https://example.org/{label}-two"),
    ]
}
fn request(urls: &[String]) -> String {
    format!(
        "Collect and compare public pages {}",
        urls.iter()
            .map(|u| format!("`{u}`"))
            .collect::<Vec<_>>()
            .join(" ")
    )
}
fn answer(value: &Value) -> String {
    value["value"]["public_pages"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let page = &v["result"]["web_page"];
            format!(
                "{} {} {}",
                page["title"].as_str().unwrap(),
                page["url"].as_str().unwrap(),
                page["text"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[tokio::test]
async fn runtime_public_page_collection_learns_then_reads_fresh_foreground_and_explicit_mission_targets()
 {
    let _g = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let learned = Arc::new(AtomicUsize::new(0));
    let counted = learned.clone();
    let model=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let counted=counted.clone();async move {
        let tools=body["tools"].as_array();let system=body["messages"][0]["content"].as_str().unwrap();
        let name=tools.and_then(|t|t.first()).and_then(|v|v["function"]["name"].as_str());
        let mission_context=serde_json::from_str::<Value>(body["messages"][1]["content"].as_str().unwrap()).ok().filter(|v|v["accepted"].is_object());
        let (delta,reason)=if name==Some("memory_command_decision") {
            (json!({"role":"assistant","tool_calls":[tool_call(0,"none".into(),"memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null}))]}),"tool_calls")
        }else if tools.is_none_or(|t|t.is_empty()) && mission_context.is_some() {
            (json!({"role":"assistant","content":json!({"steps":["Read current supplied pages","Compare observed evidence"]}).to_string()}),"stop")
        }else {
            assert!(tools.unwrap().iter().any(|v|v["function"]["name"]=="public_pages_collect"));
            let current=mission_context.as_ref().map(|v|v["accepted"]["task"].as_str().unwrap()).unwrap_or_else(||body["messages"].as_array().unwrap().iter().rev().find(|v|v["role"]=="user").unwrap()["content"].as_str().unwrap());
            let label=["A-private","B-private","C-fresh","D-mission"].into_iter().find(|v|current.contains(v)).unwrap();let current_urls=urls(label);
            let prior=if let Some(ctx)=&mission_context {
                ctx["receipts"].as_array().unwrap().iter().filter(|v| !v["result"].is_null()).map(|v|v["result"].clone()).collect::<Vec<_>>()
            }else{result_values(&body)};
            if let Some(value)=prior.iter().find(|v|v["value"]["public_pages"].is_object()) {
                let text=answer(value);
                let content=if let Some(ctx)=mission_context {let receipt=&ctx["receipts"].as_array().unwrap()[0];json!({"text":text,"evidence":[{"step":receipt["step"],"call_id":receipt["call_id"]}]}).to_string()}else{text};
                (json!({"role":"assistant","content":content}),"stop")
            }else if label.starts_with('C') || label.starts_with('D') {
                assert!(!body.to_string().contains("https://example.com/A-private") && !body.to_string().contains("https://example.com/B-private"));
                let pin=if mission_context.is_some() {
                    let raw=system.split(" Admitted recipe metadata: ").nth(1).unwrap();let pins:Value=serde_json::from_str(raw).unwrap();pins.as_array().unwrap().iter().find(|p|p["id"].as_str().unwrap().starts_with("private:")).unwrap().clone()
                }else{
                    let metadata=system.lines().find(|v|v.starts_with("private:")).unwrap();let parts=metadata.split_whitespace().collect::<Vec<_>>();json!({"id":parts[0],"revision":parts[1].strip_prefix("revision=").unwrap(),"digest":parts[2].strip_prefix("digest=").unwrap().trim_end_matches(':')})
                };
                counted.fetch_add(1,Ordering::SeqCst);
                (json!({"role":"assistant","tool_calls":[tool_call(0,"fresh-collection".into(),"learned_skill_execute",json!({"pin":pin,"arguments":{"kind":"pages","urls":current_urls,"text":null,"operands":null,"inputs":null,"tracking_number":null,"parameters":null}}))]}),"tool_calls")
            }else{
                (json!({"role":"assistant","tool_calls":[tool_call(0,"native-collection".into(),"public_pages_collect",json!({"urls":current_urls}))]}),"tool_calls")
            }
        };
        let frames=frame(json!({"id":"page-round","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";
        Response::builder().header("content-type","text/event-stream").body(Body::from(frames)).unwrap()
    }}));
    let model_task = tokio::spawn(async move { axum::serve(listener, model).await.unwrap() });
    let (dir, s, u, _) = tool_configured(&base).await;
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
    let seen = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let observed = seen.clone();
    let web = crate::web::Service::fixture(Arc::new(move |request, _| {
        let observed = observed.clone();
        Box::pin(async move {
            let WebToolRequest::Read { url } = request else {
                panic!("not a public HTTP read")
            };
            observed.lock().await.push(url.clone());
            KnowledgeToolResult::External(
                json!({"web_page":{"url":url,"title":format!("Observed {url}"),"text":format!("Actual fresh observed evidence from {url}"),"retrieved_at_unix_ms":12345,"truncated":false}}),
            )
        })
    }));
    let runtime = Runtime::start_with_web_service(s.clone(), None, web);
    for (i, label) in ["A-private", "B-private", "C-fresh"]
        .into_iter()
        .enumerate()
    {
        let t = s.create_thread(&u, label.into()).await.unwrap().id;
        s.submit_turn(
            &u,
            t.clone(),
            uuid::Uuid::new_v4().to_string(),
            request(&urls(label)),
        )
        .await
        .unwrap();
        runtime.wake();
        let done = wait_for(&s, &u, &t, |v| !v.run.state.active()).await;
        assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
        assert_eq!(done.run.web_evidence.as_ref().unwrap().len(), 2);
        assert!(done.assistant.iter().any(|v| v.text.contains(label)));
        let cards = s.learned_skills(&u).await.unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0].readiness,
            if i == 0 { "candidate" } else { "ready" }
        );
    }
    let mission = s
        .create_mission(
            &u,
            MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: request(&urls("D-mission")),
                constraints: String::new(),
                checks: vec![MissionCheck::UserReview {
                    description: "Review the supported comparison".into(),
                }],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    runtime.wake();
    let done = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let value = s
                .missions(&u)
                .await
                .unwrap()
                .into_iter()
                .find(|v| v.id == mission.id)
                .unwrap();
            if value.state == "partial" || value.state == "failed" || value.state == "unknown" {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await
        }
    })
    .await
    .unwrap();
    assert_eq!(done.state, "partial");
    assert!(done.result.unwrap().text.contains("D-mission"));
    assert!(done.receipts.iter().any(|v| v.detail.contains("D-mission")));
    assert_eq!(learned.load(Ordering::SeqCst), 2);
    let expected = ["A-private", "B-private", "C-fresh", "D-mission"]
        .into_iter()
        .flat_map(urls)
        .collect::<Vec<_>>();
    assert_eq!(*seen.lock().await, expected);
    runtime.shutdown().await;
    drop(runtime);
    model_task.abort();
}
