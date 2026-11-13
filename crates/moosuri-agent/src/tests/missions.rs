//! Synthetic full mission through the real supervisor, model HTTP parser and writer.
use super::*;
use moosuri_core::{MissionCheck, MissionCreate, MissionMode};
#[tokio::test]
async fn mission_model_plan_two_tools_and_deterministic_result_survive_reopen() {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
    let count_handler = count.clone();
    let requests_handler = requests.clone();
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let count=count_handler.clone();let requests=requests_handler.clone();async move {
 let n=count.fetch_add(1,Ordering::SeqCst);requests.lock().await.push(body.clone());
 let (delta,reason)=match n {
 0=>(json!({"role":"assistant","content":"{\"steps\":[\"Read an admitted evidence recipe\",\"Calculate the exact total\",\"Verify the result\"]}"}),"stop"),
 1=>{let message=body["messages"][1]["content"].as_str().unwrap();let original:Value=serde_json::from_str(message).unwrap();assert_eq!(original["original"]["execution_mode"],"durable");let defs=body["tools"].as_array().unwrap();assert!(defs.iter().all(|d|!d["function"]["name"].as_str().unwrap().contains("mail")));let system=body["messages"][0]["content"].as_str().unwrap();let pins:Value=serde_json::from_str(system.split("Admitted recipe metadata: ").nth(1).unwrap()).unwrap();let pin=pins.as_array().unwrap().iter().find(|p|p["id"]=="evidence-comparison").unwrap();(json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"exact-total".into(),"decimal_calculate",json!({"operation":"sum","operands":["0.1","0.2"]})),knowledge::tool_call(1,"find-source".into(),"knowledge_search",json!({"kind":"source","text":"Synthetic invoice","collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null})),knowledge::tool_call(2,"recipe".into(),"skill_read",pin.clone())]}),"tool_calls")},
 2=>{let context:Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();let slots=context["receipts"].as_array().unwrap();let receipt=|name:&str|->Value{serde_json::from_str(slots.iter().find(|v|v["call_id"]==name).unwrap()["result"].as_str().unwrap()).unwrap()};assert_eq!(receipt("exact-total")["value"]["exact"],"0.3");assert_eq!(receipt("recipe")["kind"],"skill");let search=receipt("find-source");let hit=&search["value"]["hits"][0];(json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"invoice".into(),"knowledge_read_source",json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096}))]}),"tool_calls")},
 3=>{let context:Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();let slot=context["receipts"].as_array().unwrap().iter().find(|v|v["call_id"]=="invoice").unwrap();let source:Value=serde_json::from_str(slot["result"].as_str().unwrap()).unwrap();assert_eq!(source["value"]["text"],"Synthetic invoice: apples 0.1, tea 0.2.");(json!({"role":"assistant","content":json!({"text":"The exact total is 0.3. Synthetic invoice: apples 0.1, tea 0.2.","evidence":[{"step":2,"call_id":"exact-total"},{"step":2,"call_id":"recipe"},{"step":3,"call_id":"invoice"}]}).to_string()}),"stop")},
 _=>panic!("mission exceeded its necessary model calls"),
 };
 let content=frame(json!({"id":format!("mission-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}))+&frame(json!({"id":format!("mission-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";
 Response::builder().header("content-type","text/event-stream").body(Body::from(content)).unwrap()
 }}));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (d, s, u, thread) = knowledge::tool_configured(&base).await;
    let source = knowledge::seed(
        &s,
        &u,
        "Synthetic invoice",
        vec!["Synthetic invoice: apples 0.1, tea 0.2.".into()],
    )
    .await
    .remove(0);
    let m = s
        .create_mission(
            &u,
            MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: "Calculate synthetic total 0.1 + 0.2".into(),
                constraints: String::new(),
                checks: vec![
                    MissionCheck::DecimalEquals {
                        expected: "0.3".into(),
                    },
                    MissionCheck::SourceQuote {
                        id: source,
                        revision: 1,
                        quote: "Synthetic invoice: apples 0.1, tea 0.2.".into(),
                    },
                ],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    let runtime = Runtime::start(s.clone());
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let v = s.missions(&u).await.unwrap().remove(0);
            if !["queued", "planning", "executing", "verifying"].contains(&v.state.as_str()) {
                break v;
            }
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await
    .unwrap();
    assert_eq!(result.state, "achieved", "{:?}", result.report);
    assert_eq!(
        result.result.unwrap().text,
        "The exact total is 0.3. Synthetic invoice: apples 0.1, tea 0.2."
    );
    assert_eq!(count.load(Ordering::SeqCst), 4);
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 4);
    assert!(
        s.conversation(&u, thread, None)
            .await
            .unwrap()
            .turns
            .is_empty()
    );
    assert_eq!(s.list_threads(&u).await.unwrap().len(), 1);
    runtime.shutdown().await;
    drop(runtime);
    drop(s);
    let s = Store::open(d.path()).unwrap();
    let historical = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(historical.id, m.id);
    assert_eq!(historical.state, "achieved");
    assert_eq!(count.load(Ordering::SeqCst), 4);
    upstream.abort();
}
