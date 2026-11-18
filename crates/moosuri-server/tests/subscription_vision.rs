//! Synthetic official-CLI image proof through the authenticated HTTP boundary.
#![cfg(feature = "test-fixtures")]
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use moosuri_core::{ModelSelection, ModelSource};
use moosuri_server::{ServerConfig, router_with_runtime};
use moosuri_store::{LoginResult, Store};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
async fn request(
    app: &Router,
    u: &LoginResult,
    path: &str,
    body: Value,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("host", "vision.example.test")
                .header("origin", "https://vision.example.test")
                .header("content-type", "application/json")
                .header("cookie", format!("moosuri_session={}", u.token))
                .header("x-moosuri-account", &u.user.account().id)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
fn pixels(bytes: &[u8]) -> String {
    let image = image::load_from_memory(bytes).unwrap().to_rgb8();
    (0..16)
        .map(|n| {
            let p = image.get_pixel((n % 4) * 32 + 16, (n / 4) * 32 + 16).0;
            if p[0] > 150 && p[1] > 150 {
                'Y'
            } else if p[0] > p[1] && p[0] > p[2] {
                'R'
            } else if p[1] > p[2] {
                'G'
            } else {
                'B'
            }
        })
        .collect()
}
#[tokio::test]
async fn native_pixels_prove_only_exact_tuple_and_failures_never_grant() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path()).unwrap();
    store
        .bootstrap(
            "Synthetic vision owner".into(),
            "vision@example.test".into(),
            "Synthetic-Vision-Password-2026".into(),
            false,
        )
        .await
        .unwrap();
    let u = store
        .login(
            "vision@example.test".into(),
            "Synthetic-Vision-Password-2026".into(),
        )
        .await
        .unwrap();
    let f = moosuri_connectors::claude_code::fixture::Fixture::new();
    let b = store
        .create_claude_binding(
            &u.user,
            "Synthetic native image".into(),
            f.profile.digest().into(),
            f.profile.root_id(),
        )
        .await
        .unwrap();
    store
        .record_claude_probe(&u.user, b.clone(), "claude-fixture-v1".into())
        .await
        .unwrap();
    store
        .record_claude_model_efforts(
            &u.user,
            b.clone(),
            "claude-fixture-v1".into(),
            vec!["high".into()],
        )
        .await
        .unwrap();
    let selection = ModelSelection {
        source: ModelSource::PersonalClaudeCode,
        provider_id: b.id.clone(),
        provider_revision: b.revision,
        model: "claude-fixture-v1".into(),
        effort: None,
    };
    let high = ModelSelection {
        effort: Some("high".into()),
        ..selection.clone()
    };
    let runtime = moosuri_agent::Runtime::start_with_google_setup_claude(
        store.clone(),
        None,
        None,
        None,
        "https://vision.example.test/callback".into(),
        Some(Arc::new(f.profile.clone())),
    );
    let app = router_with_runtime(
        store.clone(),
        ServerConfig::new("https://vision.example.test", false).unwrap(),
        runtime.clone(),
    );
    let path = format!("/api/v1/settings/claude/{}/vision-probe", b.id);
    let body = json!({"revision":b.revision,"model":selection.model,"effort":null});
    f.behavior("vision-probe");
    let output = f.directory.path().to_owned();
    let observer = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(10), async {
            while !output.join("vision-input.jpg").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let bytes = std::fs::read(output.join("vision-input.jpg")).unwrap();
            let answer = pixels(&bytes);
            assert_eq!(answer.len(), 16);
            std::fs::write(output.join("vision-answer"), answer).unwrap();
        })
        .await
        .unwrap();
    });
    let result = request(&app, &u, &path, body.clone()).await;
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<Value>(&result.into_body().collect().await.unwrap().to_bytes())
            .unwrap(),
        json!({"vision":true})
    );
    observer.await.unwrap();
    assert!(
        store
            .personal_vision_proven(&u.user, &selection)
            .await
            .unwrap()
    );
    assert!(!store.personal_vision_proven(&u.user, &high).await.unwrap());
    for behavior in [
        "vision-wrong",
        "vision-refusal",
        "vision-truncated",
        "identity-mismatch",
    ] {
        f.behavior(behavior);
        let response = request(
            &app,
            &u,
            &path,
            json!({"revision":b.revision,"model":selection.model,"effort":"high"}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{behavior}");
        assert!(
            !store.personal_vision_proven(&u.user, &high).await.unwrap(),
            "{behavior}"
        );
    }
    let launches = f.launches();
    let response = request(
        &app,
        &u,
        &path,
        json!({"revision":b.revision+1,"model":selection.model,"effort":null}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(f.launches(), launches);
    runtime.shutdown().await;
}
