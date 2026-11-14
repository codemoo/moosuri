//! Executes the actual native receiver process; only opt-in loopback fixture builds.
#![cfg(feature = "test-fixtures")]
use axum::{
    Json, Router,
    body::Bytes,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[tokio::test]
async fn installed_receiver_process_uses_owner_api_and_bounded_encrypted_stdin() {
    let ticket = "00000000-0000-4000-8000-000000000001";
    let app = Router::new()
        .route(
            "/api/v1/auth/session",
            get(|h: HeaderMap| async move {
                assert_eq!(h["cookie"], "moosuri_session=synthetic-owner-session");
                assert_eq!(h["x-moosuri-account"], "synthetic-account");
                Json(json!({"user":{"id":"synthetic-account"}}))
            }),
        )
        .route(
            &format!("/api/v1/settings/chatgpt/handoffs/{ticket}/import"),
            post(|h: HeaderMap, b: Bytes| async move {
                assert_eq!(h["cookie"], "moosuri_session=synthetic-owner-session");
                assert_eq!(h["x-moosuri-account"], "synthetic-account");
                assert_eq!(b.as_ref(), [7u8; 64]);
                (
                    StatusCode::OK,
                    Json(json!({"phase":"imported","receipt":null,"receipt_mac":null})),
                )
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let frame=serde_json::to_vec(&json!({"version":1,"origin":origin,"cookie":"moosuri_session=synthetic-owner-session","account":"synthetic-account","ticket":ticket,"envelope":URL_SAFE_NO_PAD.encode([7u8;64])})).unwrap();
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .args(["subscriptions", "chatgpt-receive", "--stdin"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_u32(frame.len() as u32).await.unwrap();
    input.write_all(&frame).await.unwrap();
    input.shutdown().await.unwrap();
    drop(input);
    let mut output = child.stdout.take().unwrap();
    let length = output.read_u32().await.unwrap();
    assert!(length < 16384);
    let mut data = vec![0; length as usize];
    output.read_exact(&mut data).await.unwrap();
    let safe: serde_json::Value = serde_json::from_slice(&data).unwrap();
    assert_eq!(safe["phase"], "imported");
    assert!(child.wait().await.unwrap().success());
    let printed = String::from_utf8(data).unwrap();
    for secret in [
        "synthetic-owner-session",
        "transfer_key",
        "refresh_token",
        "access_token",
    ] {
        assert!(!printed.contains(secret));
    }
    task.abort();
}
