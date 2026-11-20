use super::*;
use serde_json::{Value, json};

fn other_uid(excluded: &[u32]) -> u32 {
    let uid = unsafe { libc::geteuid() };
    (10001..)
        .find(|other| *other != uid && !excluded.contains(other))
        .unwrap()
}

fn login_config(options: &Options, route: Route) -> Config {
    let mut config: Config = serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
    config.key_file = None;
    config.subscription = Some(route);
    if route == Route::ClaudeCode {
        config.model = "claude-fixture-v1".into();
    }
    config.provider = ProviderConfig {
        kind: match route {
            Route::ChatgptPlan => ProviderKind::OpenAi,
            Route::ClaudeCode => ProviderKind::Anthropic,
        },
        base_url: None,
        allowed_ips: vec![],
        router_providers: vec![],
        zero_data_retention: false,
    };
    config.bindings[0].worker = Some(Endpoint {
        socket: options.config.parent().unwrap().join("engine.sock"),
        worker_uid: other_uid(&[]),
    });
    config
}

#[test]
fn validates_subscription_route_and_every_worker_before_opening_the_ledger() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (options, bindings) = fixture(&root, "http://127.0.0.1:65531/v1".into());
    let api: Value = serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
    assert!(api.get("subscription").is_none());
    assert!(api["bindings"][0].get("worker").is_none());
    fs::remove_file(&options.fingerprint_key).unwrap();
    let mut second = bindings[1].clone();
    let first_worker_uid = other_uid(&[]);
    second.peer_uid = other_uid(&[first_worker_uid]);
    second.worker = Some(Endpoint {
        socket: root.join("second").join("engine.sock"),
        worker_uid: other_uid(&[first_worker_uid, second.peer_uid]),
    });
    for route in [Route::ChatgptPlan, Route::ClaudeCode] {
        let mut config = login_config(&options, route);
        config.bindings.push(second.clone());
        let base = serde_json::to_value(config).unwrap();
        let reject = |config: &Value, expected| {
            file(&options.config, &serde_json::to_vec(config).unwrap());
            assert_eq!(
                Broker::new(&options, CancellationToken::new())
                    .err()
                    .as_deref(),
                Some(expected)
            );
            assert_eq!(fs::read_dir(&options.ledger).unwrap().count(), 0);
        };
        // Missing fingerprint file proves the valid configuration has passed
        // validation even while the worker socket is absent.
        reject(&base, "private_file_unavailable");
        for (pointer, invalid, expected) in [
            (
                "/connection_revision",
                json!(Uuid::nil()),
                "model_config_invalid",
            ),
            ("/key_file", json!("/never-read"), "model_config_invalid"),
            (
                "/provider/kind",
                json!("open_router"),
                "model_config_invalid",
            ),
            (
                "/provider/base_url",
                json!("https://example.invalid"),
                "model_config_invalid",
            ),
            (
                "/provider/allowed_ips",
                json!(["127.0.0.1"]),
                "model_config_invalid",
            ),
            (
                "/provider/router_providers",
                json!(["unused"]),
                "model_config_invalid",
            ),
            (
                "/provider/zero_data_retention",
                json!(true),
                "model_config_invalid",
            ),
            ("/bindings/0/worker", Value::Null, "model_binding_invalid"),
            (
                "/bindings/0/worker/worker_uid",
                json!(0),
                "model_binding_invalid",
            ),
            (
                "/bindings/0/worker/socket",
                json!("engine.sock"),
                "model_binding_invalid",
            ),
            (
                "/bindings/0/worker/worker_uid",
                json!(second.peer_uid),
                "model_binding_invalid",
            ),
            (
                "/bindings/1/worker/worker_uid",
                json!(first_worker_uid),
                "model_binding_invalid",
            ),
            (
                "/bindings/1/worker/socket",
                base["bindings"][0]["worker"]["socket"].clone(),
                "model_binding_invalid",
            ),
        ] {
            let mut config = base.clone();
            *config.pointer_mut(pointer).unwrap() = invalid;
            reject(&config, expected);
        }
        for field in ["account_id", "tenant_id", "volume_id"] {
            let mut missing_owner = base.clone();
            missing_owner["bindings"][0][field] = json!(Uuid::nil());
            reject(&missing_owner, "model_binding_invalid");
        }
        let mut api_with_worker = api.clone();
        api_with_worker["bindings"][0]["worker"] = base["bindings"][0]["worker"].clone();
        reject(&api_with_worker, "model_binding_invalid");

        file(&options.config, &serde_json::to_vec(&api).unwrap());
    }
}

