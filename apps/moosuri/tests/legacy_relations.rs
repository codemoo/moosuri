use moosuri_store::Store;
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, process::Command};
const BIN: &str = env!("CARGO_BIN_EXE_moosuri");

#[tokio::test]
async fn actual_relation_cli_accounts_unknown_rows_and_refuses_overwrite() {
    let temp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let names: Vec<String> = serde_json::from_str(include_str!(
        "../../../tools/legacy/schema23-table-names.json"
    ))
    .unwrap();
    let mut tables: BTreeMap<String, Vec<Value>> = names.into_iter().map(|n| (n, vec![])).collect();
    let checksums: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/legacy/schema23-checksums.json"
    ))
    .unwrap();
    tables.insert(
        "ma_schema_migrations".into(),
        checksums
            .into_iter()
            .map(|r| json!({"name":r["name"],"checksum":r["sha256"]}))
            .collect(),
    );
    tables.insert("ma_meta".into(), vec![json!({"id":1,"memory_epoch":0})]);
    tables.insert(
        "unknown_private_table".into(),
        vec![json!({"payload":"unsupported-payload-canary"})],
    );
    let mut lines = vec![
        json!({"type":"header","format":1,"purpose":"musuri-schema23-relations-v1","source_schema":23,"owner":"chosen","deployment_id":"synthetic-cli","source_version":"contract23","snapshot":"synthetic-tx","exported_at":"2026-01-01T00:00:00Z","inventory":tables.keys().map(|n|json!({"schema":"public","name":n})).collect::<Vec<_>>()}),
    ];
    let count = tables.len();
    for (name, rows) in tables {
        lines.push(
            json!({"type":"relation","schema":"public","name":name,"count":rows.len(),"rows":rows}),
        );
    }
    lines.push(json!({"type":"complete","snapshot":"synthetic-tx","relations":count}));
    let raw = lines
        .iter()
        .map(|r| serde_json::to_string(r).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let input = temp.path().join("relations.jsonl");
    let password = temp.path().join("password");
    let data = temp.path().join("data");
    let report = temp.path().join("reports");
    fs::write(&input, &raw).unwrap();
    fs::write(&password, "Synthetic-relation-cli-password\n").unwrap();
    for p in [&input, &password] {
        fs::set_permissions(p, fs::Permissions::from_mode(0o600)).unwrap();
    }
    let args = [
        "--data-dir",
        data.to_str().unwrap(),
        "import-legacy-relations",
        "--snapshot",
        input.to_str().unwrap(),
        "--legacy-owner",
        "chosen",
        "--name",
        "Fresh owner",
        "--email",
        "relations@example.test",
        "--password-file",
        password.to_str().unwrap(),
        "--report-dir",
        report.to_str().unwrap(),
    ];
    let output = Command::new(BIN).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("unsupported-payload-canary"));
    let before = fs::read(report.join("legacy-adapter-report.json")).unwrap();
    assert!(!String::from_utf8_lossy(&before).contains("unsupported-payload-canary"));
    assert!(String::from_utf8_lossy(&before).contains("unknown_private_table"));
    assert_eq!(fs::read_to_string(&input).unwrap(), raw);
    let store = Store::open(&data).unwrap();
    let user = store
        .login(
            "relations@example.test".into(),
            "Synthetic-relation-cli-password".into(),
        )
        .await
        .unwrap()
        .user;
    assert!(store.memories(&user, None).await.unwrap().items.is_empty());
    drop(store);
    let retry = Command::new(BIN).args(args).output().unwrap();
    assert!(!retry.status.success());
    assert_eq!(
        before,
        fs::read(report.join("legacy-adapter-report.json")).unwrap()
    );
}
