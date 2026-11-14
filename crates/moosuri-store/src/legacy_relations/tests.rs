use super::*;
use crate::{Store, tests::TEST_LOCK};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
};
const ITEM: &str = "10000000-0000-4000-a000-000000000001";
const DOC: &str = "10000000-0000-4000-a000-000000000002";
fn fixture() -> BTreeMap<String, Vec<Value>> {
    let names: Vec<String> = serde_json::from_str(include_str!(
        "../../../../tools/legacy/schema23-table-names.json"
    ))
    .unwrap();
    let mut t = names
        .into_iter()
        .map(|n| (n, vec![]))
        .collect::<BTreeMap<_, _>>();
    t.insert("ma_schema_migrations".into(),serde_json::from_str::<Vec<Value>>(include_str!("../../../../tools/legacy/schema23-checksums.json")).unwrap().into_iter().map(|v|json!({"name":v["name"],"checksum":v["sha256"],"applied_at":"2026-01-01T00:00:00Z"})).collect());
    t.insert("ma_meta".into(),vec![json!({"id":1,"memory_epoch":7,"journal_sequence":8,"journal_digest":"synthetic-watermark","items_revision":9})]);
    t.insert("ma_source_bindings".into(),vec![json!({"id":"binding","dataset_id":"dataset","scope_digest":"scope","generation":"generation","revision":2,"enabled":true,"purged":false,"source_kind":"custom","coverage":"Synthetic source scope"})]);
    t.insert("ma_source_documents".into(),vec![json!({"evidence_id":"legacy-source-evidence","binding_id":"binding","key_hash":"source-key-hash","version":"source-version","title":"Synthetic source","body":"Verified quote text","search_text":"irrelevant","source_at":"2026-01-01T00:00:00Z","observed_at":"2026-01-02T00:00:00Z"})]);
    let claim = json!({"item_id":ITEM,"identity":"new-identity","subject":"owner","predicate":"note","value":"Corrected current value","speaker":"owner","valid_from":null,"valid_until":null,"observed_at":"2026-01-02T00:00:00Z","origin":"source","state":"active","topic":"Synthetic topic"});
    t.insert("ma_memory_claims".into(), vec![claim.clone()]);
    t.insert("ma_memory_identities".into(),vec![json!({"identity":"new-identity","canonical_id":ITEM,"item_id":ITEM,"suppressed":false})]);
    t.insert("ma_memory_supports".into(),vec![json!({"item_id":ITEM,"evidence_id":"legacy-source-evidence","binding_id":"binding","key_hash":"source-key-hash","version":"source-version","quote":"Verified quote","speaker":"owner","observed_at":"2026-01-02T00:00:00Z","source_at":null,"active":true,"unit_id":"unit-1"})]);
    t.insert("ma_items".into(),vec![json!({"kind":"memories","id":ITEM,"revision":7,"deleted":false,"data":{"value":claim["value"],"subject":claim["subject"],"predicate":claim["predicate"],"speaker":claim["speaker"],"origin":claim["origin"],"valid_from":null,"valid_until":null}}),json!({"kind":"artifacts","id":DOC,"revision":4,"deleted":false,"data":{"title":"Current document","subtitle":"Preserved subtitle metadata","type":"note","body":"Actual current report","visibility_epoch":7,"evidence":[{"kind":"memory","id":ITEM,"revision":7}]}})]);
    t.insert("ma_jobs".into(),vec![json!({"run_id":"old-run","status":"running","credential":"synthetic-unsupported-secret"})]);
    t
}
fn snapshot(t: BTreeMap<String, Vec<Value>>) -> Vec<u8> {
    let mut lines = vec![
        json!({"type":"header","format":1,"purpose":"musuri-schema23-relations-v1","source_schema":23,"owner":"legacy-owner","deployment_id":"synthetic-old-source","source_version":"source-code-contract23","snapshot":"synthetic-tx-snapshot","exported_at":"2026-01-03T00:00:00Z","inventory":t.keys().map(|n|json!({"schema":"public","name":n})).collect::<Vec<_>>()}),
    ];
    let count = t.len();
    for (name, rows) in t {
        lines.push(
            json!({"type":"relation","schema":"public","name":name,"count":rows.len(),"rows":rows}),
        );
    }
    lines.push(json!({"type":"complete","snapshot":"synthetic-tx-snapshot","relations":count}));
    let mut raw = vec![];
    for l in lines {
        raw.extend(serde_json::to_vec(&l).unwrap());
        raw.push(b'\n');
    }
    raw
}
fn enrollment() -> Enrollment {
    Enrollment {
        name: "Fresh snapshot owner".into(),
        email: "adapter@example.test".into(),
        password: Zeroizing::new("Synthetic-adapter-password".into()),
    }
}
#[tokio::test]
async fn actual_relations_project_current_revisions_evidence_docs_and_account_all_rows() {
    let _guard = TEST_LOCK.lock().await;
    let temp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let input = temp.path().join("snapshot.jsonl");
    let raw = snapshot(fixture());
    fs::write(&input, &raw).unwrap();
    fs::set_permissions(&input, fs::Permissions::from_mode(0o600)).unwrap();
    let data = temp.path().join("data");
    let reports = temp.path().join("reports");
    let result = import_snapshot(&input, "legacy-owner", enrollment(), &data, &reports)
        .await
        .unwrap();
    assert_eq!(result.adapter.converted_objects, 3);
    assert_eq!(result.adapter.total_rows, result.adapter.rows.len());
    assert_eq!(fs::read(&input).unwrap(), raw);
    assert!(
        result
            .adapter
            .rows
            .iter()
            .any(|r| r.relation == "public.ma_jobs" && r.decision == "quarantined")
    );
    assert!(
        !String::from_utf8(bytes(&result.adapter).unwrap())
            .unwrap()
            .contains("synthetic-unsupported-secret")
    );
    let store = Store::open(&data).unwrap();
    let user = store
        .login(
            "adapter@example.test".into(),
            "Synthetic-adapter-password".into(),
        )
        .await
        .unwrap()
        .user;
    let memory = store.memories(&user, None).await.unwrap().items.remove(0);
    assert_eq!(memory.revision, 1);
    assert_eq!(memory.draft.text, "Corrected current value");
    assert_eq!(memory.draft.evidence.len(), 1);
    assert!(
        store
            .memory(&user, memory.id.clone(), Some(7))
            .await
            .is_err()
    );
    let sources = store.collections(&user).await.unwrap();
    assert!(!sources[0].connected);
    let source = store
        .source(&user, memory.draft.evidence[0].source_id.clone(), None)
        .await
        .unwrap();
    assert_eq!(source.source.version, "source-version");
    assert!(source.provenance.is_none());
    drop(store);
    let kit = temp.path().join("kit");
    let exports = temp.path().join("exports");
    for d in [&kit, &exports] {
        fs::DirBuilder::new().mode(0o700).create(d).unwrap();
    }
    let key = kit.join("key");
    let recipient = backup::keygen(&key).unwrap();
    backup::setup(&data, &recipient, &key).unwrap();
    backup::create(&data, &exports.join("snapshot.age")).unwrap();
    assert!(reports.join("legacy-adapter-report.json").is_file());
}
#[test]
fn suppressed_alias_scope_withholds_source_and_never_invents_old_history() {
    let mut t = fixture();
    t.get_mut("ma_memory_identities").unwrap().push(json!({"identity":"old-corrected-identity","canonical_id":DOC,"item_id":ITEM,"suppressed":true}));
    t.get_mut("ma_memory_claims").unwrap()[0]["origin"] = json!("owner");
    t.get_mut("ma_items").unwrap()[0]["data"]["origin"] = json!("owner");
    t.get_mut("ma_memory_supports").unwrap()[0]["active"] = json!(false);
    let (transfer, report) = convert(parse(&snapshot(t), "legacy-owner").unwrap()).unwrap();
    assert!(
        report
            .source_admission
            .starts_with("scoped_sources_withheld")
    );
    assert_eq!(
        transfer["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["operation"]["kind"] == "source_upsert")
            .count(),
        0
    );
    assert!(
        transfer["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["operation"]["text"] == "Corrected current value")
    );
    let mut t = fixture();
    t.get_mut("ma_items").unwrap()[0]["deleted"] = json!(true);
    t.get_mut("ma_memory_claims").unwrap()[0]["state"] = json!("forgotten");
    t.insert("ma_memory_supports".into(), vec![]);
    let (transfer, r) = convert(parse(&snapshot(t), "legacy-owner").unwrap()).unwrap();
    assert_eq!(
        r.source_admission,
        "all_sources_withheld_unscoped_legacy_suppression"
    );
    assert!(
        !transfer["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["operation"]["kind"] == "source_upsert"
                || e["operation"]["kind"] == "memory_upsert")
    );
    let mut t = fixture();
    t.get_mut("ma_items").unwrap()[0]["deleted"] = json!(true);
    t.get_mut("ma_memory_supports").unwrap()[0]["binding_id"] = json!("");
    let (_, report) = convert(parse(&snapshot(t), "legacy-owner").unwrap()).unwrap();
    assert_eq!(
        report.source_admission,
        "all_sources_withheld_unscoped_legacy_suppression"
    );
}
#[test]
fn schema_count_missing_trailer_stale_support_and_whole_input_limits() {
    let raw = snapshot(fixture());
    assert!(parse(&raw[..raw.len() - 100], "legacy-owner").is_err());
    let mut t = fixture();
    t.remove("ma_jobs");
    assert!(parse(&snapshot(t), "legacy-owner").is_err());
    let mut t = fixture();
    t.get_mut("ma_schema_migrations").unwrap()[0]["checksum"] = json!("wrong");
    assert!(parse(&snapshot(t), "legacy-owner").is_err());
    let mut t = fixture();
    t.get_mut("ma_memory_supports").unwrap()[0]["version"] = json!("stale");
    let (_, r) = convert(parse(&snapshot(t), "legacy-owner").unwrap()).unwrap();
    assert!(
        r.rows.iter().any(|r| r.relation == "public.ma_items"
            && r.index == 0
            && r.decision == "quarantined")
    );
    let mut t = fixture();
    t.insert(
        "unsupported_many".into(),
        (0..4097).map(|i| json!({"id":i})).collect(),
    );
    assert!(matches!(
        parse(&snapshot(t), "legacy-owner"),
        Err(Error::Limit)
    ));
    let raw = snapshot(fixture());
    let invalid = String::from_utf8(raw)
        .unwrap()
        .replacen("\"count\":2", "\"count\":3", 1);
    assert!(parse(invalid.as_bytes(), "legacy-owner").is_err());
    let mut t = fixture();
    t.insert(
        "ma_source_documents".into(),
        (0..129)
            .map(|i| {
                json!({
                    "evidence_id":format!("evidence-{i}"),"binding_id":"binding",
                    "key_hash":format!("key-{i}"),"version":"v1","title":"Bound proof",
                    "body":"Synthetic body","observed_at":"2026-01-02T00:00:00Z"
                })
            })
            .collect(),
    );
    assert!(matches!(
        convert(parse(&snapshot(t), "legacy-owner").unwrap()),
        Err(Error::Limit)
    ));
}
