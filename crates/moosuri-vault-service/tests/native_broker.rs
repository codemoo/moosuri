//! Real assistant/vault HTTP + native core/companion + projected GitHub fixture.
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use moosuri_credential_broker::{
    Engine, Error, auth::SigningKey, github::GitHubAdapter, live_vault::LiveVault,
    transport::Transport,
};
use moosuri_store::Store;
use moosuri_vault_core::{
    ClientVault, ItemFields, OsEntropy, PendingVault, RecoveryKey, SecretText, StorageReceipt,
    inventory_digest,
};
use moosuri_vault_protocol::broker::{
    AuthChallenge, Command, DEVICE_PATH, Device, GitHubRepository, PairProof, Reply, SignedCommand,
    pairing_bytes, request_bytes,
};
use moosuri_vault_protocol::{Operation, Owner, Receipt, VaultState};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tokio::sync::Notify;
use uuid::Uuid;
const LOGICAL: &str = "https://assistant.example.test";
const PASSWORD: &str = "Synthetic-native-account-password";
const MASTER: &str = "Synthetic-native-master-password";
const NEXT_MASTER: &str = "Synthetic-native-master-password-two";
const TOKEN_A: &str = "SYNTHETIC-LIVE-VAULT-TOKEN-A";
const TOKEN_B: &str = "SYNTHETIC-LIVE-VAULT-TOKEN-B";
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
fn secret(s: &str) -> SecretText {
    SecretText::new(s.to_owned()).unwrap()
}
fn private(parent: &Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    fs::create_dir(&p).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    p
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn fields(token: &str) -> ItemFields {
    serde_json::from_value(json!({"kind":"api_key","title":"Synthetic GitHub fixture","username":"","url":"https://api.github.com","secret":token,"notes":"","favorite":false,"expires_at":"","totp":""})).unwrap()
}
#[derive(Default)]
struct Gate {
    unavailable: AtomicBool,
    reads: AtomicUsize,
    hold_at: AtomicUsize,
    entered: Notify,
    release: Notify,
    drop_settle: AtomicBool,
    hold_settle: AtomicBool,
    settled: Notify,
    settle_release: Notify,
}
async fn vault_gate(State(g): State<Arc<Gate>>, request: Request, next: Next) -> Response {
    let read = matches!(request.uri().path(), "/session/current" | "/api/state");
    if read && g.unavailable.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let hold = request.uri().path() == "/api/state"
        && g.reads.fetch_add(1, Ordering::SeqCst) + 1 == g.hold_at.load(Ordering::SeqCst);
    let response = next.run(request).await;
    if hold {
        g.entered.notify_one();
        g.release.notified().await;
    }
    response
}
async fn assistant_gate(State(g): State<Arc<Gate>>, request: Request, next: Next) -> Response {
    if request.uri().path() != moosuri_vault_protocol::broker::DEVICE_PATH {
        return next.run(request).await;
    }
    let (parts, body) = request.into_parts();
    let bytes = to_bytes(body, 16384).await.unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    let settle = value["command"]["kind"] == "settle";
    let drop = settle && g.drop_settle.swap(false, Ordering::SeqCst);
    let hold = settle && g.hold_settle.swap(false, Ordering::SeqCst);
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    if hold {
        g.settled.notify_one();
        g.settle_release.notified().await;
    }
    if drop {
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    } else {
        response
    }
}
#[derive(Default)]
struct Provider {
    calls: AtomicUsize,
    token_b: AtomicUsize,
    unknown: AtomicBool,
    hold: AtomicBool,
    entered: Notify,
    release: Notify,
}
async fn github(State(p): State<Arc<Provider>>, headers: HeaderMap) -> Response {
    p.calls.fetch_add(1, Ordering::SeqCst);
    let token = headers["authorization"].to_str().unwrap();
    assert!(token == format!("Bearer {TOKEN_A}") || token == format!("Bearer {TOKEN_B}"));
    if token.ends_with(TOKEN_B) {
        p.token_b.fetch_add(1, Ordering::SeqCst);
    }
    if p.hold.swap(false, Ordering::SeqCst) {
        p.entered.notify_one();
        p.release.notified().await;
    }
    if p.unknown.load(Ordering::SeqCst) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    axum::Json(json!({"id":73,"private":true,"archived":false,"stargazers_count":2,"open_issues_count":1,"description":TOKEN_A,"name":TOKEN_B})).into_response()
}
struct Peer {
    client: reqwest::Client,
    origin: String,
    cookie: String,
    owner: Owner,
    engine: ClientVault,
    items: Vec<moosuri_vault_core::EncryptedItem>,
}
impl Peer {
    async fn send(&self, path: &str, method: &str, body: Option<Value>) -> reqwest::Response {
        let mut q = self
            .client
            .request(method.parse().unwrap(), format!("{}{path}", self.origin))
            .header("Origin", &self.origin)
            .header("Cookie", &self.cookie)
            .header("X-Moosuri-Vault-Account", self.owner.account.to_string());
        if let Some(body) = body {
            q = q.json(&body);
        }
        q.send().await.unwrap()
    }
    async fn publish(&self, op: Operation) -> Receipt {
        let id = op.id();
        let response = self
            .send(
                "/api/operations",
                "POST",
                Some(serde_json::to_value(op).unwrap()),
            )
            .await;
        assert!(
            response.status().is_success(),
            "prepare status {}",
            response.status()
        );
        let _: Receipt = response.json().await.unwrap();
        let response = self
            .send(
                &format!("/api/operations/{id}/commit"),
                "POST",
                Some(json!({})),
            )
            .await;
        assert!(
            response.status().is_success(),
            "commit status {}",
            response.status()
        );
        let r: Receipt = response.json().await.unwrap();
        assert_eq!(r.state, "committed");
        r
    }
    async fn put(&mut self, token: &str) {
        let old = self.items.first().cloned();
        let id = old.as_ref().map(|i| i.id).unwrap_or_else(Uuid::new_v4);
        let revision = old.as_ref().map(|i| i.revision).unwrap_or(0);
        let active = self.engine.active().unwrap();
        let item = active
            .seal_item(id, revision + 1, &fields(token), &mut OsEntropy)
            .unwrap();
        let header = hex(&moosuri_credential_broker::auth::hash(
            &active.header().to_json().unwrap(),
        ));
        self.publish(Operation::Put {
            id: Uuid::new_v4(),
            expected_header: header,
            expected_revision: revision,
            item: item.clone(),
        })
        .await;
        self.items = vec![item];
    }
    async fn delete(&mut self) {
        let old = self.items.first().unwrap();
        let expected_header = hex(&moosuri_credential_broker::auth::hash(
            &self.engine.active().unwrap().header().to_json().unwrap(),
        ));
        self.publish(Operation::Delete {
            id: Uuid::new_v4(),
            expected_header,
            item: old.id,
            expected_revision: old.revision,
        })
        .await;
        self.items.clear();
    }
    async fn key_change(&mut self, rotation: bool) {
        let active = self.engine.active().unwrap();
        let expected_header = hex(&moosuri_credential_broker::auth::hash(
            &active.header().to_json().unwrap(),
        ));
        let expected_inventory = inventory_digest(&self.items).unwrap();
        if rotation {
            self.engine
                .prepare_rotation(
                    &secret(NEXT_MASTER),
                    &self.items,
                    expected_inventory,
                    &mut OsEntropy,
                )
                .unwrap();
            let recovery = RecoveryKey::parse(
                &self
                    .engine
                    .pending_recovery()
                    .unwrap()
                    .unwrap()
                    .expose_encoded(),
            )
            .unwrap();
            self.engine.verify_recovery(&recovery).unwrap();
        } else {
            self.engine
                .prepare_password(&secret(NEXT_MASTER), &mut OsEntropy)
                .unwrap();
        }
        let (id, digest) = self.engine.pending_candidate().unwrap();
        let op = Operation::Key {
            id,
            expected_header: Some(expected_header),
            expected_inventory: hex(&expected_inventory),
            header: self.engine.pending_header().unwrap().clone(),
            items: self.engine.pending_items().unwrap().to_vec(),
            commitment: hex(&digest),
        };
        let receipt = self.publish(op).await;
        assert_eq!(receipt.commitment.as_deref(), Some(hex(&digest).as_str()));
        self.engine
            .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
            .unwrap();
        let changed = self.engine.activate().unwrap();
        if rotation {
            self.items = changed;
        }
    }
}
async fn native(assistant: &str, vault: &str, owner: Owner, master: &str) -> LiveVault {
    let mut live = LiveVault::synthetic_login(
        assistant,
        vault,
        owner,
        "native@example.test",
        &secret(PASSWORD),
    )
    .await
    .unwrap();
    live.unlock(&secret(master)).await.unwrap();
    live
}
async fn job(
    client: &reqwest::Client,
    assistant: &str,
    cookie: &str,
    owner: &Owner,
    repo: &GitHubRepository,
) -> String {
    let id = Uuid::new_v4().to_string();
    let r = client
        .post(format!("{assistant}/api/v1/credential-broker/github"))
        .header("Origin", assistant)
        .header("Cookie", cookie)
        .header("X-Moosuri-Account", owner.account.to_string())
        .json(&json!({"request_id":id,"repository":repo}))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());
    let value: Value = r.json().await.unwrap();
    assert_eq!(value["request_id"], id);
    id
}
async fn signed_command(
    transport: &Transport,
    device: &Device,
    signer: &SigningKey,
    command: Command,
) -> SignedCommand {
    let c: AuthChallenge = transport
        .post(
            "/api/v1/credential-broker/device/challenge",
            &json!({"device":device.id}),
        )
        .await
        .unwrap();
    SignedCommand {
        challenge: c.nonce,
        signature: signer.sign(
            &moosuri_credential_broker::auth::authenticated_bytes(
                &c,
                "POST",
                DEVICE_PATH,
                &command,
            )
            .unwrap(),
        ),
        command,
    }
}
async fn status(store: &Store, user: &moosuri_store::UserContext, id: &str) -> String {
    store
        .github_metadata_jobs(user)
        .await
        .unwrap()
        .into_iter()
        .find(|j| j.request_id == id)
        .unwrap()
        .status
}
#[tokio::test]
async fn native_live_vault_fresh_use_rotation_transient_revalidation_and_signed_receipt_recovery() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let data = private(root.path(), "assistant");
    let broker_dir = root.path().join("broker");
    let store = Store::open(&data).unwrap();
    store
        .bootstrap(
            "Native fixture".into(),
            "native@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let user = store
        .login("native@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let assistant_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let assistant = format!(
        "http://localhost:{}",
        assistant_listener.local_addr().unwrap().port()
    );
    let vault_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let vault = format!("http://{}", vault_listener.local_addr().unwrap());
    let internal_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let internal_origin = format!(
        "http://{}/internal/v1/vault",
        internal_listener.local_addr().unwrap()
    );
    let internal = moosuri_server::vault::adapter_router(store.clone(), KEY).unwrap();
    let internal_task =
        tokio::spawn(async move { axum::serve(internal_listener, internal).await.unwrap() });
    let gates = Arc::new(Gate::default());
    let server_config = moosuri_server::ServerConfig::new(&assistant, true)
        .unwrap()
        .with_vault_origin(&vault, true)
        .unwrap();
    let runtime = server_config.start_runtime(store.clone());
    let app =
        moosuri_server::router_with_runtime(store.clone(), server_config, runtime.clone()).layer(
            middleware::from_fn_with_state(gates.clone(), assistant_gate),
        );
    let assistant_task =
        tokio::spawn(async move { axum::serve(assistant_listener, app).await.unwrap() });
    let vault_app = moosuri_vault_service::http::router(
        moosuri_vault_service::database::Database::open(&root.path().join("vault")).unwrap(),
        moosuri_vault_service::http::Adapter::new(&internal_origin, KEY.into()).unwrap(),
        moosuri_vault_service::http::Config::new(&vault, &assistant, true).unwrap(),
    )
    .layer(middleware::from_fn_with_state(gates.clone(), vault_gate));
    let vault_task =
        tokio::spawn(async move { axum::serve(vault_listener, vault_app).await.unwrap() });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let login = client
        .post(format!("{assistant}/api/v1/auth/login"))
        .header("Origin", &assistant)
        .json(&json!({"email":"native@example.test","password":PASSWORD}))
        .send()
        .await
        .unwrap();
    assert_eq!(login.status(), 200);
    let assistant_cookie = login.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let owner = store.vault_handoff(&user, vault.clone()).await.unwrap();
    let exchange = client
        .post(format!("{vault}/session"))
        .header("Origin", &assistant)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("handoff={}", owner.token))
        .send()
        .await
        .unwrap();
    assert_eq!(exchange.status(), 303);
    let vault_cookie = exchange.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let state: VaultState = client
        .get(format!("{vault}/api/state"))
        .header("Cookie", &vault_cookie)
        .header("X-Moosuri-Vault-Account", user.account().id.clone())
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let owner = state.owner;
    let mut pending = PendingVault::prepare(
        moosuri_vault_core::Binding {
            owner_id: owner.account,
            tenant_id: owner.tenant,
            vault_id: Uuid::new_v4(),
        },
        &secret(MASTER),
        &mut OsEntropy,
    )
    .unwrap();
    let recovery = RecoveryKey::parse(&pending.recovery().expose_encoded()).unwrap();
    pending.verify_recovery(&recovery).unwrap();
    let (id, digest) = pending.candidate().unwrap();
    let op = Operation::Key {
        id,
        expected_header: None,
        expected_inventory: hex(&inventory_digest(&[]).unwrap()),
        header: pending.header().clone(),
        items: vec![],
        commitment: hex(&digest),
    };
    let prepared = client
        .post(format!("{vault}/api/operations"))
        .header("Origin", &vault)
        .header("Cookie", &vault_cookie)
        .header("X-Moosuri-Vault-Account", owner.account.to_string())
        .json(&op)
        .send()
        .await
        .unwrap();
    assert!(prepared.status().is_success());
    let committed = client
        .post(format!("{vault}/api/operations/{id}/commit"))
        .header("Origin", &vault)
        .header("Cookie", &vault_cookie)
        .header("X-Moosuri-Vault-Account", owner.account.to_string())
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert!(committed.status().is_success());
    let receipt: Receipt = committed.json().await.unwrap();
    assert_eq!(receipt.commitment, Some(hex(&digest)));
    pending
        .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    let mut peer = Peer {
        client: client.clone(),
        origin: vault.clone(),
        cookie: vault_cookie,
        owner: owner.clone(),
        engine: ClientVault::new(pending.activate().unwrap()),
        items: vec![],
    };
    peer.put(TOKEN_A).await;
    let item = peer.items[0].id;
    let key = SigningKey::generate_pkcs8().unwrap();
    let signer = SigningKey::from_pkcs8(&key).unwrap();
    let challenge = store
        .broker_pair_challenge(&user, LOGICAL.into(), signer.public_key())
        .await
        .unwrap();
    let mut engine =
        Engine::enroll(&broker_dir, challenge.device.clone(), LOGICAL.into(), &key).unwrap();
    let transport = Arc::new(Transport::synthetic_loopback(LOGICAL, &assistant).unwrap());
    let paired: Device = transport
        .post(
            "/api/v1/credential-broker/device/pair",
            &PairProof {
                challenge: challenge.nonce,
                signature: signer.sign(&pairing_bytes(&challenge).unwrap()),
            },
        )
        .await
        .unwrap();
    assert!(paired.owner == owner);
    let provider = Arc::new(Provider::default());
    let provider_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let adapter = Arc::new(
        GitHubAdapter::synthetic_loopback(provider_listener.local_addr().unwrap()).unwrap(),
    );
    let provider_app = Router::new()
        .route("/repos/fixture-owner/fixture-repo", get(github))
        .with_state(provider.clone());
    let provider_task =
        tokio::spawn(async move { axum::serve(provider_listener, provider_app).await.unwrap() });
    let repository = GitHubRepository {
        owner: "fixture-owner".into(),
        repository: "fixture-repo".into(),
    };
    let mut live = native(&assistant, &vault, owner.clone(), MASTER).await;
    let binding = engine
        .live_grant(&mut live, item, repository.clone(), i64::MAX - 1)
        .await
        .unwrap();
    transport
        .register_live(&mut engine, &binding, &repository)
        .await
        .unwrap();
    for _ in 0..2 {
        let id = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
        assert_eq!(
            transport
                .poll_live(&mut engine, &mut live, &adapter)
                .await
                .unwrap(),
            1
        );
        assert_eq!(status(&store, &user, &id).await, "complete");
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    let wrong = GitHubRepository {
        owner: "other-owner".into(),
        repository: "other-repo".into(),
    };
    let wrong_id = job(&client, &assistant, &assistant_cookie, &owner, &wrong).await;
    assert_eq!(
        transport
            .poll_live(&mut engine, &mut live, &adapter)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        status(&store, &user, &wrong_id).await,
        "waiting_for_credential"
    );
    // Offline is suspension, not a password prompt or authority to dispatch.
    let retry_id = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    gates.unavailable.store(true, Ordering::SeqCst);
    assert!(matches!(
        transport.poll_live(&mut engine, &mut live, &adapter).await,
        Err(Error::Unavailable)
    ));
    assert!(live.is_unlocked());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    gates.unavailable.store(false, Ordering::SeqCst);
    transport
        .poll_live(&mut engine, &mut live, &adapter)
        .await
        .unwrap();
    assert_eq!(status(&store, &user, &retry_id).await, "complete");
    // A transient final freshness failure after writer commitment retains the
    // same-process permit, but never enters the adapter until a fresh retry.
    let final_retry = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    gates
        .hold_at
        .store(gates.reads.load(Ordering::SeqCst) + 2, Ordering::SeqCst);
    let t = transport.clone();
    let a = adapter.clone();
    let running = tokio::spawn(async move {
        let result = t.poll_live(&mut engine, &mut live, &a).await;
        (engine, live, result)
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), gates.entered.notified())
        .await
        .unwrap();
    gates.unavailable.store(true, Ordering::SeqCst);
    gates.release.notify_one();
    let (mut engine, mut live, result) = running.await.unwrap();
    assert!(matches!(result, Err(Error::Unavailable)));
    assert!(live.is_unlocked());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 3);
    assert_eq!(
        status(&store, &user, &final_retry).await,
        "in_flight_or_unknown"
    );
    gates.unavailable.store(false, Ordering::SeqCst);
    transport
        .poll_live(&mut engine, &mut live, &adapter)
        .await
        .unwrap();
    assert_eq!(status(&store, &user, &final_retry).await, "complete");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 4);
    // A held authenticated old item read cannot pass the final fresh-state fence.
    let changed_id = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    gates
        .hold_at
        .store(gates.reads.load(Ordering::SeqCst) + 2, Ordering::SeqCst);
    let t = transport.clone();
    let a = adapter.clone();
    let running = tokio::spawn(async move {
        let result = t.poll_live(&mut engine, &mut live, &a).await;
        (engine, live, result)
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), gates.entered.notified())
        .await
        .unwrap();
    peer.put(TOKEN_B).await;
    gates.release.notify_one();
    let (mut engine, live, result) = running.await.unwrap();
    assert!(matches!(result, Err(Error::Reconnect)));
    assert!(!live.is_unlocked());
    assert_eq!(provider.calls.load(Ordering::SeqCst), 4);
    assert_eq!(status(&store, &user, &changed_id).await, "not_dispatched");
    // Password rewrap requires fresh native unlock; a still-waiting invocation
    // survives reconnection with the same opaque binding ID and scope.
    let waiting = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    let Reply::Waiting { requests, .. } = transport.command(&engine, Command::Poll).await.unwrap()
    else {
        panic!("poll");
    };
    assert_eq!(requests.len(), 1);
    let before_refresh = requests[0].clone();
    let delayed = signed_command(
        &transport,
        engine.device(),
        &signer,
        Command::CommitCurrent {
            invocation: before_refresh.invocation.clone(),
            request_digest: moosuri_credential_broker::auth::hash(&request_bytes(&before_refresh)),
        },
    )
    .await;
    peer.key_change(false).await;
    let mut live = native(&assistant, &vault, owner.clone(), NEXT_MASTER).await;
    transport.sync_retained(&mut engine).await.unwrap();
    let current = engine
        .live_grant(&mut live, item, repository.clone(), i64::MAX - 1)
        .await
        .unwrap();
    assert_eq!(current.id, binding.id);
    assert_eq!(current.epoch, 2);
    transport
        .register_live(&mut engine, &current, &repository)
        .await
        .unwrap();
    // The writer fences both a previously signed old request and legacy
    // digest-free commits after refresh. Invocation identity stays unchanged.
    assert!(
        transport
            .post::<_, Reply>(DEVICE_PATH, &delayed)
            .await
            .is_err()
    );
    let legacy = signed_command(
        &transport,
        engine.device(),
        &signer,
        Command::Commit {
            invocation: before_refresh.invocation.clone(),
        },
    )
    .await;
    assert!(
        transport
            .post::<_, Reply>(DEVICE_PATH, &legacy)
            .await
            .is_err()
    );
    let Reply::Waiting { requests, .. } = transport.command(&engine, Command::Poll).await.unwrap()
    else {
        panic!("poll");
    };
    assert_eq!(requests.len(), 1);
    assert!(requests[0].invocation == before_refresh.invocation);
    assert!(requests[0].binding == current);
    transport
        .poll_live(&mut engine, &mut live, &adapter)
        .await
        .unwrap();
    assert_eq!(status(&store, &user, &waiting).await, "complete");
    assert_eq!(provider.token_b.load(Ordering::SeqCst), 1);
    // Root/key rotation retires the old honest native engine and cannot reuse
    // previous in-memory root/credential after the current-head check fails.
    let rotated = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    peer.key_change(true).await;
    assert!(matches!(
        transport.poll_live(&mut engine, &mut live, &adapter).await,
        Err(Error::Reconnect)
    ));
    assert!(!live.is_unlocked());
    let mut live = native(&assistant, &vault, owner.clone(), NEXT_MASTER).await;
    let refreshed = engine
        .live_grant(&mut live, item, repository.clone(), i64::MAX - 1)
        .await
        .unwrap();
    assert_eq!(refreshed.id, binding.id);
    assert_eq!(refreshed.key_epoch, 2);
    transport
        .register_live(&mut engine, &refreshed, &repository)
        .await
        .unwrap();
    transport
        .poll_live(&mut engine, &mut live, &adapter)
        .await
        .unwrap();
    assert_eq!(status(&store, &user, &rotated).await, "complete");
    // A committed-but-unentered request can expire during the vault outage.
    // Its terminal local admission failure settles NotDispatched and cannot
    // strand subsequent valid work behind the in-memory pending invocation.
    let expires = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 5;
    // Test-only authority deadline in this dedicated synthetic DB, before Poll
    // constructs the writer/native request. No signed request is rewritten.
    let db = rusqlite::Connection::open(data.join("assistant.sqlite")).unwrap();
    assert_eq!(db.execute("UPDATE credential_broker_jobs SET expires_at=?1 WHERE request_id=?2 AND request IS NULL", rusqlite::params![deadline, expires]).unwrap(), 1);
    drop(db);
    let before_expiry = provider.calls.load(Ordering::SeqCst);
    gates
        .hold_at
        .store(gates.reads.load(Ordering::SeqCst) + 2, Ordering::SeqCst);
    let t = transport.clone();
    let a = adapter.clone();
    let running = tokio::spawn(async move {
        let result = t.poll_live(&mut engine, &mut live, &a).await;
        (engine, live, result)
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), gates.entered.notified())
        .await
        .unwrap();
    gates.unavailable.store(true, Ordering::SeqCst);
    gates.release.notify_one();
    let (mut engine, mut live, result) = running.await.unwrap();
    assert!(matches!(result, Err(Error::Unavailable)));
    let expiring_request = engine.retained();
    assert_eq!(expiring_request.len(), 1);
    assert_eq!(expiring_request[0].expires_at, deadline);
    let expiring_invocation = expiring_request[0].invocation.clone();
    assert!(live.is_unlocked());
    assert_eq!(provider.calls.load(Ordering::SeqCst), before_expiry);
    let current = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    if current <= deadline {
        tokio::time::sleep(std::time::Duration::from_secs(
            (deadline - current + 1) as u64,
        ))
        .await;
    }
    gates.unavailable.store(false, Ordering::SeqCst);
    let after_expiry = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    assert_eq!(
        transport
            .poll_live(&mut engine, &mut live, &adapter)
            .await
            .unwrap(),
        2
    );
    assert_eq!(status(&store, &user, &expires).await, "not_dispatched");
    assert_eq!(status(&store, &user, &after_expiry).await, "complete");
    let expired_receipt = engine.receipt(&expiring_invocation).unwrap().unwrap();
    assert!(expired_receipt.receipt.commitment.request.invocation == expiring_invocation);
    assert!(
        expired_receipt.receipt.outcome == moosuri_vault_protocol::broker::Outcome::NotDispatched
    );
    assert!(live.is_unlocked());
    assert!(engine.retained().is_empty());
    assert_eq!(provider.calls.load(Ordering::SeqCst), before_expiry + 1);
    // Native terminal input can drop the polling future while the provider
    // has entered. Its next tick settles that original invocation unknown and
    // advances a later job without unlocking again or entering the first twice.
    let cancelled = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    let before_cancel = provider.calls.load(Ordering::SeqCst);
    provider.hold.store(true, Ordering::SeqCst);
    let mut polling = Box::pin(transport.poll_live(&mut engine, &mut live, &adapter));
    tokio::select! {
        result = &mut polling => panic!("held provider completed before cancellation: {}", result.is_ok()),
        waited = tokio::time::timeout(std::time::Duration::from_secs(5), provider.entered.notified()) => waited.unwrap(),
    }
    drop(polling);
    assert!(live.is_unlocked());
    let cancelled_request = engine.retained();
    assert_eq!(cancelled_request.len(), 1);
    let cancelled_invocation = cancelled_request[0].invocation.clone();
    assert_eq!(
        status(&store, &user, &cancelled).await,
        "in_flight_or_unknown"
    );
    provider.release.notify_one();
    let later = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    assert_eq!(
        transport
            .poll_live(&mut engine, &mut live, &adapter)
            .await
            .unwrap(),
        2
    );
    assert_eq!(status(&store, &user, &cancelled).await, "outcome_unknown");
    assert_eq!(status(&store, &user, &later).await, "complete");
    let unknown_receipt = engine.receipt(&cancelled_invocation).unwrap().unwrap();
    assert!(unknown_receipt.receipt.commitment.request.invocation == cancelled_invocation);
    assert!(unknown_receipt.receipt.outcome == moosuri_vault_protocol::broker::Outcome::Unknown);
    assert!(live.is_unlocked());
    assert!(engine.retained().is_empty());
    assert_eq!(provider.calls.load(Ordering::SeqCst), before_cancel + 2);
    // Cancellation while a persisted signed result awaits settlement ACK must
    // deliver that exact result, rather than trying provider entry again.
    let cancelled_receipt = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    let before_receipt = provider.calls.load(Ordering::SeqCst);
    gates.hold_settle.store(true, Ordering::SeqCst);
    let mut polling = Box::pin(transport.poll_live(&mut engine, &mut live, &adapter));
    tokio::select! {
        result = &mut polling => panic!("held settlement completed before cancellation: {}", result.is_ok()),
        waited = tokio::time::timeout(std::time::Duration::from_secs(5), gates.settled.notified()) => waited.unwrap(),
    }
    drop(polling);
    let retained_receipt = engine.retained();
    assert_eq!(retained_receipt.len(), 1);
    let receipt_invocation = retained_receipt[0].invocation.clone();
    let receipt_before =
        serde_json::to_vec(&engine.receipt(&receipt_invocation).unwrap().unwrap()).unwrap();
    gates.settle_release.notify_one();
    assert_eq!(
        transport
            .poll_live(&mut engine, &mut live, &adapter)
            .await
            .unwrap(),
        1
    );
    assert_eq!(status(&store, &user, &cancelled_receipt).await, "complete");
    assert_eq!(
        serde_json::to_vec(&engine.receipt(&receipt_invocation).unwrap().unwrap()).unwrap(),
        receipt_before
    );
    assert!(engine.retained().is_empty());
    assert_eq!(provider.calls.load(Ordering::SeqCst), before_receipt + 1);
    // A provider failure after entry is unknown, and never reenters on polling.
    let unknown = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    provider.unknown.store(true, Ordering::SeqCst);
    transport
        .poll_live(&mut engine, &mut live, &adapter)
        .await
        .unwrap();
    provider.unknown.store(false, Ordering::SeqCst);
    let unknown_calls = provider.calls.load(Ordering::SeqCst);
    assert_eq!(status(&store, &user, &unknown).await, "outcome_unknown");
    transport
        .poll_live(&mut engine, &mut live, &adapter)
        .await
        .unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), unknown_calls);
    // Lost settlement ACK retries the exact signed receipt, never the provider.
    let lost = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    gates.drop_settle.store(true, Ordering::SeqCst);
    assert!(matches!(
        transport.poll_live(&mut engine, &mut live, &adapter).await,
        Err(Error::Unavailable)
    ));
    let count = provider.calls.load(Ordering::SeqCst);
    assert_eq!(status(&store, &user, &lost).await, "complete");
    let retained = engine.retained();
    assert_eq!(retained.len(), 1);
    let old = engine.receipt(&retained[0].invocation).unwrap().unwrap();
    let bytes = serde_json::to_vec(&old).unwrap();
    drop(engine);
    drop(live);
    let mut engine = Engine::open_local(&broker_dir).unwrap();
    assert_eq!(
        serde_json::to_vec(&engine.receipt(&retained[0].invocation).unwrap().unwrap()).unwrap(),
        bytes
    );
    transport.sync_retained(&mut engine).await.unwrap();
    assert_eq!(provider.calls.load(Ordering::SeqCst), count);
    assert!(engine.retained().is_empty());
    let mut live = native(&assistant, &vault, owner.clone(), NEXT_MASTER).await;
    let deleted = job(&client, &assistant, &assistant_cookie, &owner, &repository).await;
    peer.delete().await;
    assert!(matches!(
        transport.poll_live(&mut engine, &mut live, &adapter).await,
        Err(Error::Reconnect)
    ));
    assert!(!live.is_unlocked());
    assert_eq!(provider.calls.load(Ordering::SeqCst), count);
    assert_eq!(status(&store, &user, &deleted).await, "waiting_for_unlock");
    let mut live = native(&assistant, &vault, owner.clone(), NEXT_MASTER).await;
    let mut epoch_session = native(&assistant, &vault, owner.clone(), NEXT_MASTER).await;
    store
        .broker_revoke(&user, engine.device().id)
        .await
        .unwrap();
    transport
        .poll_live(&mut engine, &mut live, &adapter)
        .await
        .unwrap();
    assert!(engine.is_revoked());
    assert!(!live.is_unlocked());
    assert_eq!(provider.calls.load(Ordering::SeqCst), count);
    let changed_account = store
        .change_password(&user, "Synthetic-replaced-account-password".into())
        .await
        .unwrap();
    assert!(matches!(
        epoch_session.validate().await,
        Err(Error::Reconnect)
    ));
    assert!(!epoch_session.is_unlocked());
    assert_eq!(provider.calls.load(Ordering::SeqCst), count);
    let outcomes = serde_json::to_string(
        &store
            .github_metadata_jobs(&changed_account.user)
            .await
            .unwrap(),
    )
    .unwrap();
    for canary in [TOKEN_A, TOKEN_B, MASTER, NEXT_MASTER] {
        assert!(!outcomes.contains(canary));
        for path in [&data, &broker_dir] {
            for entry in fs::read_dir(path).unwrap() {
                let p = entry.unwrap().path();
                if p.is_file() {
                    assert!(
                        !fs::read(p)
                            .unwrap()
                            .windows(canary.len())
                            .any(|b| b == canary.as_bytes())
                    );
                }
            }
        }
    }
    runtime.shutdown().await;
    assistant_task.abort();
    vault_task.abort();
    internal_task.abort();
    provider_task.abort();
}
