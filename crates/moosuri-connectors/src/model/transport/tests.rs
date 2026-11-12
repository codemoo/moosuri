use super::*;
use crate::model::protocol::{FinishOutcome, TextMessage, TextRole};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, StatusCode},
    response::Response,
    routing::post,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::{sync::Mutex, task::JoinHandle};

enum Reply {
    Stream(Vec<Vec<u8>>),
    HeldTail {
        prefix: Vec<u8>,
        tail: Vec<u8>,
        waiting: Arc<tokio::sync::Notify>,
        release: Arc<tokio::sync::Notify>,
    },
    Status(u16),
    Redirect(String),
    Idle,
    DelayedHeaders,
    WrongType,
}

struct Captured {
    headers: HeaderMap,
    body: serde_json::Value,
}

struct Fixture {
    base: String,
    count: Arc<AtomicUsize>,
    captures: Arc<Mutex<Vec<Captured>>>,
    task: JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn fixture(reply: Reply) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let reply = Arc::new(reply);
    let count = Arc::new(AtomicUsize::new(0));
    let captures = Arc::new(Mutex::new(Vec::new()));
    let handler_count = count.clone();
    let handler_captures = captures.clone();
    let router = Router::new().route(
        "/{*path}",
        post(move |request: Request| {
            let reply = reply.clone();
            let count = handler_count.clone();
            let captures = handler_captures.clone();
            async move {
                let (parts, body) = request.into_parts();
                let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
                captures.lock().await.push(Captured {
                    headers: parts.headers,
                    body: serde_json::from_slice(&bytes).unwrap(),
                });
                count.fetch_add(1, Ordering::SeqCst);
                match reply.as_ref() {
                    Reply::Stream(frames) => {
                        let items: Vec<Result<Bytes, std::io::Error>> = frames
                            .iter()
                            .map(|v| Ok(Bytes::copy_from_slice(v)))
                            .collect();
                        Response::builder()
                            .header("content-type", "text/event-stream; charset=utf-8")
                            .body(Body::from_stream(futures_util::stream::iter(items)))
                            .unwrap()
                    }
                    Reply::HeldTail {
                        prefix,
                        tail,
                        waiting,
                        release,
                    } => {
                        let prefix = prefix.clone();
                        let tail = tail.clone();
                        let waiting = waiting.clone();
                        let release = release.clone();
                        let body = futures_util::stream::unfold(
                            (0, prefix, tail, waiting, release),
                            |(stage, prefix, tail, waiting, release)| async move {
                                match stage {
                                    0 => Some((
                                        Ok::<_, std::io::Error>(Bytes::copy_from_slice(&prefix)),
                                        (1, prefix, tail, waiting, release),
                                    )),
                                    1 => {
                                        waiting.notify_one();
                                        release.notified().await;
                                        if tail.is_empty() {
                                            None
                                        } else {
                                            Some((
                                                Ok(Bytes::copy_from_slice(&tail)),
                                                (2, prefix, tail, waiting, release),
                                            ))
                                        }
                                    }
                                    _ => None,
                                }
                            },
                        );
                        Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(Body::from_stream(body))
                            .unwrap()
                    }
                    Reply::Status(code) => Response::builder()
                        .status(StatusCode::from_u16(*code).unwrap())
                        .header("retry-after", "9")
                        .body(Body::from("synthetic-secret-must-not-appear-in-error"))
                        .unwrap(),
                    Reply::Redirect(target) => Response::builder()
                        .status(307)
                        .header("location", target)
                        .body(Body::empty())
                        .unwrap(),
                    Reply::Idle => Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(Body::from_stream(futures_util::stream::pending::<
                            Result<Bytes, std::io::Error>,
                        >()))
                        .unwrap(),
                    Reply::DelayedHeaders => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        Response::builder()
                            .header("content-type", "text/event-stream")
                            .body(Body::empty())
                            .unwrap()
                    }
                    Reply::WrongType => Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from("{}"))
                        .unwrap(),
                }
            }
        }),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Fixture {
        base: format!("http://{address}/v1"),
        count,
        captures,
        task,
    }
}

