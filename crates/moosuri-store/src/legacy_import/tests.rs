use super::*;
use crate::tests::{TEST_LOCK, member};
use std::os::unix::fs::PermissionsExt;
fn private() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}
fn event(id: &str, revision: i64, operation: Value) -> Value {
    serde_json::json!({"owner":"legacy-owner","id":id,"revision":revision,"operation":operation})
}
fn source(version: &str, text: &str) -> Value {
    serde_json::json!({"kind":"source_upsert","scope":"synthetic-source-scope","external_id":"legacy-external-1","version":version,"title":"Synthetic source","text":text,"speaker":"Synthetic speaker","observed_at":1,"valid_from":null,"valid_until":null})
}
fn memory(text: &str, evidence: Value) -> Value {
    serde_json::json!({"kind":"memory_upsert","subject":"Synthetic fact","text":text,"speaker":null,"valid_from":null,"valid_until":null,"evidence":evidence})
}
fn document(body: &str, references: Value) -> Value {
    serde_json::json!({"kind":"document_upsert","title":"Synthetic document","body":body,"references":references})
}
fn bundle(events: Vec<Value>) -> Vec<u8> {
    let events = events
        .into_iter()
        .enumerate()
        .map(|(i, mut e)| {
            e["sequence"] = serde_json::json!(i + 1);
            e
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&serde_json::json!({"format":1,"purpose":"moosuri-private-transfer-v1","bundle_id":Uuid::new_v4().to_string(),"source_manifest":{"deployment_id":"synthetic-legacy","source_version":"synthetic-version","source_schema":"synthetic-contract-v1","exported_at":1,"coverage":"selected source/memory/document history only; no authority"},"events":events})).unwrap()
}
fn enrollment() -> Enrollment {
    Enrollment {
        name: "New staging owner".into(),
        email: "new-owner@example.test".into(),
        password: Zeroizing::new("Synthetic-import-new-password".into()),
    }
}
#[tokio::test]
async fn ordered_private_import_history_withdrawal_quarantine_restart_and_backup() {
    let _guard = TEST_LOCK.lock().await;
    let temp = private();
    let input = temp.path().join("private.json");
    let bytes = bundle(vec![
        event("source-1", 1, source("v1", "old quoted source")),
        event(
            "memory-1",
            1,
            memory(
                "Old derived fact",
                serde_json::json!([{"source":"source-1","revision":1,"start_byte":0,"end_byte":3,"quote":"old"}]),
            ),
        ),
        event(
            "document-1",
            1,
            document(
                "Old dependent report",
                serde_json::json!([{"kind":"memory","id":"memory-1","revision":1}]),
            ),
        ),
        event("source-1", 2, source("v2", "new quoted source")),
        event(
            "memory-1",
            2,
            memory(
                "Corrected derived fact",
                serde_json::json!([{"source":"source-1","revision":2,"start_byte":0,"end_byte":3,"quote":"new"}]),
            ),
        ),
        event(
            "document-history",
            1,
            document("Original independent report", serde_json::json!([])),
        ),
        event(
            "document-history",
            2,
            document("Current independent report", serde_json::json!([])),
        ),
        event(
            "memory-forget",
            1,
            memory("Forgotten canary", serde_json::json!([])),
        ),
        event(
            "memory-forget",
            2,
            serde_json::json!({"kind":"memory_forget"}),
        ),
        event(
            "forgotten-before-export",
            1,
            serde_json::json!({"kind":"memory_forget"}),
        ),
        event(
            "poisoned-document",
            1,
            document("Do not expose after bad correction", serde_json::json!([])),
        ),
        event(
            "poisoned-document",
            2,
            document(
                "Invalid correction",
                serde_json::json!([{"kind":"source","id":"missing","revision":1}]),
            ),
        ),
        event(
            "document-child",
            1,
            document(
                "Invalid child",
                serde_json::json!([{"kind":"document","id":"poisoned-document","revision":1}]),
            ),
        ),
        event(
            "old-work",
            1,
            serde_json::json!({"kind":"mission","state":"executing","provider_credential":"synthetic-credential-canary"}),
        ),
        {
            let mut e = event(
                "foreign",
                1,
                memory("Foreign owner canary", serde_json::json!([])),
            );
            e["owner"] = serde_json::json!("other-owner");
            e
        },
    ]);
    fs::write(&input, &bytes).unwrap();
    fs::set_permissions(&input, fs::Permissions::from_mode(0o600)).unwrap();
    let dest = temp.path().join("data");
    let reports = temp.path().join("reports");
    let report = import_bundle(&input, "legacy-owner", enrollment(), &dest, &reports)
        .await
        .unwrap();
    assert_eq!(report.total_events, 15);
    assert_eq!(report.imported_events + report.quarantined_events, 15);
    assert_eq!(report.quarantined_events, 4);
    assert_eq!(fs::read(&input).unwrap(), bytes);
    assert!(
        !fs::read_to_string(reports.join("quarantine.json"))
            .unwrap()
            .contains("synthetic-credential-canary")
    );
    let store = Store::open(&dest).unwrap();
    let login = store
        .login(
            "new-owner@example.test".into(),
            "Synthetic-import-new-password".into(),
        )
        .await
        .unwrap();
    let user = login.user;
    let m = report
        .mappings
        .iter()
        .find(|m| m.legacy_id == "memory-1")
        .unwrap();
    assert_eq!(
        store
            .memory(&user, m.target_id.clone(), Some(1))
            .await
            .unwrap()
            .draft
            .text,
        "Old derived fact"
    );
    assert_eq!(
        store
            .memory(&user, m.target_id.clone(), None)
            .await
            .unwrap()
            .draft
            .text,
        "Corrected derived fact"
    );
    let src = report
        .mappings
        .iter()
        .find(|m| m.legacy_id == "source-1")
        .unwrap();
    let source = store
        .source(&user, src.target_id.clone(), Some(1))
        .await
        .unwrap();
    assert_eq!(source.source.external_id, "legacy-external-1");
    assert_eq!(source.source.version, "v1");
    assert!(source.provenance.is_none());
    let doc = report
        .mappings
        .iter()
        .find(|m| m.legacy_id == "document-history")
        .unwrap();
    assert_eq!(
        store
            .artifact(&user, doc.target_id.clone(), None)
            .await
            .unwrap()
            .draft
            .unwrap()
            .body,
        "Current independent report"
    );
    assert_eq!(
        store
            .artifact(&user, doc.target_id.clone(), Some(1))
            .await
            .unwrap()
            .draft
            .unwrap()
            .body,
        "Original independent report"
    );
    let dependent = report
        .mappings
        .iter()
        .find(|m| m.legacy_id == "document-1")
        .unwrap();
    assert!(
        store
            .artifact(&user, dependent.target_id.clone(), None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
    let forgotten = report
        .mappings
        .iter()
        .find(|m| m.legacy_id == "memory-forget")
        .unwrap();
    assert!(
        store
            .memory(&user, forgotten.target_id.clone(), None)
            .await
            .is_err()
    );
    let poison = report
        .mappings
        .iter()
        .find(|m| m.legacy_id == "poisoned-document")
        .unwrap();
    assert!(
        store
            .artifact(&user, poison.target_id.clone(), None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
    let b = member(&store, &user, "fresh-other@example.test").await.user;
    assert!(store.source(&b, src.target_id.clone(), None).await.is_err());
    assert!(store.memory(&b, m.target_id.clone(), None).await.is_err());
    drop(store);
    assert!(matches!(
        import_bundle(&input, "legacy-owner", enrollment(), &dest, &reports).await,
        Err(Error::Exists)
    ));
    let kit = temp.path().join("kit");
    fs::DirBuilder::new().mode(0o700).create(&kit).unwrap();
    let exports = temp.path().join("exports");
    fs::DirBuilder::new().mode(0o700).create(&exports).unwrap();
    let identity = kit.join("identity");
    let recipient = backup::keygen(&identity).unwrap();
    backup::setup(&dest, &recipient, &identity).unwrap();
    backup::create(&dest, &exports.join("import.age")).unwrap();
    let conn = rusqlite::Connection::open(dest.join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert!(
        !conn
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
    let reports_in_db: i64 = conn
        .query_row(
            "SELECT count(*) FROM personal_artifact_versions WHERE draft LIKE '%imported-%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(reports_in_db > 0);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM knowledge_memory_versions WHERE draft LIKE '%Forgotten canary%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
#[tokio::test]
async fn importer_rejects_unsafe_input_and_leaves_no_openable_partial_stage() {
    let _guard = TEST_LOCK.lock().await;
    let temp = private();
    let input = temp.path().join("invalid.json");
    fs::write(&input, b"not a bundle").unwrap();
    fs::set_permissions(&input, fs::Permissions::from_mode(0o600)).unwrap();
    let dest = temp.path().join("data");
    let reports = temp.path().join("reports");
    assert!(
        import_bundle(&input, "legacy-owner", enrollment(), &dest, &reports)
            .await
            .is_err()
    );
    assert!(!dest.exists());
    assert!(!reports.exists());
    let stage = Stage::new(temp.path()).unwrap();
    let path = stage.path.join("data");
    let store = open_private_stage(&path).unwrap();
    drop(store);
    assert!(Store::open(&path).is_err());
}

#[tokio::test]
async fn importer_rejects_reserved_final_name_or_ancestor_before_creating_outputs() {
    let _guard = TEST_LOCK.lock().await;
    let temp = private();
    let input = temp.path().join("private.json");
    fs::write(&input, bundle(vec![])).unwrap();
    fs::set_permissions(&input, fs::Permissions::from_mode(0o600)).unwrap();
    let ancestor = temp.path().join(".moosuri-restore-stage-reserved-parent");
    fs::DirBuilder::new().mode(0o700).create(&ancestor).unwrap();
    for (index, dest) in [
        temp.path().join(".moosuri-restore-stage-reserved-final"),
        ancestor.join("data"),
    ]
    .into_iter()
    .enumerate()
    {
        let reports = temp.path().join(format!("reports-{index}"));
        assert!(matches!(
            import_bundle(&input, "legacy-owner", enrollment(), &dest, &reports).await,
            Err(Error::Invalid)
        ));
        assert!(!dest.exists());
        assert!(!reports.exists());
    }
    assert!(fs::read_dir(&ancestor).unwrap().next().is_none());
    let mut names = fs::read_dir(temp.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(
        names,
        vec![
            ".moosuri-restore-stage-reserved-parent".to_string(),
            "private.json".to_string()
        ]
    );
}
