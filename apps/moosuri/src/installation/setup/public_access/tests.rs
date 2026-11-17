use super::*;
#[test]
fn public_mode_rejects_private_origins_and_unsafe_dns_targets() {
    assert_eq!(
        host("assistant.example.com").unwrap(),
        "assistant.example.com"
    );
    for value in [
        "localhost",
        "https://user@a.example.com",
        "https://a.example.com/path",
        "https://a.example.com:8443",
        "https://127.0.0.1",
        "a.ts.net",
        "a.local",
        "*.example.com",
        "a..example.com",
    ] {
        assert!(host(value).is_err(), "{value}");
    }
}

#[tokio::test]
async fn existing_private_https_without_public_receipt_still_opens() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let i = Installation {
        format: 1,
        id: uuid::Uuid::new_v4().to_string(),
        data_dir: root.path().join("data"),
        origin: "https://assistant.local".into(),
        network: Network::Https,
        trust_key: "ab".repeat(32),
        managed: BTreeMap::new(),
    };
    let s = State {
        format: 1,
        language: Language::En,
        port: 3210,
        service: service::Kind::None,
        launcher: None,
        service_file: None,
        next_page: "/app".into(),
        removing: false,
        autostart: true,
        desktop_files: Vec::new(),
        tailscale_requested: false,
        public_requested: None,
    };
    assert!(browser_ready(root.path(), &i, &s).await.unwrap());
}
