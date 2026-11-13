use super::*;
use std::sync::atomic::Ordering;
fn fixture(account: Uuid, responses: &[Value]) -> CapsuleReader {
    let config = CapsuleConfig {
        image_id: format!("sha256:{}", "a".repeat(64)),
        network_id: "b".repeat(64),
        proxy_container_id: "c".repeat(64),
        proxy_ip: "172.29.0.2".into(),
        owner: Uuid::new_v4(),
        project: "test-browser".into(),
        seccomp_file: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../deployment/browser/seccomp.json"),
        verified: true,
        smoke_evidence_sha256: "d".repeat(64),
        javascript_enabled: false,
    };
    let mut reader = CapsuleReader::new(config).unwrap();
    let ready = json!({"ready":true,"workspace":true,"javascript_enabled":false,"tenant":account});
    let quote = |v: &Value| format!("'{}'", v.to_string().replace('\'', "'\\''"));
    let mut script = format!("printf '%s\\n' {};", quote(&ready));
    for response in responses {
        script.push_str(&format!(
            "read -r command || exit; printf '%s\\n' {};",
            quote(response)
        ));
    }
    script.push_str("exec /bin/sleep 30");
    reader.fixture = Some(Arc::new(Fixture {
        script,
        cleaned: 0.into(),
    }));
    reader
}
fn page() -> Value {
    json!({"ok":true,"result":{"page":{"url":"https://example.org/task","title":"Task","text":"not refundable","links":[],"truncated":false,"warning":"synthetic","retrieved_at_unix_ms":1}}})
}
async fn cleaned(reader: &CapsuleReader) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while reader
            .fixture
            .as_ref()
            .unwrap()
            .cleaned
            .load(Ordering::SeqCst)
            == 0
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(reader.capacity.available_permits(), 2);
}
#[tokio::test]
async fn persistent_protocol_keeps_owner_between_calls_and_shutdown_reaps() {
    let account = Uuid::new_v4();
    let reader = fixture(
        account,
        &[
            page(),
            page(),
            json!({"ok":false,"code":"stale_page"}),
            page(),
        ],
    );
    let mut workspace = reader
        .start_workspace(account, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(reader.capacity.available_permits(), 1);
    assert_eq!(
        workspace
            .open("https://example.org/task")
            .await
            .unwrap()
            .page
            .text,
        "not refundable"
    );
    assert_eq!(
        workspace.observe().await.unwrap().page.url,
        "https://example.org/task"
    );
    assert_eq!(
        reader
            .fixture
            .as_ref()
            .unwrap()
            .cleaned
            .load(Ordering::SeqCst),
        0
    );
    assert_eq!(
        workspace
            .input(
                &"e".repeat(64),
                WorkspaceInput::Key {
                    key: WorkspaceKey::Enter
                }
            )
            .await,
        Err(BrowserError::StalePage)
    );
    assert!(workspace.observe().await.is_ok());
    workspace.shutdown().await.unwrap();
    cleaned(&reader).await;
}
#[tokio::test]
async fn malformed_output_poisoning_and_wrong_tenant_never_reused() {
    let account = Uuid::new_v4();
    let reader = fixture(
        account,
        &[json!({"ok":true,"result":{"page":{"url":"http://127.0.0.1/"}}})],
    );
    let mut workspace = reader
        .start_workspace(account, &CancellationToken::new())
        .await
        .unwrap();
    assert!(workspace.observe().await.is_err());
    assert!(matches!(
        workspace.observe().await,
        Err(BrowserError::Cancelled)
    ));
    workspace.shutdown().await.unwrap();
    let reader = fixture(Uuid::new_v4(), &[]);
    assert!(matches!(
        reader
            .start_workspace(account, &CancellationToken::new())
            .await,
        Err(BrowserError::Protocol)
    ));
    cleaned(&reader).await;
}
#[tokio::test]
async fn drop_pending_action_and_external_cancel_finish_cleanup_independently() {
    let account = Uuid::new_v4();
    let reader = fixture(account, &[]);
    let mut workspace = reader
        .start_workspace(account, &CancellationToken::new())
        .await
        .unwrap();
    let task = tokio::spawn(async move { workspace.observe().await });
    tokio::time::sleep(Duration::from_millis(30)).await;
    task.abort();
    let _ = task.await;
    cleaned(&reader).await;
    let reader = fixture(account, &[]);
    let cancel = CancellationToken::new();
    let mut workspace = reader.start_workspace(account, &cancel).await.unwrap();
    cancel.cancel();
    assert!(workspace.is_closed());
    assert!(matches!(
        workspace.observe().await,
        Err(BrowserError::Cancelled)
    ));
    workspace.shutdown().await.unwrap();
    cleaned(&reader).await;
}
#[tokio::test]
async fn output_bound_and_capacity_apply_to_workspaces_too() {
    let account = Uuid::new_v4();
    let reader = fixture(
        account,
        &[json!({"ok":true,"result":{"unexpected":"x".repeat(MAX_RESPONSE)}})],
    );
    let mut first = reader
        .start_workspace(account, &CancellationToken::new())
        .await
        .unwrap();
    let second = reader
        .start_workspace(account, &CancellationToken::new())
        .await
        .unwrap();
    assert!(matches!(
        reader
            .start_workspace(account, &CancellationToken::new())
            .await,
        Err(BrowserError::Capacity)
    ));
    assert!(matches!(
        first.observe().await,
        Err(BrowserError::OutputTooLarge)
    ));
    first.shutdown().await.unwrap();
    second.shutdown().await.unwrap();
    assert_eq!(reader.capacity.available_permits(), 2);
}
#[test]
fn human_input_schema_and_bounds_do_not_accept_scripts_or_unbounded_data() {
    for value in [
        json!({"kind":"evaluate","script":"hello"}),
        json!({"kind":"key","key":"F12"}),
        json!({"kind":"click","x":1,"y":1,"script":"extra"}),
    ] {
        assert!(serde_json::from_value::<WorkspaceInput>(value).is_err());
    }
    assert!(WorkspaceInput::Click { x: 1280, y: 0 }.validate().is_err());
    assert!(
        WorkspaceInput::Scroll {
            delta_x: 0,
            delta_y: i32::MIN
        }
        .validate()
        .is_err()
    );
    assert!(
        WorkspaceInput::InsertText {
            text: "x".repeat(4097)
        }
        .validate()
        .is_err()
    );
    assert!(
        WorkspaceInput::InsertText {
            text: "fixture\0secret".into()
        }
        .validate()
        .is_err()
    );
    assert!(
        WorkspaceInput::InsertText {
            text: "synthetic password".into()
        }
        .validate()
        .is_ok()
    );
}

#[tokio::test]
async fn dropping_startup_future_keeps_cleanup_owner_running() {
    let account = Uuid::new_v4();
    let mut reader = fixture(account, &[]);
    reader.fixture = Some(Arc::new(Fixture {
        script: "exec /bin/sleep 30".into(),
        cleaned: 0.into(),
    }));
    let cloned = reader.clone();
    let task = tokio::spawn(async move {
        cloned
            .start_workspace(account, &CancellationToken::new())
            .await
    });
    tokio::time::sleep(Duration::from_millis(30)).await;
    task.abort();
    let _ = task.await;
    cleaned(&reader).await;
}
#[test]
fn frame_projection_checks_identity_url_png_dimensions_and_output_bounds() {
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend(1280u32.to_be_bytes());
    png.extend(900u32.to_be_bytes());
    let value = json!({"png_base64":base64::engine::general_purpose::STANDARD.encode(png),"width":1280,"height":900,"url":"https://example.org/","frame_token":"a".repeat(64)});
    assert!(frame(value.clone()).is_ok());
    for (key, bad) in [
        ("url", json!("http://127.0.0.1/")),
        ("frame_token", json!("unbound")),
        ("width", json!(10)),
        ("png_base64", json!("x".repeat(3 * 1024 * 1024 + 1))),
    ] {
        let mut invalid = value.clone();
        invalid[key] = bad;
        assert!(frame(invalid).is_err());
    }
}

#[test]
fn key_names_match_the_human_ui_snake_case_contract() {
    assert_eq!(
        serde_json::to_value(WorkspaceKey::Enter).unwrap(),
        json!("enter")
    );
    assert_eq!(
        serde_json::to_value(WorkspaceKey::ArrowLeft).unwrap(),
        json!("arrow_left")
    );
    assert!(serde_json::from_value::<WorkspaceKey>(json!("Enter")).is_err());
}
