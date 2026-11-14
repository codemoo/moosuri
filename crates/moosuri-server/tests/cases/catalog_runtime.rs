//! Synthetic composed index/artifact + independently verified GitHub object acquisition.
#![cfg(feature = "test-fixtures")]
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
#[tokio::test]
async fn configured_catalog_preview_requires_independent_origin_and_keeps_user_install_explicit() {
    let _g = TEST_LOCK.lock().await;
    let files = BTreeMap::from([
        (
            "LICENSE",
            include_str!("../../../../community/skills/source-briefing/LICENSE"),
        ),
        (
            "SKILL.md",
            include_str!("../../../../community/skills/source-briefing/SKILL.md"),
        ),
        (
            "examples.json",
            include_str!("../../../../community/skills/source-briefing/examples.json"),
        ),
        (
            "moosuri.toml",
            include_str!("../../../../community/skills/source-briefing/moosuri.toml"),
        ),
    ]);
    let repository = "https://github.com/example/recipes";
    let commit = "1".repeat(40);
    let directory = "recipe";
    let name = "source-briefing";
    let mut h = Sha256::new();
    for s in [
        "moosuri-public-github-envelope-v2",
        repository,
        directory,
        &commit,
        name,
    ]
    .into_iter()
    .chain(files.iter().flat_map(|(k, v)| [*k, *v]))
    {
        h.update((s.len() as u64).to_be_bytes());
        h.update(s.as_bytes());
    }
    let digest = format!("{:x}", h.finalize());
    let mut h = Sha256::new();
    for s in [repository, directory] {
        h.update((s.len() as u64).to_be_bytes());
        h.update(s.as_bytes());
    }
    let id = format!("github-{:x}", h.finalize());
    let artifact=json!({"format_version":2,"repository":repository,"directory":directory,"commit":commit,"slug":name,"files":files}).to_string();
    let index=json!({"format_version":1,"repository":repository,"source_commit":commit,"packages":[{"directory":directory,"id":id,"name":name,"version":"1.0.0","description":"Prepare a concise briefing from the user's saved sources and memories.","license":"Apache-2.0","capabilities":["knowledge_search","knowledge_read_source","knowledge_read_memory"],"envelope_version":2,"content_digest":digest,"artifact":format!("package-{digest}.json"),"artifact_sha256":format!("{:x}",Sha256::digest(&artifact))}]}).to_string();
    let origins_available = Arc::new(AtomicBool::new(false));
    let github_calls = Arc::new(AtomicUsize::new(0));
    let raw_calls = Arc::new(AtomicUsize::new(0));
    let git_flag = origins_available.clone();
    let count = github_calls.clone();
    let raw_count = raw_calls.clone();
    let app=Router::new().fallback(axum::routing::get(move|request:Request<Body>|{let index=index.clone();let artifact=artifact.clone();let files=files.clone();let git_flag=git_flag.clone();let count=count.clone();let raw_count=raw_count.clone();async move {
        assert!(request.headers().get("cookie").is_none());assert!(request.headers().get("authorization").is_none());assert!(request.uri().query().is_none());
        let path=request.uri().path();
        if !path.starts_with("/repos/") {raw_count.fetch_add(1,Ordering::SeqCst);return axum::http::Response::builder().header("content-type","application/json").header("etag","\"synthetic-index\"").body(Body::from(if path.ends_with("catalog.json"){index}else{artifact})).unwrap();}
        count.fetch_add(1,Ordering::SeqCst);if !git_flag.load(Ordering::SeqCst){return axum::http::Response::builder().status(403).body(Body::empty()).unwrap();}
        let root="2".repeat(40);let dir="3".repeat(40);
        let hashes=[("LICENSE","909a61d6986dfda1c484444903f7883052b32c27"),("SKILL.md","674a029eb0a4ab1520b8a3cf48d12eb36a1ab9cf"),("examples.json","7eaa1adfab7fd9c6db1f1696124360859f5fbdbd"),("moosuri.toml","7a1ca71598a8ee49fe16db87297133739b4f09f9")];
        let value=if path.contains("/git/commits/"){json!({"sha":"1".repeat(40),"tree":{"sha":root}})}else if path.ends_with(&root){json!({"sha":root,"truncated":false,"tree":[{"path":"recipe","mode":"040000","type":"tree","sha":dir}]})}else if path.ends_with(&dir){json!({"sha":dir,"truncated":false,"tree":hashes.iter().map(|(name,hash)|json!({"path":name,"mode":"100644","type":"blob","sha":hash,"size":files[name].len()})).collect::<Vec<_>>()})}else{let (name,hash)=hashes.iter().find(|(_,h)|path.ends_with(h)).unwrap();let content=files[name];json!({"sha":hash,"size":content.len(),"encoding":"base64","content":STANDARD.encode(content)})};
        axum::http::Response::builder().header("content-type","application/json").body(Body::from(value.to_string())).unwrap()
    }}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}/", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_d, store, _default) = fixture(false).await;
    let config = ServerConfig::new(ORIGIN, false)
        .unwrap()
        .with_community_catalog(repository, "catalog", "release")
        .unwrap()
        .with_community_catalog_fixtures(&origin, &origin)
        .unwrap();
    let app = router(store.clone(), config);
    assert_eq!(
        send(&app, "GET", "/api/v1/community-skills", None, Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(raw_calls.load(Ordering::SeqCst), 0);
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let cards = value(
        send(
            &app,
            "GET",
            "/api/v1/community-skills",
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    let offer = cards
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap();
    assert_eq!(offer["readiness"], "preview_required");
    assert_eq!(github_calls.load(Ordering::SeqCst), 0);
    assert_eq!(raw_calls.load(Ordering::SeqCst), 1);
    let path = format!("/api/v1/community-skills/catalog/{id}/preview");
    assert_eq!(
        send(
            &app,
            "POST",
            &path,
            Some(&cookie),
            json!({"expected_digest":digest})
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert!(
        github_calls.load(Ordering::SeqCst) > 0,
        "self-declared artifact must still require the original verifier"
    );
    assert!(store.claim_agent_run().await.unwrap().is_none());
    origins_available.store(true, Ordering::SeqCst);
    let preview = send(
        &app,
        "POST",
        &path,
        Some(&cookie),
        json!({"expected_digest":digest}),
    )
    .await;
    assert_eq!(preview.status(), StatusCode::OK);
    let preview = value(preview).await;
    assert_eq!(preview["catalog"]["verified"], true);
    assert!(preview["installed_digest"].is_null());
    let calls = github_calls.load(Ordering::SeqCst);
    assert_eq!(
        send(
            &app,
            "POST",
            &path,
            Some(&cookie),
            json!({"expected_digest":digest})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        github_calls.load(Ordering::SeqCst),
        calls,
        "verified exact source cache is reused"
    );
    let installed = send(
        &app,
        "POST",
        &format!("/api/v1/community-skills/{id}"),
        Some(&cookie),
        json!({"action":"install","expected_revision":0,"expected_digest":digest}),
    )
    .await;
    assert_eq!(installed.status(), StatusCode::OK);
    assert!(store.claim_agent_run().await.unwrap().is_none());
    let installed = value(installed).await;
    assert_eq!(
        installed
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap()["installed_digest"],
        digest
    );
    task.abort();
}