fn request() -> TextRequest {
    TextRequest {
        model: "fixture-model".into(),
        messages: vec![TextMessage {
            role: TextRole::User,
            text: "Synthetic hello".into(),
            phase: None,
        }],
        max_output_tokens: 128,
        effort: None,
    }
}

fn local(f: &Fixture) -> ModelClient {
    ModelClient::compatible(
        &f.base,
        NetworkPolicy::LocalExact(vec!["127.0.0.1".parse().unwrap()]),
        Some(Secret::new("synthetic-fixture-key".into()).unwrap()),
        TextCapabilities { efforts: vec![] },
    )
    .unwrap()
}

async fn run(
    client: &ModelClient,
    request: &TextRequest,
) -> (Result<(), TransportError>, Vec<ModelEvent>) {
    let (tx, mut rx) = mpsc::channel(4);
    let cancellation = CancellationToken::new();
    tokio::join!(
        async {
            let result = client.stream_text(request, &tx, &cancellation).await;
            drop(tx);
            result
        },
        async {
            let mut events = vec![];
            while let Some(e) = rx.recv().await {
                events.push(e);
            }
            events
        }
    )
}

fn chat_stream() -> String {
    concat!(
        ": synthetic keepalive\r\n\r\n",
        "data: {\"id\":\"fixture-response\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"안녕\"},\"finish_reason\":null}]}\r\n\r\n",
        "data: {\"id\":\"fixture-response\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"!\"},\"finish_reason\":null}]}\r\n\r\n",
        "data: {\"id\":\"fixture-response\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\n",
        "data: {\"id\":\"fixture-response\",\"model\":\"fixture-model\",\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\r\n\r\n",
        "data: [DONE]\r\n\r\n",
    ).into()
}

#[tokio::test]
async fn real_http_stream_preserves_utf8_usage_and_one_terminal() {
    let bytes = chat_stream().into_bytes();
    let f = fixture(Reply::Stream(bytes.chunks(7).map(|c| c.to_vec()).collect())).await;
    let (result, events) = run(&local(&f), &request()).await;
    assert!(result.is_ok(), "{result:?}");
    let mut text = String::new();
    let mut terminal = 0;
    let mut known_usage = false;
    for event in events {
        match event {
            ModelEvent::TextDelta { text: delta, .. } => text.push_str(&delta),
            ModelEvent::UsageSnapshot(usage) => {
                known_usage |= usage.input_tokens == Some(7) && usage.output_tokens == Some(3)
            }
            ModelEvent::Finished {
                outcome: FinishOutcome::Completed,
                ..
            } => terminal += 1,
            _ => {}
        }
    }
    assert_eq!(text, "안녕!");
    assert_eq!(terminal, 1);
    assert!(known_usage);
    assert_eq!(f.count.load(Ordering::SeqCst), 1);
    let captured = f.captures.lock().await;
    assert_eq!(
        captured[0].headers.get("authorization").unwrap(),
        "Bearer synthetic-fixture-key"
    );
    assert!(captured[0].headers.get("x-api-key").is_none());
    assert_eq!(captured[0].body["stream"], true);
    assert_eq!(captured[0].body["model"], "fixture-model");
}

