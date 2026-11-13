use super::*;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

fn snapshot() -> Value {
    // A visible aria-hidden negation, a form value, a hidden script, and an
    // editable subtree. Snapshot strings/attributes intentionally contain secrets.
    json!({"strings":["#document","BODY","SPAN","#text","INPUT","TEXTAREA","SCRIPT","DIV","not ","refundable","FORM_SECRET","SCRIPT_SECRET","EDIT_SECRET","type","password","value","contenteditable","true","block","visible","1","none","https://example.org/","","auto","static","0","frame"],
      "documents":[{"documentURL":22,"frameId":27,"scrollOffsetX":0,"scrollOffsetY":0,"nodes":{"nodeType":[9,1,1,3,3,1,1,3,1,3,1,3],"nodeName":[0,1,2,3,3,4,5,3,6,3,7,3],"parentIndex":[-1,0,1,2,1,1,1,6,1,8,1,10],
       "backendNodeId":[1,2,3,4,5,6,7,8,9,10,11,12],"attributes":[[],[],[],[],[],[13,14,15,10],[],[],[],[],[16,17],[]],
       "inputValue":{"index":[5],"value":[10]},"textValue":{"index":[6],"value":[10]}},
       "layout":{"bounds":[[0,0,1280,900],[0,0,30,20],[0,0,20,20],[20,0,100,20],[20,60,200,30],[20,110,200,30],[20,110,20,20],[0,0,10,10],[0,0,10,10],[0,0,10,10],[0,0,10,10]],"nodeIndex":[0,2,3,4,5,6,7,8,9,10,11],"text":[-1,-1,8,9,10,-1,10,-1,11,-1,12],"styles":[[],[18,19,20],[18,19,20],[18,19,20],[18,19,20],[18,19,20],[18,19,20],[18,19,20],[18,19,20],[18,19,20],[18,19,20]]}}]})
}
fn ax() -> Value {
    json!({"nodes":[
      {"nodeId":"1","backendDOMNodeId":1,"role":{"value":"RootWebArea"},"name":{"value":"Fixture"},"value":{"value":"AX_SECRET"}},
      {"nodeId":"2","backendDOMNodeId":5,"role":{"value":"StaticText"},"name":{"value":"refundable"}},
      {"nodeId":"3","backendDOMNodeId":6,"role":{"value":"textbox"},"name":{"value":"Password"},"value":{"value":"FORM_SECRET"},"childIds":["6"]},
      {"nodeId":"4","backendDOMNodeId":8,"role":{"value":"StaticText"},"name":{"value":"FORM_SECRET"}},
      {"nodeId":"5","backendDOMNodeId":12,"role":{"value":"StaticText"},"name":{"value":"EDIT_SECRET"}},
      {"nodeId":"6","role":{"value":"StaticText"},"name":{"value":"VIRTUAL_FORM_SECRET"}}
    ]})
}
#[test]
fn ax_is_structured_and_visible_aria_hidden_negation_survives_without_form_values() {
    let page = project_observation("https://example.org/", &ax(), &snapshot())
        .unwrap()
        .page;
    assert!(page.text.contains("\"role\":\"StaticText\""));
    assert!(
        page.text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .contains("not refundable")
    );
    for secret in [
        "FORM_SECRET",
        "AX_SECRET",
        "SCRIPT_SECRET",
        "EDIT_SECRET",
        "VIRTUAL_FORM_SECRET",
    ] {
        assert!(!page.text.contains(secret));
    }
    assert_eq!(page.title, "Fixture");
    let mut malformed = snapshot();
    malformed["documents"][0]["nodes"]["parentIndex"][3] = json!(50);
    assert!(project_observation("https://example.org/", &ax(), &malformed).is_err());
}
struct State {
    loader: String,
    url: String,
    screenshot: u8,
    shifted: bool,
    overlay: bool,
    reordered: bool,
    focused_backend: u64,
    observations: usize,
    scroll: u32,
    inputs: usize,
    commands: Vec<String>,
    stall_input: bool,
}
async fn fixture() -> (
    BrowserWorkspace,
    Arc<Mutex<State>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!(
        "ws://{}/devtools/browser/synthetic",
        listener.local_addr().unwrap()
    );
    let state = Arc::new(Mutex::new(State {
        loader: "one".into(),
        url: "https://example.org/one".into(),
        screenshot: 1,
        shifted: false,
        overlay: false,
        reordered: false,
        focused_backend: 1,
        observations: 0,
        scroll: 0,
        inputs: 0,
        commands: Vec::new(),
        stall_input: false,
    }));
    let serve = state.clone();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        while let Some(Ok(message)) = ws.next().await {
            let request: Value = serde_json::from_str(message.to_text().unwrap()).unwrap();
            let (result, stall) = {
                let mut state = serve.lock().unwrap();
                let method = request["method"].as_str().unwrap();
                state.commands.push(method.into());
                let result = match method {
                    "Page.getFrameTree" => {
                        json!({"frameTree":{"frame":{"id":"frame","loaderId":state.loader,"url":state.url}}})
                    }
                    "Accessibility.getFullAXTree" => {
                        let mut ax = ax();
                        for node in ax["nodes"].as_array_mut().unwrap() {
                            if node["backendDOMNodeId"].as_u64() == Some(state.focused_backend) {
                                node["properties"] = json!([{"name":"focused","value":{"type":"boolean","value":true}}]);
                            }
                        }
                        ax
                    }
                    "DOMSnapshot.captureSnapshot" => {
                        let mut snapshot = snapshot();
                        let style_count = request["params"]["computedStyles"]
                            .as_array()
                            .unwrap()
                            .len();
                        if style_count == 3 {
                            state.observations += 1;
                        }
                        if style_count == INTERACTION_STYLES.len() {
                            let mut styles = vec![json!([18, 19, 24, 20, 24, 25, 19, 21]); 11];
                            styles[0] = json!([]);
                            // An unchanged-bounds overlay can become interactive.
                            if !state.overlay {
                                styles[9][2] = json!(21);
                                styles[9][3] = json!(26);
                            }
                            snapshot["documents"][0]["layout"]["styles"] = json!(styles);
                            let mut paint: Vec<u32> = (0..11).collect();
                            if state.reordered {
                                paint[9] = 100;
                            }
                            snapshot["documents"][0]["layout"]["paintOrders"] = json!(paint);
                        }
                        if state.shifted {
                            snapshot["documents"][0]["layout"]["bounds"][4][0] = json!(250);
                        }
                        snapshot["documents"][0]["scrollOffsetY"] = json!(state.scroll);
                        snapshot
                    }
                    "Page.getLayoutMetrics" => {
                        json!({"cssLayoutViewport":{"pageX":0,"pageY":state.scroll,"clientWidth":1280,"clientHeight":900},"cssVisualViewport":{"offsetX":0,"offsetY":0,"pageX":0,"pageY":state.scroll,"clientWidth":1280,"clientHeight":900,"scale":1},"cssContentSize":{"x":0,"y":0,"width":1280,"height":900}})
                    }
                    "Page.captureScreenshot" => {
                        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
                        png.extend(1280u32.to_be_bytes());
                        png.extend(900u32.to_be_bytes());
                        png.push(state.screenshot);
                        json!({"data":base64::engine::general_purpose::STANDARD.encode(png)})
                    }
                    "Input.insertText" | "Input.dispatchKeyEvent" | "Input.dispatchMouseEvent" => {
                        state.inputs += 1;
                        json!({})
                    }
                    "Page.navigate" => {
                        state.url = request["params"]["url"].as_str().unwrap().into();
                        json!({"frameId":"frame"})
                    }
                    _ => panic!("unexpected CDP method {method}"),
                };
                (result, state.stall_input && method.starts_with("Input."))
            };
            if stall {
                std::future::pending::<()>().await;
            }
            if ws
                .send(Message::Text(
                    json!({"id":request["id"],"sessionId":request["sessionId"],"result":result})
                        .to_string()
                        .into(),
                ))
                .await
                .is_err()
            {
                break;
            }
        }
    });
    let generation = Uuid::new_v4();
    let page = Uuid::new_v4();
    let handle = PageHandle {
        page,
        worker_generation: generation,
        navigation_generation: 1,
    };
    let mut targets = HashMap::new();
    targets.insert(
        page,
        Target {
            target: "target".into(),
            session: "session".into(),
            frame: "frame".into(),
            loader: "one".into(),
        },
    );
    let session = BrowserSession {
        tenant: Uuid::new_v4(),
        generation,
        child: Arc::new(Mutex::new(None)),
        watchdog: tokio::spawn(async {}),
        profile: std::env::temp_dir().join(format!("no-browser-profile-{}", Uuid::new_v4())),
        cdp: Cdp::connect(&endpoint).await.unwrap(),
        targets,
        deadline: Instant::now() + Duration::from_secs(100),
        poisoned: false,
        cancellation: CancellationToken::new(),
        javascript_enabled: true,
    };
    (
        BrowserWorkspace {
            session,
            current: Some(handle),
            proof: None,
        },
        state,
        server,
    )
}
#[tokio::test]
async fn workspace_refreshes_navigation_while_original_read_handles_reject_it() {
    let (mut workspace, state, server) = fixture().await;
    {
        let mut s = state.lock().unwrap();
        s.loader = "two".into();
        s.url = "https://example.org/two".into();
    }
    let handle = workspace.current.clone().unwrap();
    assert!(matches!(
        workspace.session.read(&handle).await,
        Err(BrowserError::StalePage)
    ));
    assert_eq!(
        workspace.observe().await.unwrap().page.url,
        "https://example.org/two"
    );
    assert!(
        !state
            .lock()
            .unwrap()
            .commands
            .iter()
            .any(|c| c == "Runtime.evaluate")
    );
    workspace.open("https://example.org/three").await.unwrap();
    {
        let state = state.lock().unwrap();
        assert_eq!(
            state
                .commands
                .iter()
                .filter(|c| *c == "Page.navigate")
                .count(),
            1
        );
        assert!(!state.commands.iter().any(|c| c == "Target.createTarget"));
    }
    workspace.shutdown().await;
    server.abort();
}
#[tokio::test]
async fn frame_tokens_accept_caret_pixels_but_reject_navigation_geometry_scroll_age_and_replay() {
    let (mut workspace, state, server) = fixture().await;
    let frame = workspace.frame().await.unwrap();
    state.lock().unwrap().loader = "changed".into();
    assert!(matches!(
        workspace
            .input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 })
            .await,
        Err(BrowserError::StalePage)
    ));
    assert_eq!(state.lock().unwrap().inputs, 0);
    let frame = workspace.frame().await.unwrap();
    state.lock().unwrap().shifted = true;
    assert!(matches!(
        workspace
            .input(
                &frame.frame_token,
                WorkspaceInput::Key {
                    key: WorkspaceKey::Enter
                }
            )
            .await,
        Err(BrowserError::StalePage)
    ));
    assert_eq!(state.lock().unwrap().inputs, 0);
    let frame = workspace.frame().await.unwrap();
    state.lock().unwrap().scroll = 20;
    assert!(matches!(
        workspace
            .input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 })
            .await,
        Err(BrowserError::StalePage)
    ));
    let frame = workspace.frame().await.unwrap();
    workspace.proof.as_mut().unwrap().captured_at =
        Instant::now() - FRAME_MAX_AGE - Duration::from_secs(1);
    assert!(matches!(
        workspace
            .input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 })
            .await,
        Err(BrowserError::StalePage)
    ));
    assert_eq!(state.lock().unwrap().inputs, 0);
    let frame = workspace.frame().await.unwrap();
    // A blinking caret/color animation changes PNG pixels but keeps geometry.
    state.lock().unwrap().screenshot = 2;
    tokio::time::sleep(Duration::from_millis(20)).await;
    workspace
        .input(
            &frame.frame_token,
            WorkspaceInput::InsertText {
                text: "SYNTHETIC_SECRET".into(),
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        workspace
            .input(
                &frame.frame_token,
                WorkspaceInput::Key {
                    key: WorkspaceKey::Enter
                }
            )
            .await,
        Err(BrowserError::StalePage)
    ));
    {
        let state = state.lock().unwrap();
        assert_eq!(state.inputs, 1);
        // Input extracts only focus identities from AX; no page text projection.
        assert_eq!(state.observations, 0);
        assert!(!state.commands.iter().any(|c| c == "Runtime.evaluate"));
    }
    workspace.shutdown().await;
    server.abort();
}
#[tokio::test]
async fn cancelled_dispatched_input_poisoning_prevents_any_replay() {
    let (mut workspace, state, server) = fixture().await;
    let frame = workspace.frame().await.unwrap();
    state.lock().unwrap().stall_input = true;
    let cancel = workspace.session.cancellation.clone();
    let task = tokio::spawn(async move {
        let result = workspace
            .input(
                &frame.frame_token,
                WorkspaceInput::InsertText {
                    text: "synthetic".into(),
                },
            )
            .await;
        (workspace, result)
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        while state.lock().unwrap().inputs == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    let (mut workspace, result) = task.await.unwrap();
    assert!(matches!(result, Err(BrowserError::Cancelled)));
    assert!(matches!(
        workspace.observe().await,
        Err(BrowserError::Cancelled)
    ));
    assert_eq!(state.lock().unwrap().inputs, 1);
    workspace.shutdown().await;
    server.abort();
}

#[tokio::test]
async fn unchanged_bounds_overlay_paint_order_or_focus_switch_invalidates_frame() {
    let (mut workspace, state, server) = fixture().await;
    let frame = workspace.frame().await.unwrap();
    state.lock().unwrap().overlay = true;
    assert!(matches!(
        workspace
            .input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 })
            .await,
        Err(BrowserError::StalePage)
    ));
    let frame = workspace.frame().await.unwrap();
    state.lock().unwrap().reordered = true;
    assert!(matches!(
        workspace
            .input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 })
            .await,
        Err(BrowserError::StalePage)
    ));
    let frame = workspace.frame().await.unwrap();
    state.lock().unwrap().focused_backend = 6;
    assert!(matches!(
        workspace
            .input(
                &frame.frame_token,
                WorkspaceInput::InsertText {
                    text: "synthetic".into()
                }
            )
            .await,
        Err(BrowserError::StalePage)
    ));
    assert_eq!(state.lock().unwrap().inputs, 0);
    workspace.shutdown().await;
    server.abort();
}
#[test]
fn chromium_empty_style_nontext_root_is_supported_but_text_style_shape_is_checked() {
    let snapshot = snapshot();
    assert_eq!(snapshot["documents"][0]["layout"]["styles"][0], json!([]));
    assert!(project_observation("https://example.org/", &ax(), &snapshot).is_ok());
    let mut malformed = snapshot.clone();
    malformed["documents"][0]["layout"]["styles"][2] = json!([]);
    assert!(matches!(
        project_observation("https://example.org/", &ax(), &malformed),
        Err(BrowserError::Protocol)
    ));
}

#[test]
fn snapshot_absent_string_sentinels_are_field_aware_and_bad_indexes_fail() {
    let strings = vec![json!("field"), json!("")];
    assert_eq!(snapshot_string(&strings, &json!(-1)).unwrap(), None);
    assert_eq!(snapshot_string(&strings, &json!(1)).unwrap(), Some(""));
    assert!(required_snapshot_string(&strings, &json!(-1)).is_err());
    for invalid in [json!(-2), json!(2), json!(u64::MAX), json!("0")] {
        assert!(snapshot_string(&strings, &invalid).is_err());
    }
    let mut snapshot = snapshot();
    assert!(project_observation("https://example.org/", &ax(), &snapshot).is_ok());
    snapshot["documents"][0]["nodes"]["attributes"][5][3] = json!(-1);
    assert!(project_observation("https://example.org/", &ax(), &snapshot).is_ok());
    snapshot["documents"][0]["nodes"]["attributes"][5][3] = json!(99999);
    assert!(matches!(
        project_observation("https://example.org/", &ax(), &snapshot),
        Err(BrowserError::Protocol)
    ));
}
