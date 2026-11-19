use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicBool, Ordering},
};
use uuid::Uuid;

fn uid() -> u32 {
    unsafe { libc::geteuid() }
}

fn socket_directory() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("gws")
        .tempdir_in("/tmp")
        .unwrap();
    let root = dir.path().canonicalize().unwrap();
    (dir, root)
}

fn allocation(root: &Path) -> (Uuid, runtime_registry::Allocation, Binding) {
    fs::create_dir(root).unwrap();
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    let request = Uuid::new_v4();
    let allocation = runtime_registry::reserve(root, request, "relay@example.test".into()).unwrap();
    let binding = Binding {
        account_id: Uuid::parse_str(&allocation.account_id).unwrap(),
        tenant_id: Uuid::parse_str(&allocation.tenant_id).unwrap(),
        volume_id: Uuid::parse_str(&allocation.volume_id).unwrap(),
        generation: allocation.generation,
        peer_uid: uid(),
    };
    (request, allocation, binding)
}

fn admit(registry: PathBuf, binding: Binding) -> Admission {
    Admission {
        registry: Arc::new(registry),
        binding,
        callback: None,
        slot: Arc::new(Arc::new(Semaphore::new(1)).try_acquire_owned().unwrap()),
    }
}

#[tokio::test]
async fn connect_framing_accepts_only_canonical_google_https_authorities() {
    for allowed in Destination::ALLOWED {
        let authority = allowed.authority();
        let request = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\nopaque");
        let mut reader = BufReader::new(request.as_bytes());
        assert_eq!(read_connect(&mut reader).await, Some(allowed));
        let mut payload = Vec::new();
        reader.read_to_end(&mut payload).await.unwrap();
        assert_eq!(payload, b"opaque");
    }
    for denied in [
        "GET http://gmail.googleapis.com/ HTTP/1.1\r\n\r\n",
        "CONNECT gmail.googleapis.com:80 HTTP/1.1\r\n\r\n",
        "CONNECT storage.googleapis.com:443 HTTP/1.1\r\n\r\n",
        "CONNECT drive.googleapis.com:443 HTTP/1.1\r\n\r\n",
        "CONNECT www.googleapis.com:443/discovery/v1/apis HTTP/1.1\r\n\r\n",
        "CONNECT gmail.googleapis.com:0443 HTTP/1.1\r\n\r\n",
        "CONNECT gmail.googleapis.com.:443 HTTP/1.1\r\n\r\n",
        "CONNECT gmail.googleapis.com.example.test:443 HTTP/1.1\r\n\r\n",
        "CONNECT https://gmail.googleapis.com:443 HTTP/1.1\r\n\r\n",
        "CONNECT gmail.googleapis.com:443/path HTTP/1.1\r\n\r\n",
        "CONNECT gmail.googleapis.com@127.0.0.1:443 HTTP/1.1\r\n\r\n",
        "CONNECT 127.0.0.1:443 HTTP/1.1\r\n\r\n",
        "CONNECT [::1]:443 HTTP/1.1\r\n\r\n",
        "CONNECT gmail.googleapis.com:443 HTTP/1.1\r\nHost: 127.0.0.1:443\r\n\r\n",
        "CONNECT gmail.googleapis.com:443 HTTP/1.1\r\nContent-Length: 4\r\n\r\naaaa",
        "CONNECT gmail.googleapis.com:443 HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
        "CONNECT gmail.googleapis.com:443 HTTP/1.1\r\nProxy-Authorization: synthetic\r\n\r\n",
        "CONNECT gmail.googleapis.com:443 HTTP/1.1\r\nHost : gmail.googleapis.com:443\r\n\r\n",
        "CONNECT gmail.googleapis.com:443 HTTP/1.1\r\nHost: gmail.googleapis.com:443\r\nHost: gmail.googleapis.com:443\r\n\r\n",
        "CONNECT gmail.googleapis.com:443 HTTP/1.1\n\n",
        "CONNECT gmail.googleapis.com:443 HTTP/1.1\r\nHost: gmail.googleapis.com:443\r\n",
    ] {
        assert_eq!(
            read_connect(&mut BufReader::new(denied.as_bytes())).await,
            None,
            "{denied:?}"
        );
    }
    let oversized = format!(
        "CONNECT gmail.googleapis.com:443 HTTP/1.1\r\nUser-Agent: {}\r\n\r\n",
        "x".repeat(REQUEST_LIMIT as usize)
    );
    assert_eq!(
        read_connect(&mut BufReader::new(oversized.as_bytes())).await,
        None
    );
}