#[tokio::test]
async fn unavailable_worker_preserves_budget_and_replays_only_its_own_receipt() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (options, _) = fixture(&root, "http://127.0.0.1:65531/v1".into());
    let mut config = login_config(&options, Route::ChatgptPlan);
    config.bindings[0].budget.max_calls = 1;
    file(&options.config, &serde_json::to_vec(&config).unwrap());
    let broker = ready_broker(&options);
    let b = &config.bindings[0];
    let r = request(b);
    let bytes = serde_json::to_vec(&r).unwrap();
    for _ in 0..2 {
        assert_eq!(
            broker
                .call(b, serde_json::from_slice(&bytes).unwrap())
                .await
                .state,
            "model_worker_unavailable"
        );
    }

    // Synthesize a completed admission to test replay without requiring
    // another OS user, installed CLI or provider session.
    let worker = b.worker.as_ref().unwrap();
    let key = broker
        .call_key(b, &r, Some((Route::ChatgptPlan, worker)))
        .unwrap();
    assert_ne!(
        key.fingerprint,
        broker
            .call_key(b, &r, Some((Route::ClaudeCode, worker)))
            .unwrap()
            .fingerprint
    );
    let start_epoch = epoch(&options.registry, b).unwrap();
    let Some(Admission::Fresh(permit)) = broker.admit(b, &r, &key, true, start_epoch).unwrap()
    else {
        panic!("failed worker connection must not consume a call");
    };
    broker
        .ledger
        .lock()
        .unwrap()
        .settle(&permit, Some(7), Some(3))
        .unwrap();
    drop(broker);

    let broker = ready_broker(&options);
    let reply = broker
        .call(b, serde_json::from_slice(&bytes).unwrap())
        .await;
    assert_eq!(reply.state, "settled_response_unavailable");
    assert!(reply.output.is_none());
    assert_eq!(reply.receipt.unwrap()["charged_tokens"], 10);
    drop(broker);

    config.bindings[0].worker.as_mut().unwrap().socket = root.join("other").join("engine.sock");
    file(&options.config, &serde_json::to_vec(&config).unwrap());
    let broker = ready_broker(&options);
    let reply = broker
        .call(&config.bindings[0], serde_json::from_slice(&bytes).unwrap())
        .await;
    assert_eq!(reply.state, "call_conflict");
}

#[tokio::test]
async fn login_route_validates_fresh_request_shape_and_preserves_existing_receipts() {
    for (route, invalid_role) in [
        (Route::ChatgptPlan, Role::System),
        (Route::ClaudeCode, Role::Assistant),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let (options, _) = fixture(&root, "http://127.0.0.1:65531/v1".into());
        let config = login_config(&options, route);
        file(&options.config, &serde_json::to_vec(&config).unwrap());
        let broker = ready_broker(&options);
        let binding = &config.bindings[0];
        let mut invalid = request(binding);
        invalid.model = config.model.clone();
        invalid.messages[0].role = invalid_role;
        let bytes = serde_json::to_vec(&invalid).unwrap();
        let reply = broker
            .call(binding, serde_json::from_slice(&bytes).unwrap())
            .await;
        assert_eq!(reply.state, "request_invalid");

        let key = broker
            .call_key(
                binding,
                &invalid,
                Some((route, binding.worker.as_ref().unwrap())),
            )
            .unwrap();
        let start_epoch = epoch(&options.registry, binding).unwrap();
        let Some(Admission::Fresh(permit)) = broker
            .admit(binding, &invalid, &key, true, start_epoch)
            .unwrap()
        else {
            panic!("invalid request must leave the ledger unchanged");
        };
        broker
            .ledger
            .lock()
            .unwrap()
            .uncertain(&permit, None, None)
            .unwrap();
        let replay = broker.call(binding, invalid).await;
        assert_eq!(replay.state, "outcome_unknown");
        assert!(replay.output.is_none());
        assert_eq!(
            replay.receipt.unwrap()["charged_tokens"],
            binding.budget.reservation_tokens
        );
    }
}