#[test]
fn registered_network_boundary_rejects_metadata_and_unregistered_destinations() {
    let cases = [
        ("http://127.0.0.1:9000/v1", NetworkPolicy::PublicHttps),
        ("https://127.0.0.1/v1", NetworkPolicy::PublicHttps),
        (
            "http://169.254.169.254/v1",
            NetworkPolicy::LocalExact(vec!["169.254.169.254".parse().unwrap()]),
        ),
        (
            "http://[fd00:ec2::254]/v1",
            NetworkPolicy::LocalExact(vec!["fd00:ec2::254".parse().unwrap()]),
        ),
        (
            "http://10.0.0.2/v1",
            NetworkPolicy::LocalExact(vec!["10.0.0.3".parse().unwrap()]),
        ),
        (
            "http://[::ffff:169.254.169.254]/v1",
            NetworkPolicy::LocalExact(vec!["::ffff:169.254.169.254".parse().unwrap()]),
        ),
        (
            "http://100.100.100.200/v1",
            NetworkPolicy::LocalExact(vec!["100.100.100.200".parse().unwrap()]),
        ),
        (
            "http://[::ffff:100.100.100.200]/v1",
            NetworkPolicy::LocalExact(vec!["::ffff:100.100.100.200".parse().unwrap()]),
        ),
    ];
    for (url, network) in cases {
        assert!(matches!(
            ModelClient::compatible(url, network, None, TextCapabilities { efforts: vec![] }),
            Err(TransportError::DestinationDenied)
        ));
    }
    for url in [
        "https://user:password@example.test/v1",
        "https://example.test/v1?key=secret",
        "https://example.test/v1#fragment",
        "file:///tmp/model",
        "https://example.test/v1%2f",
    ] {
        assert!(
            ModelClient::compatible(
                url,
                NetworkPolicy::PublicHttps,
                None,
                TextCapabilities { efforts: vec![] }
            )
            .is_err()
        );
    }
    assert!(
        ModelClient::compatible(
            "http://100.100.10.20:11434/v1",
            NetworkPolicy::LocalExact(vec!["100.100.10.20".parse().unwrap()]),
            None,
            TextCapabilities { efforts: vec![] }
        )
        .is_ok()
    );
}

#[tokio::test]
async fn redirect_does_not_forward_request_or_key_and_http_failure_never_replays() {
    let target = fixture(Reply::Status(200)).await;
    let redirect = fixture(Reply::Redirect(format!("{}/chat/completions", target.base))).await;
    let (result, _) = run(&local(&redirect), &request()).await;
    assert!(matches!(
        result,
        Err(TransportError::Http { status: 307, .. })
    ));
    assert_eq!(target.count.load(Ordering::SeqCst), 0);
    assert_eq!(redirect.count.load(Ordering::SeqCst), 1);
    let overloaded = fixture(Reply::Status(429)).await;
    let (result, _) = run(&local(&overloaded), &request()).await;
    assert!(matches!(
        result,
        Err(TransportError::Http {
            status: 429,
            retry_after_seconds: Some(9)
        })
    ));
    assert!(!format!("{result:?}").contains("synthetic-secret"));
    assert_eq!(overloaded.count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn truncated_or_failed_stream_keeps_observed_prefix_without_success() {
    let prefix = "data: {\"id\":\"fixture-response\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n";
    for tail in [
        "",
        "data: {\"error\":{\"code\":\"synthetic_failure\",\"message\":\"synthetic-secret\"}}\n\n",
    ] {
        // One HTTP body/chunk can contain both a good event and a later error.
        let f = fixture(Reply::Stream(vec![format!("{prefix}{tail}").into_bytes()])).await;
        let (result, events) = run(&local(&f), &request()).await;
        assert!(matches!(result, Err(TransportError::Protocol(_))));
        assert!(!format!("{result:?}").contains("synthetic-secret"));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, ModelEvent::TextDelta { text, .. } if text == "partial"))
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ModelEvent::Finished { .. }))
        );
        assert_eq!(f.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn pre_cancel_and_invalid_capability_do_not_dispatch() {
    let f = fixture(Reply::Idle).await;
    let client = local(&f);
    let (tx, _rx) = mpsc::channel(1);
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        client.stream_text(&request(), &tx, &cancelled).await,
        Err(TransportError::Cancelled)
    ));
    let mut invalid = request();
    invalid.effort = Some("unsupported-effort".into());
    assert!(matches!(
        client
            .stream_text(&invalid, &tx, &CancellationToken::new())
            .await,
        Err(TransportError::Protocol(_))
    ));
    assert_eq!(f.count.load(Ordering::SeqCst), 0);
}

