use super::*;
use std::sync::atomic::Ordering;
fn config() -> CapsuleConfig {
    CapsuleConfig {
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
    }
}
fn fixture(script: String) -> CapsuleReader {
    let mut reader = CapsuleReader::new(config()).unwrap();
    reader.fixture = Some(Arc::new(Fixture {
        script,
        cleaned: 0.into(),
    }));
    reader
}
fn ready(account: Uuid) -> String {
    json!({"ready":true,"javascript_enabled":false,"tenant":account}).to_string()
}
fn script(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| format!("printf '%s\\n' '{}';", line.replace('\'', "'\\''")))
        .collect::<String>()
        + "exec /bin/sleep 30"
}
fn page() -> Value {
    json!({"ok":true,"result":{"handle":{"page":Uuid::new_v4()},"url":"https://example.org/final","title":"Rendered","text":"visible text","links":[{"text":"source","url":"https://example.org/source"}],"truncated":false}})
}
#[test]
fn deployment_pin_and_validation_are_strict() {
    let config = config();
    let first = CapsuleReader::new(config.clone()).unwrap();
    let same = CapsuleReader::new(config.clone()).unwrap();
    assert_eq!(first.deployment_pin(), same.deployment_pin());
    let mut changed = config.clone();
    changed.javascript_enabled = true;
    assert_ne!(
        first.deployment_pin(),
        CapsuleReader::new(changed).unwrap().deployment_pin()
    );
    for invalid in 0..6 {
        let mut invalid_config = config.clone();
        match invalid {
            0 => invalid_config.verified = false,
            1 => invalid_config.image_id = "latest".into(),
            2 => invalid_config.network_id = "browser-net".into(),
            3 => invalid_config.proxy_ip = "127.0.0.1".into(),
            4 => invalid_config.owner = Uuid::nil(),
            _ => invalid_config.smoke_evidence_sha256 = "0".repeat(64),
        }
        assert!(CapsuleReader::new(invalid_config).is_err());
    }
    assert!(CapsuleConfig::from_json("{\"command\":\"sh\"}").is_err());
}
#[tokio::test]
async fn projects_bounded_real_protocol_and_reaps_worker() {
    let account = Uuid::new_v4();
    let reader = fixture(script(&[ready(account), page().to_string()]));
    let page = reader
        .read(account, "https://example.org/", &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(page.text, "visible text");
    assert_eq!(page.url, "https://example.org/final");
    assert_eq!(page.links.len(), 1);
    assert!(page.retrieved_at_unix_ms > 0);
    assert!(!serde_json::to_string(&page).unwrap().contains("handle"));
    assert_eq!(
        reader
            .fixture
            .as_ref()
            .unwrap()
            .cleaned
            .load(Ordering::SeqCst),
        1
    );
}
#[tokio::test]
async fn rejects_malformed_oversized_wrong_account_and_private_output() {
    let account = Uuid::new_v4();
    let mut private = page();
    private["result"]["url"] = json!("http://127.0.0.1/");
    let cases = vec![
        vec!["garbage".into()],
        vec!["x".repeat(2049)],
        vec![ready(Uuid::new_v4())],
        vec![ready(account), "x".repeat(MAX_RESPONSE + 1)],
        vec![ready(account), private.to_string()],
    ];
    for lines in cases {
        let reader = fixture(script(&lines));
        assert!(
            reader
                .read(account, "https://example.org/", &CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(
            reader
                .fixture
                .as_ref()
                .unwrap()
                .cleaned
                .load(Ordering::SeqCst),
            1
        );
    }
}
#[tokio::test]
async fn cancellation_and_caller_drop_clean_stalled_stdout_and_release_capacity() {
    let account = Uuid::new_v4();
    let reader = fixture("exec /bin/sleep 30".into());
    let cancel = CancellationToken::new();
    let task = {
        let reader = reader.clone();
        let cancel = cancel.clone();
        tokio::spawn(async move { reader.read(account, "https://example.org/", &cancel).await })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    cancel.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap(),
        Err(BrowserError::Cancelled)
    ));
    assert_eq!(
        reader
            .fixture
            .as_ref()
            .unwrap()
            .cleaned
            .load(Ordering::SeqCst),
        1
    );
    let task = {
        let reader = reader.clone();
        tokio::spawn(async move {
            reader
                .read(account, "https://example.org/", &CancellationToken::new())
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    task.abort();
    let _ = task.await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while reader
            .fixture
            .as_ref()
            .unwrap()
            .cleaned
            .load(Ordering::SeqCst)
            < 2
        {
            tokio::task::yield_now().await
        }
    })
    .await
    .unwrap();
    assert_eq!(reader.capacity.available_permits(), 2);
}
#[tokio::test]
async fn capacity_rejects_immediately() {
    let account = Uuid::new_v4();
    let reader = fixture("exec /bin/sleep 30".into());
    let _first = reader.capacity.clone().acquire_owned().await.unwrap();
    let _second = reader.capacity.clone().acquire_owned().await.unwrap();
    assert!(matches!(
        reader
            .read(account, "https://example.org/", &CancellationToken::new())
            .await,
        Err(BrowserError::Capacity)
    ));
}

#[test]
fn health_requires_all_five_capability_sets_and_restrictions() {
    let status = "NoNewPrivs:\t1\nSeccomp:\t2\nCapEff:\t0000000000000000\nCapPrm:\t0000000000000000\nCapInh:\t0000000000000000\nCapAmb:\t0000000000000000\nCapBnd:\t0000000000000000\n";
    assert!(super::super::capsule_status_confined(status));
    for field in ["CapEff", "CapPrm", "CapInh", "CapAmb", "CapBnd"] {
        let mutated = status.replace(
            &format!("{field}:\t0000000000000000"),
            &format!("{field}:\t0000000000000001"),
        );
        assert!(!super::super::capsule_status_confined(&mutated));
    }
    for (old, new) in [
        ("NoNewPrivs:\t1", "NoNewPrivs:\t0"),
        ("Seccomp:\t2", "Seccomp:\t0"),
    ] {
        assert!(!super::super::capsule_status_confined(
            &status.replace(old, new)
        ));
    }
}

#[test]
fn docker_config_cannot_inject_proxy_environment_or_credentials() {
    let path = std::env::temp_dir().join(format!("moosuri-empty-docker-test-{}", Uuid::new_v4()));
    assert!(empty_docker_config(&path).is_ok());
    std::fs::create_dir(&path).unwrap();
    assert!(empty_docker_config(&path).is_ok());
    std::fs::write(
        path.join("config.json"),
        r#"{"proxies":{"default":{"httpProxy":"do-not-inherit"}}}"#,
    )
    .unwrap();
    assert!(empty_docker_config(&path).is_err());
    std::fs::remove_file(path.join("config.json")).unwrap();
    std::fs::remove_dir(&path).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/tmp", &path).unwrap();
        assert!(empty_docker_config(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn pre_ready_delayed_creation_is_removed_by_id_but_never_observed_stays_uncertain() {
    let name = "moosuri-browser-read-synthetic";
    let read = Uuid::new_v4();
    let owner = Uuid::new_v4();
    let id = "e".repeat(64);
    assert_eq!(
        cleanup_decision(b"", name, read, owner, false, false).unwrap(),
        CleanupDecision::Retry
    );
    let appeared =
        serde_json::to_vec(&json!({"id":id,"name":name,"owner":owner,"read":read})).unwrap();
    assert_eq!(
        cleanup_decision(&appeared, name, read, owner, false, true).unwrap(),
        CleanupDecision::Remove(id.clone())
    );
    assert_eq!(
        cleanup_decision(b"", name, read, owner, false, true).unwrap(),
        CleanupDecision::Unconfirmed
    );
    assert_eq!(
        cleanup_decision(b"", name, read, owner, true, true).unwrap(),
        CleanupDecision::Clean
    );
    assert!(cleanup_decision(&appeared, name, Uuid::new_v4(), owner, false, true).is_err());
    assert!(cleanup_decision(&appeared, name, read, Uuid::new_v4(), false, true).is_err());
    assert!(cleanup_decision(b"not json", name, read, owner, false, true).is_err());
}

#[tokio::test]
async fn automatic_removal_race_requires_observed_absence_and_keeps_unknown_failures() {
    use std::collections::VecDeque;
    for (observations, expected, calls) in [
        (vec![Ok(true)], Ok(()), 1),
        (vec![Ok(false), Ok(true)], Ok(()), 2),
        (
            vec![Ok(false), Ok(false), Ok(false)],
            Err(BrowserError::CleanupUnconfirmed),
            3,
        ),
        (vec![Err(BrowserError::Setup)], Err(BrowserError::Setup), 1),
        (
            vec![Err(BrowserError::IsolationRequired)],
            Err(BrowserError::IsolationRequired),
            1,
        ),
    ] {
        let mut observations = VecDeque::from(observations);
        let mut queries = 0;
        let actual = confirm_removal(Err(BrowserError::Setup), || {
            queries += 1;
            std::future::ready(observations.pop_front().expect("bounded reconciliation"))
        })
        .await;
        assert_eq!(actual, expected);
        assert_eq!(queries, calls);
    }
    confirm_removal(Ok(vec![]), || {
        panic!("successful immutable-ID removal needs no extra Docker query");
        #[allow(unreachable_code)]
        std::future::ready(Ok(false))
    })
    .await
    .unwrap();
}