async fn worker_receive(stream: &mut UnixStream) -> Option<Value> {
    let len = match stream.read_u32().await {
        Ok(len) => len as usize,
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return None,
        Err(error) => panic!("worker read failed: {error}"),
    };
    assert!(len <= MAX_FRAME);
    let mut buffer = vec![0; len];
    stream.read_exact(&mut buffer).await.unwrap();
    Some(serde_json::from_slice(&buffer).unwrap())
}

async fn worker_send(stream: &mut UnixStream, value: Value) {
    let bytes = serde_json::to_vec(&value).unwrap();
    stream.write_u32(bytes.len() as u32).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
}

#[tokio::test]
async fn worker_acknowledgement_precedes_one_durable_dispatch_and_epoch_recheck() {
    let uid = unsafe { libc::geteuid() };
    if uid == 0 {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let (options, members) = fixture(&root, "http://127.0.0.1:65531/v1".into());
    let mut config = login_config(&options, Route::ChatgptPlan);
    config.effort = Some("high".into());
    config.models.push(ModelChoice {
        model: config.model.clone(),
        effort: None,
    });
    file(&options.config, &serde_json::to_vec(&config).unwrap());
    let broker = ready_broker(&options);
    // A native test has one non-root OS identity. Use the real wire and peer
    // checks with a test binding after startup validates production UID rules.
    let mut binding = config.bindings[0].clone();
    let endpoint = binding.worker.as_mut().unwrap();
    let listener = UnixListener::bind(&endpoint.socket).unwrap();
    fs::set_permissions(&endpoint.socket, fs::Permissions::from_mode(0o600)).unwrap();
    endpoint.worker_uid = uid;

    let mut first = request(&binding);
    first.effort = config.effort.clone();
    let duplicate = serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    let hello = json!({
        "version": 1, "route": "chatgpt_plan", "owner": binding.owner(),
        "revision": first.connection_revision, "model": first.model, "effort": "high",
    });
    let worker = async {
        let (mut a, _) = listener.accept().await.unwrap();
        let (mut b, _) = listener.accept().await.unwrap();
        for stream in [&mut a, &mut b] {
            assert_eq!(worker_receive(stream).await, Some(hello.clone()));
            worker_send(stream, json!(true)).await;
        }
        let (a_prompt, b_prompt) = tokio::join!(worker_receive(&mut a), worker_receive(&mut b));
        let (stream, prompt) = match (a_prompt, b_prompt) {
            (Some(prompt), None) => (&mut a, prompt),
            (None, Some(prompt)) => (&mut b, prompt),
            _ => panic!("exactly one worker connection must receive prompt text"),
        };
        assert_eq!(prompt["messages"][0]["text"], "SYNTHETIC-PROMPT-CANARY");
        for event in [
            json!({"event": "text", "text": "SYNTHETIC-WORKER-ANSWER"}),
            json!({"event": "usage", "input": 7, "output": 3}),
            json!({"event": "finished", "outcome": "completed"}),
        ] {
            worker_send(stream, event).await;
        }
    };
    let (a, b, _) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(
            broker.call(&binding, first),
            broker.call(&binding, duplicate),
            worker
        )
    })
    .await
    .unwrap();
    let (settled, replay) = if a.state == "settled" { (a, b) } else { (b, a) };
    assert_eq!(settled.output.as_deref(), Some("SYNTHETIC-WORKER-ANSWER"));
    assert_eq!(settled.effort.as_deref(), Some("high"));
    assert_eq!(settled.receipt.unwrap()["charged_tokens"], 10);
    assert_eq!(replay.state, "in_progress");

    let actor = operator(&options, &members[1], "b@example.invalid");
    let mut changed = request(&binding);
    changed.effort = config.effort.clone();
    let worker = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(worker_receive(&mut stream).await, Some(hello.clone()));
        engine::change(
            &options.registry,
            &actor,
            engine::InstallationChange {
                revision: settings_revision(&options, &actor),
                connection_revision: config.connection_revision,
                enabled: true,
                default_choice: Some(config.models[0].clone()),
            },
        )
        .unwrap();
        worker_send(&mut stream, json!(true)).await;
        assert!(worker_receive(&mut stream).await.is_none());
    };
    let (reply, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(broker.call(&binding, changed), worker)
    })
    .await
    .unwrap();
    assert_eq!(reply.state, "model_selection_changed");
    assert!(reply.receipt.is_none());

    let fenced = request(&binding);
    let mut hello = hello;
    hello.as_object_mut().unwrap().remove("effort");
    let worker = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        assert_eq!(worker_receive(&mut stream).await, Some(hello));
        let allocation =
            runtime_registry::lookup_id(&options.registry, binding.account_id).unwrap();
        runtime_registry::set_routing(&options.registry, &allocation, false).unwrap();
        worker_send(&mut stream, json!(true)).await;
        assert!(worker_receive(&mut stream).await.is_none());
    };
    let (reply, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(broker.call(&binding, fenced), worker)
    })
    .await
    .unwrap();
    assert_eq!(reply.state, "account_fenced");
    assert!(reply.receipt.is_none());
}

