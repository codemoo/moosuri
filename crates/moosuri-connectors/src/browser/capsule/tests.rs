use super::*;
use std::sync::atomic::Ordering;
fn config() -> CapsuleConfig {
    CapsuleConfig {
        engine: Default::default(),
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
        recover_workspaces: false,
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
    let mut directory = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directory.mode(0o700);
    }
    directory.create(&path).unwrap();
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
fn kiri_requires_the_verified_engine_with_custom_seccomp_support() {
    let mut server =
        json!({"Platform":{"Name":"Kiri"},"Components":[{"Name":"Kiri","Version":"0.2.2"}]});
    assert!(supported_kiri(&serde_json::to_vec(&server).unwrap()));
    for version in ["0.2.1", "27.5.1", "0.2.3", ""] {
        server["Components"][0]["Version"] = json!(version);
        assert!(!supported_kiri(&serde_json::to_vec(&server).unwrap()));
    }
    server["Components"][0]["Version"] = json!("0.2.2");
    server["Platform"]["Name"] = json!("Docker");
    assert!(!supported_kiri(&serde_json::to_vec(&server).unwrap()));
    assert!(!supported_kiri(b"kiri\n"));
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

#[test]
fn recovery_opt_in_preserves_historical_config_bytes_and_changes_enabled_pin() {
    let old = config();
    let encoded = serde_json::to_value(&old).unwrap();
    assert!(encoded.get("recover_workspaces").is_none());
    let restored = CapsuleConfig::from_json(&encoded.to_string()).unwrap();
    assert!(!restored.recover_workspaces);
    let mut enabled = old.clone();
    enabled.recover_workspaces = true;
    assert_ne!(
        CapsuleReader::new(old).unwrap().deployment_pin(),
        CapsuleReader::new(enabled).unwrap().deployment_pin()
    );
}

#[tokio::test]
async fn slow_daemon_auto_removal_is_confirmed_without_deleting_a_replacement() {
    let began = tokio::time::Instant::now();
    confirm_removal(Err(BrowserError::Setup), || {
        std::future::ready(Ok(began.elapsed() >= Duration::from_millis(600)))
    })
    .await
    .unwrap();
    assert!(began.elapsed() < Duration::from_secs(3));
}

#[test]
fn omitted_engine_preserves_legacy_deployment_pin_and_kiri_changes_it() {
    let config = config();
    let wire = serde_json::to_string(&config).unwrap();
    assert!(!wire.contains("engine"));
    let legacy = CapsuleConfig::from_json(&wire).unwrap();
    let first = CapsuleReader::new(legacy).unwrap();
    let mut kiri = config.clone();
    kiri.engine = Engine::Kiri;
    let next = CapsuleReader::new(kiri).unwrap();
    assert_ne!(first.deployment_pin(), next.deployment_pin());
    let explicit = wire.replacen('{', "{\"engine\":\"docker\",", 1);
    assert_eq!(
        first.deployment_pin(),
        CapsuleReader::new(CapsuleConfig::from_json(&explicit).unwrap())
            .unwrap()
            .deployment_pin()
    );
}

#[test]
fn engine_selector_rejects_arbitrary_remote_or_local_destinations() {
    for engine in [
        "tcp://example.test:2375",
        "unix:///tmp/engine.sock",
        "Kiri",
        "auto",
        "",
    ] {
        let mut wire = serde_json::to_value(config()).unwrap();
        wire["engine"] = json!(engine);
        assert!(CapsuleConfig::from_json(&wire.to_string()).is_err());
    }
}

#[test]
fn kiri_refuses_unaccepted_profile_recovery() {
    let mut cfg = config();
    cfg.recover_workspaces = true;
    assert!(CapsuleReader::new(cfg.clone()).is_ok());
    cfg.engine = Engine::Kiri;
    assert!(matches!(
        CapsuleReader::new(cfg),
        Err(BrowserError::IsolationRequired)
    ));
}

#[test]
fn selected_engine_pins_control_and_worker_commands_without_ambient_configuration() {
    for (engine, socket) in [
        (Engine::Docker, "unix:///var/run/docker.sock"),
        (Engine::Kiri, "unix:///run/kiri/docker.sock"),
    ] {
        let mut cfg = config();
        cfg.engine = engine;
        let reader = CapsuleReader::new(cfg).unwrap();
        let control = reader.docker();
        let worker = reader.worker_command(Uuid::new_v4(), Uuid::new_v4(), "test-worker", None);
        for command in [control, worker] {
            let command = command.as_std();
            let args: Vec<_> = command.get_args().map(|s| s.to_str().unwrap()).collect();
            assert_eq!(args[0], format!("--host={socket}"));
            assert_eq!(args[1], "--config=/run/moosuri-browser-empty-docker-config");
            let env: Vec<_> = command
                .get_envs()
                .map(|(k, v)| (k.to_str().unwrap(), v.map(|s| s.to_str().unwrap())))
                .collect();
            assert!(
                !env.iter()
                    .any(|(k, _)| matches!(*k, "DOCKER_HOST" | "DOCKER_CONTEXT" | "DOCKER_CONFIG"))
            );
            assert_eq!(
                env.iter()
                    .find(|(k, _)| *k == "DOCKER_API_VERSION")
                    .map(|(_, v)| *v),
                if engine == Engine::Kiri {
                    Some(Some("1.47"))
                } else {
                    None
                }
            );
        }
    }
}

#[tokio::test]
async fn bootstrap_cleanup_requires_graceful_zero_exit_and_still_reaps_failures() {
    let reader = fixture(String::new());
    for (script, clean) in [
        ("exit 0", true),
        ("exit 1", false),
        ("exec sleep 30", false),
    ] {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let result = reader
            .cleanup_grace(
                &mut child,
                "fixture",
                Uuid::new_v4(),
                true,
                false,
                Some(Duration::from_millis(100)),
            )
            .await;
        if clean {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(BrowserError::CleanupUnconfirmed));
        }
        assert!(child.try_wait().unwrap().is_some());
    }
}

#[test]
fn authentication_memory_is_finite_and_swap_stays_disabled() {
    let reader = CapsuleReader::new(config()).unwrap();
    for auth in [false, true] {
        let command = reader.worker_command_mode(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "bounded-worker",
            None,
            auth,
        );
        let args: Vec<_> = command
            .as_std()
            .get_args()
            .map(|x| x.to_string_lossy().into_owned())
            .collect();
        let memory = if auth {
            "--memory=1610612736"
        } else {
            "--memory=805306368"
        };
        assert_eq!(
            args.iter()
                .filter(|x| x.starts_with("--memory="))
                .collect::<Vec<_>>(),
            vec![memory]
        );
        assert_eq!(
            args.iter()
                .filter(|x| x.starts_with("--memory-swap="))
                .collect::<Vec<_>>(),
            if auth {
                vec!["--memory-swap=1610612736"]
            } else {
                vec![]
            }
        );
        for flag in [
            "--read-only",
            "--pids-limit=256",
            "--cpus=1",
            "--security-opt=no-new-privileges:true",
        ] {
            assert!(args.iter().any(|x| x == flag));
        }
        assert!(!args.iter().any(|x| x.starts_with("--mount=")));
    }
}
