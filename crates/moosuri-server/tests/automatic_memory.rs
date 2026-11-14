//! Actual authenticated HTTP settings never turn source text into authority.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use moosuri_core::*;
use moosuri_server::{ServerConfig, router};
use moosuri_store::{CredentialChange, ProviderChange, Store};
use serde_json::{Value, json};
use tower::ServiceExt;
async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    account: Option<&str>,
    body: Value,
) -> axum::response::Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "assistant.example.test")
        .header("origin", "https://assistant.example.test")
        .header("content-type", "application/json");
    if let Some(t) = token {
        request = request.header("cookie", format!("moosuri_session={t}"));
    }
    if let Some(a) = account {
        request = request.header("x-moosuri-account", a);
    }
    app.clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
#[tokio::test]
async fn extraction_controls_require_current_account_and_do_not_backfill_or_grant_new_tools() {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Synthetic".into(),
            "owner@example.test".into(),
            "Synthetic-server-password".into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login(
            "owner@example.test".into(),
            "Synthetic-server-password".into(),
        )
        .await
        .unwrap();
    let u = &login.user;
    let provider = store
        .create_provider(
            u,
            ProviderChange {
                name: "Synthetic".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(u, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(u, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            u,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: provider.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let c = store
        .create_collection(u, "Synthetic".into(), "synthetic-scope".into())
        .await
        .unwrap();
    let app = router(
        store.clone(),
        ServerConfig::new("https://assistant.example.test", false).unwrap(),
    );
    let path = format!("/api/v1/knowledge/collections/{}/extraction", c.id);
    let setting = json!({"expected_revision":0,"enabled":true,"attribution_mode":"reported","reported_speaker":"","daily_calls":10,"daily_input_bytes":24000,"daily_output_tokens":4000});
    assert_eq!(
        send(&app, "GET", &path, None, None, json!(null))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &app,
            "PUT",
            &path,
            Some(&login.token),
            Some("different-account"),
            setting.clone()
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let result = send(
        &app,
        "PUT",
        &path,
        Some(&login.token),
        Some(&u.account().id),
        setting,
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    let result: Value =
        serde_json::from_slice(&result.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(result["enabled"], true);
    assert_eq!(result["state"], "idle");
    assert!(store.claim_memory_extraction().await.unwrap().is_none());
    assert!(store.claim_agent_run().await.unwrap().is_none());
}