#[tokio::test]
async fn openrouter_quota_reads_only_on_admin_request_and_fences_key_and_connection_changes() {
    use engine::quota::{self, Request as QuotaRead};
    use moosuri_core::{QuotaMetric, QuotaStatus, QuotaUnit};
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let (options, bindings) = fixture(&root, "http://127.0.0.1:9/v1".into());
    let mut config: Config = serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
    config.provider = ProviderConfig {
        kind: ProviderKind::OpenRouter,
        base_url: None,
        allowed_ips: vec![],
        router_providers: vec!["example".into()],
        zero_data_retention: false,
    };
    file(&options.config, &serde_json::to_vec(&config).unwrap());
    let broker = ready_broker(&options);
    let actor = operator(&options, &bindings[0], "a@example.invalid");
    let rev = settings_revision(&options, &actor);
    let connection = config.connection_revision;
    let request = |probe_id| {
        quota::request(
            &options.registry,
            &actor,
            QuotaRead {
                connection_revision: connection,
                revision: rev,
                probe_id,
            },
        )
    };
    let known = || {
        vec![QuotaMetric {
            id: "key_credit_limit".into(),
            label: "API key credit cap (all models)".into(),
            unit: QuotaUnit::Usd,
            used: None,
            limit: Some(1.),
            remaining: Some(0.),
            reset_interval: Some("daily".into()),
            reset_at: None,
        }]
    };
    broker
        .process_quota(async |_| panic!("no approved read"))
        .await;
    let first = request(None).unwrap();
    assert_eq!(request(None).unwrap().probe_id, first.probe_id);
    broker.process_quota(async |_| Some(known())).await;
    // A second tab starts a fresh read before the first tab receives its result.
    let second = request(None).unwrap();
    assert_ne!(first.probe_id, second.probe_id);
    assert!(request(Some(second.probe_id)).unwrap().report.is_none());
    let report = request(Some(first.probe_id)).unwrap().report.unwrap();
    assert_eq!(report.status, QuotaStatus::Known);
    assert!(report.observed_at.is_some());
    assert_eq!(report.metrics[0].remaining, Some(0.));
    assert_eq!(report.metrics[0].reset_at, None);
    assert_eq!(report.metrics[0].reset_interval.as_deref(), Some("daily"));

    broker
        .process_quota(async |_| {
            assert_eq!(request(None).unwrap().probe_id, second.probe_id);
            file(config.key_file.as_ref().unwrap(), b"rotated-synthetic-key");
            Some(known())
        })
        .await;
    let failed = request(Some(second.probe_id)).unwrap().report.unwrap();
    assert_eq!(failed.status, QuotaStatus::Unavailable);
    assert!(failed.metrics.is_empty() && failed.observed_at.is_none());

    let third = request(None).unwrap();
    broker
        .process_quota(async |_| {
            engine::publish_models(
                &options.registry,
                Uuid::new_v4(),
                ProviderKind::OpenRouter,
                engine::ConnectionMode::Api,
                &broker.models[0],
                &broker.models,
                &[bindings[0].account_id],
            )
            .unwrap();
            Some(known())
        })
        .await;
    assert!(matches!(
        request(Some(third.probe_id)),
        Err(StoreError::Conflict)
    ));
}