async fn wait_request(f: &Fixture) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while f.count.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn cancellation_interrupts_inflight_body_and_full_event_channel() {
    for reply in [Reply::Idle, Reply::Stream(vec![chat_stream().into_bytes()])] {
        let f = fixture(reply).await;
        let client = local(&f);
        let request = request();
        let (tx, _rx) = mpsc::channel(1); // Deliberately never drained.
        let cancellation = CancellationToken::new();
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(client.stream_text(&request, &tx, &cancellation), async {
                wait_request(&f).await;
                tokio::time::sleep(Duration::from_millis(20)).await;
                cancellation.cancel();
            })
        })
        .await
        .expect("cancellation must interrupt transport promptly");
        assert!(matches!(result, Err(TransportError::Cancelled)));
        assert_eq!(f.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn body_idle_and_total_backpressure_have_deadlines() {
    for reply in [Reply::Idle, Reply::Stream(vec![chat_stream().into_bytes()])] {
        let f = fixture(reply).await;
        let client = local(&f)
            .with_limits(TransportLimits {
                connect: Duration::from_millis(500),
                idle: Duration::from_millis(150),
                total: Duration::from_millis(750),
                response_bytes: 8192,
            })
            .unwrap();
        let (tx, _rx) = mpsc::channel(1);
        let started = std::time::Instant::now();
        let result = client
            .stream_text(&request(), &tx, &CancellationToken::new())
            .await;
        assert!(matches!(result, Err(TransportError::Timeout)));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert_eq!(f.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn dropped_consumer_interrupts_held_headers_and_body_without_waiting_for_timeout() {
    for reply in [Reply::DelayedHeaders, Reply::Idle] {
        let f = fixture(reply).await;
        let client = local(&f);
        let request = request();
        let (tx, rx) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        let (result, ()) = tokio::time::timeout(Duration::from_millis(500), async {
            tokio::join!(client.stream_text(&request, &tx, &cancellation), async {
                wait_request(&f).await;
                drop(rx);
            })
        })
        .await
        .expect("receiver closure should interrupt transport promptly");
        assert!(matches!(result, Err(TransportError::Cancelled)));
        assert_eq!(f.count.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn response_type_size_and_closed_consumer_fail_boundedly() {
    let wrong = fixture(Reply::WrongType).await;
    assert!(matches!(
        run(&local(&wrong), &request()).await.0,
        Err(TransportError::ContentType)
    ));
    let large = fixture(Reply::Stream(vec![vec![b' '; 4096]])).await;
    let client = local(&large)
        .with_limits(TransportLimits {
            response_bytes: 128,
            ..TransportLimits::default()
        })
        .unwrap();
    assert!(matches!(
        run(&client, &request()).await.0,
        Err(TransportError::LimitExceeded)
    ));
    let idle = fixture(Reply::Idle).await;
    let (tx, rx) = mpsc::channel(1);
    drop(rx);
    assert!(matches!(
        local(&idle)
            .stream_text(&request(), &tx, &CancellationToken::new())
            .await,
        Err(TransportError::Cancelled)
    ));
    assert_eq!(idle.count.load(Ordering::SeqCst), 0);
}

#[test]
fn hosted_credentials_and_router_policy_cannot_be_retargeted_or_silently_relaxed() {
    assert!(
        ModelClient::hosted(
            Protocol::ChatCompletions,
            Secret::new("synthetic-key".into()).unwrap(),
            TextCapabilities { efforts: vec![] },
            None
        )
        .is_err()
    );
    assert!(
        ModelClient::hosted(
            Protocol::OpenRouterChat,
            Secret::new("synthetic-key".into()).unwrap(),
            TextCapabilities { efforts: vec![] },
            None
        )
        .is_err()
    );
    assert!(Secret::new("synthetic\r\ninjected-header".into()).is_err());
    assert!(
        ModelClient::hosted(
            Protocol::OpenRouterChat,
            Secret::new("synthetic-key".into()).unwrap(),
            TextCapabilities { efforts: vec![] },
            Some(RouterPolicy {
                providers: vec!["fixture-provider".into()],
                zero_data_retention: true
            })
        )
        .is_ok()
    );
}

#[tokio::test]
async fn hosted_protocols_send_correct_headers_and_explicit_routing_over_real_http() {
    use serde_json::json;
    for protocol in [
        Protocol::Responses,
        Protocol::AnthropicMessages,
        Protocol::OpenRouterChat,
    ] {
        let wire = match protocol {
            Protocol::Responses => {
                let part = json!({"type":"output_text","text":"Fixture answer","annotations":[]});
                let item = json!({"id":"fixture-item","type":"message","role":"assistant","status":"completed","phase":"final_answer","content":[part.clone()]});
                [
                    json!({"type":"response.created","response":{"id":"fixture-response","model":"fixture-model","status":"in_progress","output":[]}}),
                    json!({"type":"response.output_item.added","output_index":0,"item":{"id":"fixture-item","type":"message","role":"assistant","phase":"final_answer","content":[]}}),
                    json!({"type":"response.content_part.added","item_id":"fixture-item","output_index":0,"content_index":0,"part":{"type":"output_text","text":""}}),
                    json!({"type":"response.output_text.delta","item_id":"fixture-item","output_index":0,"content_index":0,"delta":"Fixture answer"}),
                    json!({"type":"response.output_text.done","item_id":"fixture-item","output_index":0,"content_index":0,"text":"Fixture answer"}),
                    json!({"type":"response.content_part.done","item_id":"fixture-item","output_index":0,"content_index":0,"part":part}),
                    json!({"type":"response.output_item.done","output_index":0,"item":item.clone()}),
                    json!({"type":"response.completed","response":{"id":"fixture-response","model":"fixture-model","status":"completed","output":[item],"usage":{"input_tokens":8,"output_tokens":2}}}),
                ].iter().map(|v| format!("data: {v}\n\n")).collect::<String>()
            }
            Protocol::AnthropicMessages => [
                json!({"type":"message_start","message":{"id":"fixture-message","model":"fixture-model","role":"assistant","content":[],"usage":{"input_tokens":8,"output_tokens":0}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Fixture answer"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
                json!({"type":"message_stop"}),
            ].iter().map(|v| format!("data: {v}\n\n")).collect::<String>(),
            _ => chat_stream(),
        };
        let f = fixture(Reply::Stream(vec![wire.into_bytes()])).await;
        let router = if matches!(protocol, Protocol::OpenRouterChat) {
            Some(RouterPolicy {
                providers: vec!["fixture-provider".into()],
                zero_data_retention: true,
            })
        } else {
            None
        };
        let mut client = ModelClient::hosted(
            protocol,
            Secret::new("synthetic-hosted-key".into()).unwrap(),
            TextCapabilities {
                efforts: vec!["high".into()],
            },
            router,
        )
        .unwrap();
        let official = match protocol {
            Protocol::Responses => "api.openai.com",
            Protocol::AnthropicMessages => "api.anthropic.com",
            _ => "openrouter.ai",
        };
        assert_eq!(client.endpoint.host_str(), Some(official));
        // Private test-only field access; the production API has no retarget method.
        client.endpoint = Url::parse(&format!("{}/fixture", f.base)).unwrap();
        client.network = NetworkPolicy::LocalExact(vec!["127.0.0.1".parse().unwrap()]);
        let mut requested = request();
        requested.effort = Some("high".into());
        let (result, events) = run(&client, &requested).await;
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(
                    e,
                    ModelEvent::Finished {
                        outcome: FinishOutcome::Completed,
                        ..
                    }
                ))
                .count(),
            1
        );
        let captured = f.captures.lock().await;
        let sent = &captured[0];
        match protocol {
            Protocol::AnthropicMessages => {
                assert_eq!(
                    sent.headers.get("x-api-key").unwrap(),
                    "synthetic-hosted-key"
                );
                assert_eq!(sent.headers.get("anthropic-version").unwrap(), "2023-06-01");
                assert!(sent.headers.get("authorization").is_none());
                assert_eq!(sent.body["output_config"]["effort"], "high");
            }
            _ => {
                assert_eq!(
                    sent.headers.get("authorization").unwrap(),
                    "Bearer synthetic-hosted-key"
                );
                assert!(sent.headers.get("x-api-key").is_none());
                assert_eq!(sent.body["reasoning"]["effort"], "high");
            }
        }
        if matches!(protocol, Protocol::OpenRouterChat) {
            assert_eq!(
                sent.body["provider"],
                json!({"only":["fixture-provider"],"allow_fallbacks":false,"require_parameters":true,"data_collection":"deny","zdr":true})
            );
        }
        assert_eq!(f.count.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn catalogs_preserve_ids_and_bound_first_page_without_claiming_capability() {
    use serde_json::json;
    let list = parse_catalog(
        Protocol::AnthropicMessages,
        br#"{"data":[{"id":"claude-fixture","display_name":"Fixture"}],"has_more":true}"#,
    )
    .unwrap();
    assert_eq!(list.models[0].id, "claude-fixture");
    assert_eq!(list.models[0].name, "Fixture");
    assert!(list.truncated);
    let data: Vec<_> = (0..513)
        .map(|n| json!({"id":format!("fixture-{n}")}))
        .collect();
    let list = parse_catalog(
        Protocol::Responses,
        &serde_json::to_vec(&json!({"data":data})).unwrap(),
    )
    .unwrap();
    assert_eq!(list.models.len(), 512);
    assert_eq!(list.models[0].name, "fixture-0");
    assert!(list.truncated);
    let list = parse_catalog(
        Protocol::ChatCompletions,
        br#"{"data":[{"id":"local-fixture"},{"id":"local-fixture"}]}"#,
    )
    .unwrap();
    assert_eq!(list.models.len(), 1);
    assert!(!list.truncated);
    for bytes in [
        b"{}".as_slice(),
        br#"{"data":[{"id":"a\nb"}]}"#,
        br#"{"data":[],"has_more":"true"}"#,
        br#"{"error":"fixture-private-body"}"#,
    ] {
        let error = parse_catalog(Protocol::Responses, bytes).err().unwrap();
        assert!(matches!(error, TransportError::InvalidCatalog));
        assert!(!error.to_string().contains("fixture-private"));
    }
}

#[tokio::test]
async fn model_catalog_uses_registered_base_and_scoped_key_with_bounded_response() {
    use axum::routing::get;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let router = Router::new().route(
        "/local/v1/models",
        get(|headers: HeaderMap| async move {
            assert!(headers["authorization"] == "Bearer synthetic-catalog-key");
            assert_eq!(headers["accept"], "application/json");
            (
                [("content-type", "application/json")],
                r#"{"data":[{"id":"local-model"}]}"#,
            )
        }),
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = ModelClient::compatible(
        &format!("http://{address}/local/v1"),
        NetworkPolicy::LocalExact(vec![address.ip()]),
        Some(Secret::new("synthetic-catalog-key".into()).unwrap()),
        TextCapabilities::default(),
    )
    .unwrap();
    let catalog = client.list_models(&CancellationToken::new()).await.unwrap();
    assert_eq!(catalog.models[0].id, "local-model");
    let client = client
        .with_limits(TransportLimits {
            response_bytes: 4,
            ..TransportLimits::default()
        })
        .unwrap();
    assert!(matches!(
        client.list_models(&CancellationToken::new()).await,
        Err(TransportError::LimitExceeded)
    ));
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        client.list_models(&cancel).await,
        Err(TransportError::Cancelled)
    ));
    task.abort();
}

fn tool_definitions() -> Vec<super::super::protocol::tools::FunctionDefinition> {
    vec![super::super::protocol::tools::FunctionDefinition {
        name: "knowledge_search".into(),
        description: "Search synthetic data".into(),
        parameters: serde_json::json!({"type":"object","properties":{"q":{"type":"string"}},"required":["q"],"additionalProperties":false}),
    }]
}
fn tool_chat_stream() -> String {
    concat!(
      "data: {\"id\":\"tool-response\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"검색합니다\",\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"knowledge_search\",\"arguments\":\"{\\\"q\\\":\"}}]},\"finish_reason\":null}]}\n\n",
      "data: {\"id\":\"tool-response\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"홍차\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
      "data: {\"id\":\"tool-response\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
      "data: [DONE]\n\n",
    ).into()
}
async fn run_tools(
    client: &ModelClient,
    cancellation: &CancellationToken,
) -> (
    Result<super::super::protocol::tools::ToolRound, TransportError>,
    Vec<ModelEvent>,
) {
    use super::super::protocol::tools::{ToolCapabilities, ToolRequest};
    let request = request();
    let tools = tool_definitions();
    let (tx, mut rx) = mpsc::channel(2);
    tokio::join!(
        async {
            let result = client
                .stream_tool_round(
                    &ToolRequest {
                        text: &request,
                        tools: &tools,
                        continuation: None,
                        results: &[],
                    },
                    &ToolCapabilities {
                        function_tools: true,
                    },
                    &tx,
                    cancellation,
                )
                .await;
            drop(tx);
            result
        },
        async {
            let mut events = vec![];
            while let Some(e) = rx.recv().await {
                events.push(e);
            }
            events
        }
    )
}
#[tokio::test]
async fn tool_round_real_transport_returns_calls_only_after_complete_stream() {
    let stream = tool_chat_stream();
    let f = fixture(Reply::Stream(
        stream.as_bytes().chunks(3).map(|s| s.to_vec()).collect(),
    ))
    .await;
    let (result, events) = run_tools(&local(&f), &CancellationToken::new()).await;
    let round = result.unwrap();
    assert_eq!(round.calls.len(), 1);
    assert_eq!(round.calls[0].arguments(), "{\"q\":\"홍차\"}");
    assert_eq!(round.outcome, FinishOutcome::ContinuationRequired);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ModelEvent::TextDelta { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ModelEvent::Finished { .. }))
    );
    let captures = f.captures.lock().await;
    assert_eq!(captures[0].body["tools"][0]["function"]["strict"], true);
    assert_eq!(captures[0].body["parallel_tool_calls"], false);
}
#[tokio::test]
async fn tool_transport_prefix_error_cancellation_and_limit_never_return_calls() {
    let mut stream = tool_chat_stream();
    stream.push_str("data: {\"error\":{\"message\":\"synthetic private failure\"}}\n\n");
    let f = fixture(Reply::Stream(vec![stream.into_bytes()])).await;
    let (result, events) = run_tools(&local(&f), &CancellationToken::new()).await;
    assert!(result.is_err());
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ModelEvent::TextDelta { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, ModelEvent::Finished { .. }))
    );
    let f = fixture(Reply::Stream(vec![tool_chat_stream().into_bytes()])).await;
    let client = local(&f)
        .with_limits(TransportLimits {
            response_bytes: 32,
            ..TransportLimits::default()
        })
        .unwrap();
    let (result, _) = run_tools(&client, &CancellationToken::new()).await;
    assert!(matches!(result, Err(TransportError::LimitExceeded)));
    let f = fixture(Reply::Idle).await;
    let client = local(&f);
    let cancellation = CancellationToken::new();
    let cancel = cancellation.clone();
    let trigger = async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        cancel.cancel();
    };
    let ((result, _), ()) = tokio::join!(run_tools(&client, &cancellation), trigger);
    assert!(matches!(result, Err(TransportError::Cancelled)));
}
#[tokio::test]
async fn tool_transport_opt_in_validation_precedes_network_and_preserves_destination_policy() {
    use super::super::protocol::tools::{ToolCapabilities, ToolRequest};
    let f = fixture(Reply::Stream(vec![tool_chat_stream().into_bytes()])).await;
    let client = local(&f);
    let text = request();
    let tools = tool_definitions();
    let req = ToolRequest {
        text: &text,
        tools: &tools,
        continuation: None,
        results: &[],
    };
    let (tx, _rx) = mpsc::channel(2);
    let result = client
        .stream_tool_round(
            &req,
            &ToolCapabilities::default(),
            &tx,
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(
        result,
        Err(TransportError::Protocol(
            ProtocolError::UnsupportedCapability
        ))
    ));
    assert_eq!(f.count.load(Ordering::SeqCst), 0);
    let denied = ModelClient::compatible(
        &f.base,
        NetworkPolicy::PublicHttps,
        None,
        TextCapabilities::default(),
    );
    assert!(matches!(denied, Err(TransportError::DestinationDenied)));
}

#[tokio::test]
async fn completed_tool_round_waits_for_clean_eof_and_rejects_later_chunk_suffixes() {
    use super::super::protocol::tools::{ToolCapabilities, ToolRequest};
    // The server cannot produce its tail until the client has consumed previews
    // and the test explicitly releases it. These are separate HTTP body deliveries,
    // not just different Vec entries that Hyper could coalesce into one chunk.
    for tail in [
        b"data: {\"error\":{\"message\":\"synthetic private tail\"}}\n\n".to_vec(),
        b"data: [DONE]\n\n".to_vec(),
        Vec::new(),
    ] {
        let expected_success = tail.is_empty();
        let waiting = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let f = fixture(Reply::HeldTail {
            prefix: tool_chat_stream().into_bytes(),
            tail,
            waiting: waiting.clone(),
            release: release.clone(),
        })
        .await;
        let client = local(&f);
        let text = request();
        let tools = tool_definitions();
        let req = ToolRequest {
            text: &text,
            tools: &tools,
            continuation: None,
            results: &[],
        };
        let (tx, mut rx) = mpsc::channel(16);
        let cancel = CancellationToken::new();
        let capabilities = ToolCapabilities {
            function_tools: true,
        };
        let operation = client.stream_tool_round(&req, &capabilities, &tx, &cancel);
        tokio::pin!(operation);
        loop {
            tokio::select! {biased;result=&mut operation=>panic!("tool round returned before EOF: {}",result.is_ok()),event=rx.recv()=>if matches!(event,Some(ModelEvent::TextDelta{..})){break;}}
        }
        tokio::select! {biased;result=&mut operation=>panic!("tool round returned before held tail: {}",result.is_ok()),_=waiting.notified()=>{}}
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut operation)
                .await
                .is_err()
        );
        release.notify_one();
        let result = operation.await;
        if expected_success {
            let round = result.unwrap();
            assert_eq!(round.calls.len(), 1);
            assert_eq!(round.calls[0].call_id(), "call-1");
        } else {
            assert!(matches!(result, Err(TransportError::Protocol(_))));
        }
    }
}

#[tokio::test]
async fn tool_round_held_after_terminal_is_still_cancellable_and_timed_out() {
    use super::super::protocol::tools::{ToolCapabilities, ToolRequest};
    for should_cancel in [true, false] {
        let waiting = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let f = fixture(Reply::HeldTail {
            prefix: tool_chat_stream().into_bytes(),
            tail: Vec::new(),
            waiting: waiting.clone(),
            release,
        })
        .await;
        let client = local(&f)
            .with_limits(TransportLimits {
                connect: Duration::from_millis(200),
                idle: Duration::from_millis(200),
                total: Duration::from_millis(200),
                ..TransportLimits::default()
            })
            .unwrap();
        let text = request();
        let tools = tool_definitions();
        let req = ToolRequest {
            text: &text,
            tools: &tools,
            continuation: None,
            results: &[],
        };
        let (tx, mut rx) = mpsc::channel(16);
        let cancel = CancellationToken::new();
        let capabilities = ToolCapabilities {
            function_tools: true,
        };
        let operation = client.stream_tool_round(&req, &capabilities, &tx, &cancel);
        tokio::pin!(operation);
        loop {
            tokio::select! {biased;result=&mut operation=>panic!("tool round returned before held EOF: {}",result.is_ok()),event=rx.recv()=>if matches!(event,Some(ModelEvent::TextDelta{..})){break;}}
        }
        tokio::select! {biased;result=&mut operation=>panic!("tool round returned before held EOF: {}",result.is_ok()),_=waiting.notified()=>{}}
        if should_cancel {
            cancel.cancel();
        }
        let result = operation.await;
        if should_cancel {
            assert!(matches!(result, Err(TransportError::Cancelled)));
        } else {
            assert!(matches!(result, Err(TransportError::Timeout)));
        }
    }
}

#[tokio::test]
async fn existing_text_stream_keeps_terminal_marker_completion_without_waiting_for_eof() {
    let f = fixture(Reply::HeldTail {
        prefix: chat_stream().into_bytes(),
        tail: Vec::new(),
        waiting: Arc::new(tokio::sync::Notify::new()),
        release: Arc::new(tokio::sync::Notify::new()),
    })
    .await;
    let client = local(&f);
    let (tx, _rx) = mpsc::channel(16);
    let req = request();
    let cancellation = CancellationToken::new();
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        client.stream_text(&req, &tx, &cancellation),
    )
    .await;
    assert!(matches!(result, Ok(Ok(()))));
}
