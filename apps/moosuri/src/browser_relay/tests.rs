use super::*;
#[test]
fn membership_and_operation_envelope_is_closed() {
    let mut v = json!({"v":1,"generation":2,"workspace_id":Uuid::new_v4(),"op":"tabs"});
    assert!(Request::parse(&serde_json::to_vec(&v).unwrap()).is_ok());
    v["account_id"] = json!(Uuid::new_v4());
    assert!(Request::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    v.as_object_mut().unwrap().remove("account_id");
    v["generation"] = json!(0);
    assert!(Request::parse(&serde_json::to_vec(&v).unwrap()).is_err());
}

#[tokio::test]
async fn socket_admission_binds_process_uid_registry_generation_and_epoch() {
    use moosuri_store::runtime_registry;
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let registry = root.join("registry");
    fs::create_dir(&registry).unwrap();
    fs::set_permissions(&registry, fs::Permissions::from_mode(0o700)).unwrap();
    let invitation = Uuid::new_v4();
    let a = runtime_registry::reserve(
        &registry,
        invitation,
        "synthetic-member@example.test".into(),
    )
    .unwrap();
    runtime_registry::activate_invitation(&registry, invitation, &a).unwrap();
    let uid = unsafe { libc::geteuid() };
    let b = Binding {
        account_id: a.account_id.parse().unwrap(),
        tenant_id: a.tenant_id.parse().unwrap(),
        volume_id: a.volume_id.parse().unwrap(),
        generation: a.generation,
        peer_uid: uid,
    };
    let config = CapsuleConfig {
        image_id: format!("sha256:{}", "a".repeat(64)),
        network_id: "b".repeat(64),
        proxy_container_id: "c".repeat(64),
        proxy_ip: "172.29.0.2".into(),
        owner: Uuid::new_v4(),
        project: "kiri-test".into(),
        seccomp_file: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../deployment/browser/seccomp.json"),
        verified: true,
        smoke_evidence_sha256: "d".repeat(64),
        recover_workspaces: false,
        javascript_enabled: false,
    };
    let host = Host {
        registry: Arc::new(registry.clone()),
        reader: CapsuleReader::new(config).unwrap(),
        clients: Arc::new(Semaphore::new(2)),
        checks: Arc::new(Semaphore::new(2)),
    };
    async fn call(b: Binding, host: Host, generation: i64) -> Option<Value> {
        let (mut stream, peer) = UnixStream::pair().unwrap();
        let task = tokio::spawn(serve_client(
            peer,
            host,
            b,
            Arc::new(Mutex::new(Sessions::new())),
            CancellationToken::new(),
        ));
        let mut bytes = serde_json::to_vec(
            &json!({"v":1,"generation":generation,"workspace_id":Uuid::new_v4(),"op":"tabs"}),
        )
        .unwrap();
        bytes.push(b'\n');
        let _ = stream.write_all(&bytes).await;
        // A passive client may half-close after sending its one request. This
        // must not cancel the underlying shared workspace.
        let _ = stream.shutdown().await;
        let mut result = Vec::new();
        let _ = stream.read_to_end(&mut result).await;
        task.await.unwrap();
        serde_json::from_slice(&result).ok()
    }
    assert!(
        call(
            Binding {
                peer_uid: uid ^ 1,
                ..b
            },
            host.clone(),
            a.generation
        )
        .await
        .is_none()
    );
    assert_eq!(
        call(b, host.clone(), a.generation + 1).await.unwrap()["code"],
        "forbidden"
    );
    assert_eq!(
        call(b, host.clone(), a.generation).await.unwrap()["code"],
        "missing"
    );
    assert_eq!(
        call(
            Binding {
                volume_id: Uuid::new_v4(),
                ..b
            },
            host.clone(),
            a.generation
        )
        .await
        .unwrap()["code"],
        "forbidden"
    );
    runtime_registry::set_routing(&registry, &a, false).unwrap();
    assert_eq!(
        call(b, host, a.generation).await.unwrap()["code"],
        "forbidden"
    );
}
