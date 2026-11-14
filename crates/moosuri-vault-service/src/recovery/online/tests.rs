use super::*;
use crate::http::{Adapter, Config, router_shared};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;
fn private(parent: &Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    p
}
fn owner() -> protocol::Owner {
    protocol::Owner {
        tenant: uuid::Uuid::new_v4(),
        account: uuid::Uuid::new_v4(),
        installation: uuid::Uuid::new_v4(),
        epoch: 1,
    }
}
#[tokio::test]
async fn ongoing_snapshot_worker_keeps_http_and_writer_available_and_drains() {
    let t = tempfile::tempdir().unwrap();
    fs::set_permissions(t.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let source = t.path().join("source");
    let exports = private(t.path(), "exports");
    let db = Arc::new(Mutex::new(Database::open(&source).unwrap()));
    let identity = age::x25519::Identity::generate();
    assert!(Worker::start(db.clone(), &source, &identity.to_public().to_string(), 300).is_err());
    assert!(Worker::start(db.clone(), &exports, &identity.to_public().to_string(), 1).is_err());
    let app = router_shared(
        db.clone(),
        Adapter::new("http://127.0.0.1:9/internal/v1/vault", "00".repeat(32)).unwrap(),
        Config::new(
            "https://vault.example.test",
            "https://assistant.example.test",
            false,
        )
        .unwrap(),
    );
    let worker =
        Worker::start(db.clone(), &exports, &identity.to_public().to_string(), 300).unwrap();
    let response = app
        .oneshot(
            Request::builder()
                .uri("/health/live")
                .header("host", "vault.example.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let o = owner();
    db.lock()
        .unwrap()
        .create_session("new-live-session", &o)
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if fs::read_dir(&exports).unwrap().any(|e| {
                e.unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".head.json")
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    worker.shutdown().await;
    assert_eq!(fs::read_dir(&exports).unwrap().count(), 2);
    let head = fs::read_dir(&exports)
        .unwrap()
        .find(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".head.json")
        })
        .unwrap()
        .unwrap()
        .path();
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(head).unwrap()).unwrap();
    assert_eq!(receipt["snapshot"]["format"], SNAPSHOT_FORMAT);
    assert!(receipt.get("seal").is_none());
    assert!(!source.join("vault.sealed").exists());
    assert!(db.lock().unwrap().session("new-live-session").unwrap() == o);
}
#[test]
fn ongoing_snapshot_missing_capture_evidence_never_publishes_or_seals() {
    let t = tempfile::tempdir().unwrap();
    fs::set_permissions(t.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let source = t.path().join("source");
    let exports = private(t.path(), "exports");
    let mut db = Database::open(&source).unwrap();
    let captured = db.capture_snapshot(&exports).unwrap();
    let stage = captured.stage.0.clone();
    fs::remove_file(stage.join("vault.journal")).unwrap();
    let identity = age::x25519::Identity::generate();
    assert!(
        captured
            .encrypt(
                &identity.to_public().to_string(),
                &exports.join("bad.age"),
                &exports.join("bad.head.json")
            )
            .is_err()
    );
    assert!(!stage.exists());
    assert_eq!(fs::read_dir(&exports).unwrap().count(), 0);
    assert!(!source.join("vault.sealed").exists());
    db.create_session("still-live", &owner()).unwrap();
}