#[test]
fn only_public_unicast_addresses_can_be_dialed() {
    for denied in [
        "0.0.0.0",
        "10.20.0.0",
        "100.64.2.3",
        "127.1.2.3",
        "169.254.169.254",
        "172.31.0.0",
        "192.0.0.9",
        "192.0.2.1",
        "192.168.1.1",
        "198.19.0.1",
        "224.0.0.0",
        "240.0.0.1",
        "::",
        "::1",
        "::ffff:8.8.8.8",
        "64:ff9b::808:808",
        "fc00::1",
        "fe80::1",
        "ff02::1",
        "2001:db8::1",
        "2002:7f00:1::",
        "3fff::1",
    ] {
        assert!(!is_public(denied.parse().unwrap()), "{denied}");
    }
    for allowed in ["142.250.64.110", "2607:f8b0:4000::86"] {
        assert!(is_public(allowed.parse().unwrap()), "{allowed}");
    }
}

#[tokio::test]
async fn member_requires_private_socket_and_kernel_broker_uid_before_selector() {
    let (_temp, root) = socket_directory();
    let path = root.join("test.sock");
    let listener = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    let proxy = start_member(path.clone(), uid() ^ 1).await.unwrap();
    let mut client = TcpStream::connect(proxy.address()).await.unwrap();
    client
        .write_all(b"CONNECT gmail.googleapis.com:443 HTTP/1.1\r\n\r\n")
        .await
        .unwrap();
    let (mut rejected, _) = timeout(REQUEST_TIMEOUT, listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rejected.read(&mut [0u8; 4]).await.unwrap(), 0);
    let mut response = Vec::new();
    client.read_to_end(&mut response).await.unwrap();
    assert_eq!(response, UNAVAILABLE);

    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(
        connect_relay(&path, uid(), Destination::Gmail)
            .await
            .is_none()
    );
    let link = root.join("link");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(
        connect_relay(&link, uid(), Destination::Gmail)
            .await
            .is_none()
    );
}

#[tokio::test]
async fn host_rejects_wrong_peer_invalid_selector_and_fenced_allocation() {
    let (_temp, root) = socket_directory();
    let registry = root.join("registry");
    let (request, allocated, binding) = allocation(&registry);
    runtime_registry::activate_invitation(&registry, request, &allocated).unwrap();
    for (binding, selector) in [
        (
            Binding {
                peer_uid: uid() ^ 1,
                ..binding
            },
            Destination::Gmail.frame(),
        ),
        (binding, *b"BAD!"),
        (binding, [b'G', b'W', b'1', 7]),
        (
            Binding {
                generation: binding.generation + 1,
                ..binding
            },
            Destination::Gmail.frame(),
        ),
    ] {
        let (mut client, host) = UnixStream::pair().unwrap();
        client.write_all(&selector).await.unwrap();
        relay_connection(host, admit(registry.clone(), binding), |_| async {
            panic!("unexpected upstream connection");
        })
        .await;
        let mut bytes = Vec::new();
        let _ = client.read_to_end(&mut bytes).await;
        assert!(bytes.is_empty());
    }

    runtime_registry::set_routing(&registry, &allocated, false).unwrap();
    let (mut client, host) = UnixStream::pair().unwrap();
    client.write_all(&Destination::Gmail.frame()).await.unwrap();
    relay_connection(host, admit(registry, binding), |_| async {
        panic!("unexpected upstream connection after fencing");
    })
    .await;
    assert_eq!(client.read(&mut [0u8; 4]).await.unwrap(), 0);
}

#[tokio::test]
async fn proxy_forwards_opaque_bytes_and_closes_open_tunnel_after_fencing() {
    let (_temp, root) = socket_directory();
    let registry = root.join("registry");
    let (request, allocated, binding) = allocation(&registry);
    runtime_registry::activate_invitation(&registry, request, &allocated).unwrap();
    let (host, guard) = listener(&root, binding).await.unwrap();
    let path = root
        .join(binding.account_id.to_string())
        .join("google.sock");
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert_eq!(metadata.mode() & 0o7777, 0o600);
    assert_eq!(metadata.uid(), uid());
    assert_eq!(
        fs::symlink_metadata(path.parent().unwrap()).unwrap().mode() & 0o7777,
        0o711
    );
    let proxy = start_member(path.clone(), uid()).await.unwrap();
    assert_eq!(proxy.address().ip(), Ipv4Addr::LOCALHOST);

    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let peer = upstream.local_addr().unwrap();
    let admission = admit(registry.clone(), binding);
    let relay = tokio::spawn(async move {
        let (stream, _) = host.accept().await.unwrap();
        relay_connection(stream, admission, |destination| async move {
            assert_eq!(destination, Destination::Gmail);
            TcpStream::connect(peer).await.ok()
        })
        .await;
    });

    let mut client = TcpStream::connect(proxy.address()).await.unwrap();
    client.write_all(b"CONNECT gmail.googleapis.com:443 HTTP/1.1\r\nHost: gmail.googleapis.com:443\r\n\r\n\x16\x03\x01synthetic").await.unwrap();
    let mut client_reply = [0; ESTABLISHED.len()];
    client.read_exact(&mut client_reply).await.unwrap();
    assert_eq!(client_reply.as_slice(), ESTABLISHED);
    let (mut upstream, _) = upstream.accept().await.unwrap();
    let mut upstream_seen = [0; b"\x16\x03\x01synthetic".len()];
    upstream.read_exact(&mut upstream_seen).await.unwrap();
    assert_eq!(&upstream_seen, b"\x16\x03\x01synthetic");
    upstream.write_all(b"\x17\x03\x03ciphertext").await.unwrap();
    let mut opaque_reply = [0; b"\x17\x03\x03ciphertext".len()];
    client.read_exact(&mut opaque_reply).await.unwrap();
    assert_eq!(&opaque_reply, b"\x17\x03\x03ciphertext");

    runtime_registry::set_routing(&registry, &allocated, false).unwrap();
    assert_eq!(
        timeout(
            FENCE_INTERVAL + Duration::from_secs(2),
            client.read(&mut [0])
        )
        .await
        .unwrap()
        .unwrap(),
        0
    );
    assert_eq!(upstream.read(&mut [0]).await.unwrap(), 0);
    relay.await.unwrap();
    drop(guard);
    assert!(!path.exists());
}

#[tokio::test]
async fn reqwest_sends_tls_through_proxy_and_dropping_guard_closes_active_tunnel() {
    let (_temp, root) = socket_directory();
    let path = root.join("test.sock");
    let fake_relay = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let proxy = start_member(path, uid()).await.unwrap();
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::https(format!("http://{}", proxy.address())).unwrap())
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap();
    let request = tokio::spawn(async move {
        client
            .get("https://oauth2.googleapis.com/token")
            .send()
            .await
    });
    let (mut stream, _) = timeout(REQUEST_TIMEOUT, fake_relay.accept())
        .await
        .unwrap()
        .unwrap();
    let mut selected = [0; 4];
    stream.read_exact(&mut selected).await.unwrap();
    assert_eq!(selected, Destination::Token.frame());
    stream.write_u8(1).await.unwrap();
    let mut hello = [0; 2];
    stream.read_exact(&mut hello).await.unwrap();
    assert_eq!(hello, [0x16, 0x03]);
    drop(proxy);
    let mut remaining_hello = Vec::new();
    timeout(REQUEST_TIMEOUT, stream.read_to_end(&mut remaining_hello))
        .await
        .unwrap()
        .unwrap();
    assert!(request.await.unwrap().is_err());
}

#[tokio::test]
async fn fencing_during_upstream_dial_refuses_successful_handshake() {
    let (_temp, root) = socket_directory();
    let registry = root.join("registry");
    let (request, allocated, binding) = allocation(&registry);
    runtime_registry::activate_invitation(&registry, request, &allocated).unwrap();
    let (mut client, host) = UnixStream::pair().unwrap();
    client.write_all(&Destination::Gmail.frame()).await.unwrap();
    let upstream = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let seen = AtomicBool::new(false);
    relay_connection(host, admit(registry.clone(), binding), |_| async {
        seen.store(true, Ordering::Relaxed);
        runtime_registry::set_routing(&registry, &allocated, false).unwrap();
        TcpStream::connect(upstream.local_addr().unwrap())
            .await
            .ok()
    })
    .await;
    assert!(seen.load(Ordering::Relaxed));
    let (mut server, _) = upstream.accept().await.unwrap();
    assert_eq!(server.read(&mut [0]).await.unwrap(), 0);
    assert_eq!(client.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn member_connection_limit_queues_a_new_tunnel_until_a_slot_is_available() {
    let (_temp, root) = socket_directory();
    let path = root.join("test.sock");
    let fake_relay = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let proxy = start_member(path, uid()).await.unwrap();
    let request = b"CONNECT gmail.googleapis.com:443 HTTP/1.1\r\n\r\n";
    let mut in_flight = Vec::new();
    for _ in 0..MEMBER_CONNECTIONS {
        let mut client = TcpStream::connect(proxy.address()).await.unwrap();
        client.write_all(request).await.unwrap();
        let (mut relay, _) = timeout(REQUEST_TIMEOUT, fake_relay.accept())
            .await
            .unwrap()
            .unwrap();
        let mut selector = [0; 4];
        relay.read_exact(&mut selector).await.unwrap();
        assert_eq!(selector, Destination::Gmail.frame());
        in_flight.push((client, relay));
    }

    let mut queued = TcpStream::connect(proxy.address()).await.unwrap();
    queued.write_all(request).await.unwrap();
    assert!(
        timeout(Duration::from_millis(100), fake_relay.accept())
            .await
            .is_err()
    );

    drop(in_flight.pop());
    let (mut relay, _) = timeout(REQUEST_TIMEOUT, fake_relay.accept())
        .await
        .unwrap()
        .unwrap();
    let mut selector = [0; 4];
    relay.read_exact(&mut selector).await.unwrap();
    assert_eq!(selector, Destination::Gmail.frame());
    relay.write_u8(1).await.unwrap();
    let mut response = [0; ESTABLISHED.len()];
    queued.read_exact(&mut response).await.unwrap();
    assert_eq!(response.as_slice(), ESTABLISHED);
}

#[tokio::test]
async fn managed_bindings_add_and_remove_sockets_without_restarting_other_members() {
    let (_temp, root) = socket_directory();
    let registry = root.join("registry");
    let (_, _, first) = allocation(&registry);
    let second = Binding {
        account_id: Uuid::new_v4(),
        ..first
    };
    let registry = Arc::new(registry);
    let slots = Arc::new(Semaphore::new(4));
    let callback = Some("https://example.test/api/v1/mail/oauth/callback".to_owned());
    let mut members = BTreeMap::new();

    reconcile_listeners(
        &root,
        &registry,
        &slots,
        &mut members,
        vec![first],
        callback.clone(),
    )
    .await
    .unwrap();
    let path = |b: Binding| root.join(b.account_id.to_string()).join("google.sock");
    let original = fs::symlink_metadata(path(first)).unwrap().ino();
    reconcile_listeners(
        &root,
        &registry,
        &slots,
        &mut members,
        vec![first, second],
        callback.clone(),
    )
    .await
    .unwrap();
    assert_eq!(fs::symlink_metadata(path(first)).unwrap().ino(), original);
    assert!(path(second).exists());
    reconcile_listeners(
        &root,
        &registry,
        &slots,
        &mut members,
        vec![second],
        callback,
    )
    .await
    .unwrap();
    assert!(!path(first).exists());
    assert!(path(second).exists());
    drop(members);
    assert!(!path(second).exists());
}

#[tokio::test]
async fn managed_registration_is_bounded_and_requires_the_expected_broker() {
    let (_temp, root) = socket_directory();
    let path = root.join("fixture.sock");
    let relay = UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let client = tokio::spawn({
        let path = path.clone();
        async move { registration(&path, uid() ^ 1).await }
    });
    let (mut socket, _) = relay.accept().await.unwrap();
    assert_eq!(socket.read(&mut [0; 4]).await.unwrap(), 0);
    assert!(client.await.unwrap().is_none());
    let client = tokio::spawn(async move { registration(&path, uid()).await });
    let (mut socket, _) = relay.accept().await.unwrap();
    let mut frame = [0; 4];
    socket.read_exact(&mut frame).await.unwrap();
    assert_eq!(frame, REGISTRATION);
    socket.write_u16(1).await.unwrap();
    socket.write_u16((CLIENT_LIMIT + 1) as u16).await.unwrap();
    assert!(client.await.unwrap().is_none());
}

#[tokio::test]
async fn active_member_receives_only_the_exact_installation_registration() {
    let (_temp, root) = socket_directory();
    let registry = root.join("registry");
    let (request, a, b) = allocation(&registry);
    let callback = "https://example.test/api/v1/mail/oauth/callback";
    let fixture = serde_json::json!({"web":{"client_id":"synthetic","client_secret":"synthetic","redirect_uris":[callback]}}).to_string();
    let installed = registry.join("google-web-client.json");
    let mut members = BTreeMap::new();
    reconcile_listeners(
        &root,
        &Arc::new(registry.clone()),
        &Arc::new(Semaphore::new(4)),
        &mut members,
        vec![b],
        Some(callback.into()),
    )
    .await
    .unwrap();
    let socket = root.join(b.account_id.to_string()).join("google.sock");
    assert!(registration(&socket, uid()).await.is_none());
    runtime_registry::activate_invitation(&registry, request, &a).unwrap();
    assert!(registration(&socket, uid()).await.is_none());
    fs::write(&installed, fixture.as_bytes()).unwrap();
    fs::set_permissions(&installed, fs::Permissions::from_mode(0o600)).unwrap();
    let (received, app) = registration(&socket, uid()).await.unwrap();
    assert_eq!(received, callback);
    assert_eq!(app.as_slice(), fixture.as_bytes());
    fs::write(
        &installed,
        fixture.replace("example.test", "different.test"),
    )
    .unwrap();
    assert!(registration(&socket, uid()).await.is_none());
    fs::write(&installed, fixture.as_bytes()).unwrap();
    runtime_registry::set_routing(&registry, &a, false).unwrap();
    assert!(registration(&socket, uid()).await.is_none());
}
