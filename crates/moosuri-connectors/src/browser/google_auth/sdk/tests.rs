use super::*;
use std::os::unix::fs::PermissionsExt;

const STATE: &str = "abcdefghijklmnopqrstuvwxyz012345";
const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

fn email() -> Email {
    Email::parse("Admin@Example.com").unwrap()
}
fn url_with(edit: impl FnOnce(&mut Vec<(String, String)>)) -> String {
    let mut p: Vec<(String, String)> = [
        ("response_type", "code"),
        ("client_id", SDK_CLIENT_ID),
        ("redirect_uri", "http://localhost:8085/"),
        ("scope", SCOPES.join(" ").as_str()),
        ("state", STATE),
        ("code_challenge", CHALLENGE),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("login_hint", "admin@example.com"),
    ]
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .into();
    edit(&mut p);
    Url::parse_with_params("https://accounts.google.com/o/oauth2/auth", p)
        .unwrap()
        .to_string()
}
fn set(p: &mut Vec<(String, String)>, key: &str, value: &str) {
    p.iter_mut().find(|(k, _)| k == key).unwrap().1 = value.to_owned();
}

#[test]
fn accepts_exact_sdk_url_and_hides_it_from_debug() {
    let raw = url_with(|_| {});
    let request = validate_login_url(&raw, &email()).unwrap();
    assert_eq!(request.url(), raw);
    assert_eq!(request.callback(), "http://localhost:8085/");
    assert_eq!(request.state(), STATE);
    let shown = format!("{request:?} {:?}", email());
    assert!(!shown.contains(STATE) && !shown.contains("example.com"));
}

#[test]
fn rejects_unexpected_url_fields() {
    let bad: Vec<String> = vec![
        url_with(|p| p.push(("state".into(), "second-state-value-123".into()))),
        url_with(|p| p.push(("extra".into(), "1".into()))),
        url_with(|p| set(p, "client_id", "1.apps.googleusercontent.com")),
        url_with(|p| set(p, "code_challenge_method", "plain")),
        url_with(|p| set(p, "state", "short")),
        url_with(|p| set(p, "login_hint", "other@example.com")),
        url_with(|p| set(p, "redirect_uri", "http://localhost/")),
        url_with(|p| set(p, "redirect_uri", "http://evil.example:8085/")),
        url_with(|p| set(p, "redirect_uri", "https://localhost:8085/")),
        url_with(|p| set(p, "redirect_uri", "http://localhost:8085/cb")),
        url_with(|p| {
            let s = SCOPES[..6].join(" ");
            set(p, "scope", &s)
        }),
        url_with(|p| {
            let s = format!("{} https://mail.google.com/", SCOPES.join(" "));
            set(p, "scope", &s)
        }),
        url_with(|p| p.retain(|(k, _)| k != "code_challenge")),
        url_with(|_| {}).replace("accounts.google.com", "accounts.google.com.evil.test"),
        url_with(|_| {}).replace("https://", "http://"),
        url_with(|_| {}) + "#fragment",
    ];
    for raw in bad {
        assert_eq!(
            validate_login_url(&raw, &email()).unwrap_err(),
            Error::Invalid,
            "{raw}"
        );
    }
}

#[test]
fn extracts_url_only_after_complete_marker_line() {
    let url = url_with(|_| {});
    let text = format!("warning\n{OPENED_MARKER}\n\n    {url}\n");
    assert_eq!(extract_url(text.as_bytes()).unwrap().as_str(), url);
    assert!(extract_url(text.trim_end().as_bytes()).is_none());
    assert!(extract_url(url.as_bytes()).is_none());
}

#[test]
fn identifiers_are_strict() {
    assert!(ProjectId::parse("moosuri-0123456789abcdef0123").is_ok());
    for bad in [
        "moosuri-0123456789ABCDEF0123",
        "moosuri-0123",
        "x-0123456789abcdef0123",
        "moosuri-0123456789abcdef012g",
    ] {
        assert!(ProjectId::parse(bad).is_err());
    }
    assert!(SetupLabel::parse("setup-0123abcd").is_ok());
    assert!(SetupLabel::parse("Setup_0123abcd").is_err());
    assert!(SetupLabel::parse("short").is_err());
    assert!(Email::parse("-o@example.com").is_err());
    assert!(Email::parse("a b@example.com").is_err());
    assert!(Email::parse("a@localhost").is_err());
}

#[test]
fn project_identity_labels_and_unbilled_state() {
    let id = ProjectId::parse("moosuri-0123456789abcdef0123").unwrap();
    let setup = SetupLabel::parse("setup-0123abcd").unwrap();
    let project = |state: &str, labels: &str, name: &str| {
        format!(
            r#"{{"projectId":"{}","name":"{name}","lifecycleState":"{state}","labels":{labels}}}"#,
            id.as_str()
        )
    };
    let good = r#"{"moosuri-purpose":"oauth","moosuri-setup":"setup-0123abcd"}"#;
    assert!(verify_project(project("ACTIVE", good, id.as_str()).as_bytes(), &id, &setup).is_ok());
    for bad in [
        project("DELETE_REQUESTED", good, id.as_str()),
        project("ACTIVE", good, "other"),
        project(
            "ACTIVE",
            r#"{"moosuri-purpose":"oauth","moosuri-setup":"setup-ffffffff"}"#,
            id.as_str(),
        ),
        project("ACTIVE", r#"{"moosuri-purpose":"oauth"}"#, id.as_str()),
        project(
            "ACTIVE",
            r#"{"moosuri-purpose":"oauth","moosuri-setup":"setup-0123abcd","x":"y"}"#,
            id.as_str(),
        ),
    ] {
        assert_eq!(
            verify_project(bad.as_bytes(), &id, &setup),
            Err(Error::Blocked)
        );
    }
    let billing = |tail: &str| format!(r#"{{"projectId":"{}",{tail}}}"#, id.as_str());
    assert!(verify_unbilled(billing(r#""billingEnabled":false"#).as_bytes(), &id).is_ok());
    for bad in [
        billing(r#""billingEnabled":true"#),
        billing(r#""billingEnabled":false,"billingAccountName":"billingAccounts/X""#),
        billing(r#""name":"x""#),
    ] {
        assert_eq!(verify_unbilled(bad.as_bytes(), &id), Err(Error::Blocked));
    }
}

#[test]
fn enabled_api_set_is_fixed() {
    let list = |names: &[&str]| {
        let items: Vec<String> = names
            .iter()
            .map(|n| format!(r#"{{"config":{{"name":"{n}"}}}}"#))
            .collect();
        format!("[{}]", items.join(","))
    };
    let mut names = APIS.to_vec();
    names.push("serviceusage.googleapis.com");
    assert!(verify_apis(list(&names).as_bytes()).is_ok());
    assert_eq!(
        verify_apis(list(&names[1..]).as_bytes()),
        Err(Error::Blocked)
    );
    assert_eq!(
        verify_apis(list(&["calendar.googleapis.com"]).as_bytes()),
        Err(Error::Blocked)
    );
    assert_eq!(verify_apis(b"{}"), Err(Error::Unavailable));
}

#[test]
fn account_list_parsing_supports_mismatch_and_empty_proof() {
    let one = parse_accounts(br#"[{"account":"Other@Example.com","status":"ACTIVE"}]"#).unwrap();
    assert_eq!(one, [("other@example.com".to_owned(), "ACTIVE".to_owned())]);
    assert_ne!(one[0].0, email().0);
    assert!(parse_accounts(b"[]").unwrap().is_empty());
    assert!(parse_accounts(br#"[{"status":"ACTIVE"}]"#).is_err());
}

#[test]
fn child_environment_is_private_and_clean() {
    let root = tempfile::tempdir().unwrap();
    let proxy = private_proxy("10.42.17.2").unwrap();
    let sdk = Sdk::open(root.path(), PathBuf::from("/usr/bin/true"), proxy).unwrap();
    let mode = std::fs::metadata(sdk.dir.path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
    let env = sdk.env();
    let get = |k: &str| {
        env.iter()
            .find(|(n, _)| *n == k)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    assert!(get("HOME").starts_with(sdk.dir.path().to_str().unwrap()));
    assert!(get("CLOUDSDK_CONFIG").starts_with(sdk.dir.path().to_str().unwrap()));
    assert_eq!(get("CLOUDSDK_CORE_DISABLE_FILE_LOGGING"), "true");
    assert_eq!(get("CLOUDSDK_PROXY_ADDRESS"), "10.42.17.2");
    assert!(
        env.iter()
            .all(|(k, _)| !k.starts_with("GOOGLE_") && *k != "GCE_METADATA_HOST")
    );
    assert!(
        Sdk::open(
            root.path(),
            PathBuf::from("/usr/bin/true"),
            "8.8.8.8:3128".parse().unwrap()
        )
        .is_err()
    );
}

/// Synthetic owned "gcloud": logs its fixed arguments, reports a foreign account until
/// `auth revoke` runs. `login` is the shell body of `auth login`.
fn fake_gcloud(dir: &Path, login: &str) -> (PathBuf, PathBuf) {
    let log = dir.join("calls.log");
    let state = dir.join("revoked");
    let script = dir.join("gcloud");
    let body = format!(
        "#!/bin/sh\necho \"$*\" >> {log}\ncase \"$1 $2\" in\n\
         \"auth login\") {login} ;;\n\
         \"auth list\") if [ -e {state} ]; then echo '[]'; else echo '[{{\"account\":\"other@example.com\",\"status\":\"ACTIVE\"}}]'; fi ;;\n\
         \"auth revoke\") touch {state} ;;\nesac\n",
        log = log.display(),
        state = state.display(),
    );
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    (script, log)
}

fn synthetic_login(sdk: &Sdk) -> LoginHandle {
    let mut command = sdk.command(&["auth", "login"], Stdio::null(), Stdio::piped());
    let mut group = Group::spawn(&mut command).unwrap();
    let stderr = group.0.stderr.take().unwrap();
    LoginHandle {
        group,
        stderr,
        request: validate_login_url(&url_with(|_| {}), &email()).unwrap(),
        email: email(),
        owner: sdk.dir.path().to_owned(),
    }
}

fn request_for(callback: &str) -> LoginRequest {
    validate_login_url(&url_with(|p| set(p, "redirect_uri", callback)), &email()).unwrap()
}
const CALLBACK: &str = "http://localhost:8085/";

/// Run only in the Google-auth image with the real namespace firewall, seccomp,
/// tmpfs and no external network. No Google request or credential is involved.
#[tokio::test]
#[ignore = "requires the confined Google-auth image; run sdk-browser-smoke.py"]
async fn actual_chrome_intercepts_only_the_bound_top_level_callback() {
    use crate::browser::google_auth::{Binding, chrome::Chrome};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use uuid::Uuid;
    let binding = Binding {
        installation: Uuid::new_v4(),
        account: Uuid::new_v4(),
        tenant: std::env::var("MOOSURI_BROWSER_TENANT")
            .unwrap()
            .parse()
            .unwrap(),
        volume: Uuid::new_v4(),
        generation: 1,
        epoch: 0,
        challenge: Uuid::new_v4(),
        email: "member@example.test".into(),
        origin: "https://moosuri.example.test".into(),
    };
    let cancel = CancellationToken::new();
    let mut chrome = Chrome::start(&binding, cancel.clone()).await.unwrap();
    let sdk = Sdk::open(
        Path::new(PROFILES),
        PathBuf::from("/usr/bin/true"),
        private_proxy("10.42.17.2").unwrap(),
    )
    .unwrap();
    // A failed authentication capsule is destroyed, never restarted in place.
    // Only clean success reuses the same Chrome profile (in a new tab).
    let case = std::env::var("MOOSURI_SDK_TEST_CASE").unwrap_or_else(|_| "valid".into());
    if case == "valid" {
        let actual = Sdk::create(private_proxy("10.42.17.2").unwrap()).unwrap();
        actual.cleanup_login_credentials().await.unwrap();
        actual.cleanup_login_credentials().await.unwrap();
        assert!(!actual.dir.path().join("config/logs").exists());
    }
    let cases = match case.as_str() {
        "valid" => vec!["valid", "valid_again"],
        "wrong_state" | "iframe" | "popup" | "subresource" | "burst" | "retry_after_error" => {
            vec![case.as_str()]
        }
        _ => panic!("unknown synthetic scenario"),
    };
    for scenario in cases {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let callback = format!(
            "http://localhost:{}/",
            listener.local_addr().unwrap().port()
        );
        let state = if matches!(scenario, "wrong_state" | "burst" | "retry_after_error") {
            "other-state"
        } else {
            STATE
        };
        let target = format!("{callback}?state={state}&code=synthetic-only-code");
        let escaped = target.replace('&', "&amp;");
        let html = match scenario {
            "iframe" => format!("<iframe src='{escaped}'></iframe>"),
            "popup" => format!(
                "<script>window.open({})</script>",
                serde_json::to_string(&target).unwrap()
            ),
            "subresource" => format!(
                "<script>fetch({},{{mode:'no-cors'}}).catch(()=>0)</script>",
                serde_json::to_string(&target).unwrap()
            ),
            // Many callback-shaped subframes plus a wrong-state top-level navigation:
            // exceeds the paused-queue bound and must fail closed, not leak.
            "burst" => format!(
                "{}<script>location.replace({})</script>",
                format!("<iframe src='{escaped}'></iframe>").repeat(12),
                serde_json::to_string(&target).unwrap()
            ),
            _ => format!(
                "<script>location.replace({})</script>",
                serde_json::to_string(&target).unwrap()
            ),
        };
        let mut handle = synthetic_login(&sdk);
        handle.request = request_for(&callback);
        // Private test module replaces ONLY the initial document with synthetic HTML.
        // Production LoginRequest constructors accept Google's exact SDK URL only.
        handle.request.url = Zeroizing::new(format!(
            "data:text/html,{}",
            url::form_urlencoded::byte_serialize(html.as_bytes()).collect::<String>()
        ));
        let limit = if matches!(scenario, "popup" | "subresource") {
            4
        } else {
            12
        };
        let captured = chrome
            .authorize(
                handle.request(),
                &email(),
                &cancel,
                Duration::from_secs(limit),
            )
            .await;
        // Even a valid callback must not have reached the loopback server via Chrome.
        assert!(
            tokio::time::timeout(Duration::from_millis(150), listener.accept())
                .await
                .is_err(),
            "browser bypassed callback interception"
        );
        if scenario == "retry_after_error" {
            // A tainted Chrome is never reused by a (possibly retried) authorize.
            // The first call (wrong state) already failed and tainted Chrome.
            assert!(captured.is_err() && !chrome.is_clean());
            assert!(
                chrome
                    .authorize(handle.request(), &email(), &cancel, Duration::from_secs(2))
                    .await
                    .is_err()
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(150), listener.accept())
                    .await
                    .is_err()
            );
            continue;
        }
        if scenario.starts_with("valid") {
            assert!(
                chrome.is_clean(),
                "clean success must leave Chrome reusable"
            );
            let captured = captured.expect("valid top-level callback was not captured");
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 2048];
                let size = stream.read(&mut buf).await.unwrap();
                assert!(
                    std::str::from_utf8(&buf[..size])
                        .unwrap()
                        .starts_with(&format!(
                            "GET /?state={STATE}&code=synthetic-only-code HTTP/1.1\r\n"
                        ))
                );
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .await
                    .unwrap();
            });
            sdk.deliver_callback(&handle, captured, &cancel)
                .await
                .unwrap();
            server.await.unwrap();
        } else {
            assert!(captured.is_err(), "unbound callback was accepted");
            assert!(!chrome.is_clean(), "error must taint Chrome");
        }
    }
    chrome.shutdown().await;
}

fn cb(query: &str) -> String {
    format!("http://localhost:8085/?{query}")
}

#[test]
fn callback_accepts_only_exact_code_and_state() {
    let request = request_for(CALLBACK);
    assert_eq!(request.fetch_pattern().unwrap(), "http://localhost:8085/*");
    let good = [
        cb(&format!("state={STATE}&code=4%2Fsynthetic-code_1.x")),
        cb(&format!(
            "code=abc&state={STATE}&scope=openid%20email&authuser=0&hd=Example.com&prompt=consent"
        )),
    ];
    for raw in &good {
        let accepted = request.accept_callback(raw).unwrap();
        let shown = format!("{accepted:?}");
        assert!(!shown.contains("abc") && !shown.contains(STATE));
    }
    let bad = [
        cb(&format!("state={STATE}")),                            // no code
        cb(&format!("state={STATE}&code=")),                      // empty code
        cb("code=abc&state=other-state-value-0123"),              // wrong state
        cb("code=abc"),                                           // no state
        cb(&format!("state={STATE}&code=a&code=b")),              // two codes
        cb(&format!("state={STATE}&state={STATE}&code=a")),       // two states
        cb(&format!("state={STATE}&code=a&error=access_denied")), // error
        cb(&format!("state={STATE}&code=a&extra=1")),             // unexpected field
        cb(&format!("state={STATE}&code=a%20b")),                 // unsafe code
        cb(&format!("state={STATE}&code={}", "a".repeat(513))),   // too long
        cb(&format!(
            "state={STATE}&code=a&scope=https://mail.google.com/"
        )),
        cb(&format!("state={STATE}&code=a&authuser=x")),
        cb(&format!("state={STATE}&code=a&hd=evil.test")),
        cb(&format!("state={STATE}&code=a&prompt=login")),
        format!("http://localhost:8086/?state={STATE}&code=a"), // port
        format!("http://127.0.0.1:8085/?state={STATE}&code=a"), // host
        format!("http://localhost.evil.test:8085/?state={STATE}&code=a"),
        format!("https://localhost:8085/?state={STATE}&code=a"), // scheme
        format!("http://localhost:8085/cb?state={STATE}&code=a"), // path
        format!("http://u@localhost:8085/?state={STATE}&code=a"),
        format!("http://localhost:8085/?state={STATE}&code=a#frag"),
        format!("http://localhost:8085/?state={STATE}&code=a\n"),
    ];
    for raw in &bad {
        assert!(request.accept_callback(raw).is_err());
    }
    // A request for 127.0.0.1 accepts only that host.
    let loopback = request_for("http://127.0.0.1:8085/");
    assert!(
        loopback
            .accept_callback(&format!("http://127.0.0.1:8085/?state={STATE}&code=a"))
            .is_ok()
    );
    assert!(loopback.accept_callback(&good[0]).is_err());
}

async fn listener() -> (tokio::net::TcpListener, u16) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    (l, port)
}

fn loopback_login(sdk: &Sdk, port: u16) -> LoginHandle {
    let mut handle = synthetic_login(sdk);
    handle.request = request_for(&format!("http://127.0.0.1:{port}/"));
    handle
}

#[tokio::test]
async fn delivers_exact_get_to_loopback_without_following_redirects() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = tempfile::tempdir().unwrap();
    let sdk = Sdk::open(
        root.path(),
        PathBuf::from("/usr/bin/true"),
        private_proxy("10.42.17.2").unwrap(),
    )
    .unwrap();
    let (server, port) = listener().await;
    let (redirect_target, target_port) = listener().await;
    let handle = loopback_login(&sdk, port);
    let raw = format!("http://127.0.0.1:{port}/?state={STATE}&code=synthetic-code");
    let callback = handle.request.accept_callback(&raw).unwrap();
    let served = tokio::spawn(async move {
        let (mut stream, peer) = server.accept().await.unwrap();
        assert!(peer.ip().is_loopback());
        let mut seen = Vec::new();
        let mut chunk = [0u8; 512];
        while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = stream.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            seen.extend_from_slice(&chunk[..n]);
        }
        let reply = format!(
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{target_port}/next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream.write_all(reply.as_bytes()).await.unwrap();
        String::from_utf8(seen).unwrap()
    });
    sdk.deliver_callback(&handle, callback, &CancellationToken::new())
        .await
        .unwrap();
    let seen = served.await.unwrap();
    assert_eq!(
        seen,
        format!(
            "GET /?state={STATE}&code=synthetic-code HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nAccept: */*\r\n\r\n"
        )
    );
    // The redirect target is never contacted.
    assert!(
        tokio::time::timeout(Duration::from_millis(300), redirect_target.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn bad_or_mismatched_callbacks_never_connect() {
    let root = tempfile::tempdir().unwrap();
    let sdk = Sdk::open(
        root.path(),
        PathBuf::from("/usr/bin/true"),
        private_proxy("10.42.17.2").unwrap(),
    )
    .unwrap();
    let (server, port) = listener().await;
    let (_other, other_port) = listener().await;
    let handle = loopback_login(&sdk, port);
    // Constructed directly (bypassing Chrome-side acceptance) to prove SDK re-validation.
    let forged = |raw: String| Callback {
        url: Zeroizing::new(raw),
    };
    for raw in [
        format!("http://127.0.0.1:{other_port}/?state={STATE}&code=a"),
        format!("http://127.0.0.1:{port}/?state=wrong-state-value-0123&code=a"),
        format!("http://127.0.0.1:{port}/?state={STATE}&code=a&error=x"),
        format!("http://127.0.0.1:{port}/other?state={STATE}&code=a"),
        format!("http://localhost:{port}/?state={STATE}&code=a"),
    ] {
        assert_eq!(
            sdk.deliver_callback(&handle, forged(raw), &CancellationToken::new())
                .await,
            Err(Error::Invalid)
        );
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(300), server.accept())
            .await
            .is_err()
    );
    // A handle from another Sdk directory is refused too.
    let other_sdk = Sdk::open(
        root.path(),
        PathBuf::from("/usr/bin/true"),
        private_proxy("10.42.17.2").unwrap(),
    )
    .unwrap();
    let good = forged(format!("http://127.0.0.1:{port}/?state={STATE}&code=a"));
    assert_eq!(
        other_sdk
            .deliver_callback(&handle, good, &CancellationToken::new())
            .await,
        Err(Error::Invalid)
    );
}

#[tokio::test]
async fn delivery_is_bounded_and_rejects_error_status() {
    use tokio::io::AsyncWriteExt;
    let (server, port) = listener().await;
    let served = tokio::spawn(async move {
        let (mut stream, _) = server.accept().await.unwrap();
        stream
            .write_all(b"HTTP/1.1 500 Internal Server Error\r\n\r\n")
            .await
            .unwrap();
    });
    assert_eq!(
        deliver(port, "127.0.0.1", "/?code=a", &CancellationToken::new()).await,
        Err(Error::Blocked)
    );
    served.await.unwrap();
    // Cancellation wins over a silent listener.
    let (silent, port) = listener().await;
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        deliver(port, "127.0.0.1", "/?code=a", &cancel).await,
        Err(Error::Unavailable)
    );
    drop(silent);
}

fn calls(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn account_mismatch_revokes_all_in_the_private_config_and_keeps_classification() {
    let tools = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (script, log) = fake_gcloud(tools.path(), "exit 0");
    let proxy = private_proxy("10.42.17.2").unwrap();
    let sdk = Sdk::open(root.path(), script, proxy).unwrap();
    let outcome = sdk
        .finish_login(synthetic_login(&sdk), &CancellationToken::new())
        .await;
    assert_eq!(outcome, Err(Error::Blocked));
    assert_eq!(
        calls(&log),
        [
            "auth login",
            "auth list --format=json",
            "auth list --format=json",
            "auth revoke --all --quiet",
            "auth list --format=json"
        ]
    );
}

#[tokio::test]
async fn cancelled_login_kills_child_then_cleans_up_with_its_own_token() {
    let tools = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (script, log) = fake_gcloud(tools.path(), "exec sleep 30");
    let proxy = private_proxy("10.42.17.2").unwrap();
    let sdk = Sdk::open(root.path(), script, proxy).unwrap();
    let handle = synthetic_login(&sdk);
    let pid = handle.group.0.id().unwrap() as i32;
    let cancel = CancellationToken::new();
    cancel.cancel();
    let started = std::time::Instant::now();
    assert_eq!(
        sdk.finish_login(handle, &cancel).await,
        Err(Error::Unavailable)
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    let seen = calls(&log);
    assert!(
        seen.contains(&"auth revoke --all --quiet".to_owned()),
        "{seen:?}"
    );
    // The login group is gone before/regardless of cleanup.
    // SAFETY: signal 0 only probes for the process group.
    assert_ne!(unsafe { libc::kill(-pid, 0) }, 0);
    // Explicit controller cleanup is repeatable and proves an empty account list.
    assert_eq!(sdk.cleanup_login_credentials().await, Ok(()));
}

#[tokio::test]
async fn cleanup_skips_empty_configuration_but_never_accepts_failed_revocation() {
    let tools = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (script, log) = fake_gcloud(tools.path(), "exit 0");
    let body = std::fs::read_to_string(&script).unwrap();
    // Emulate a failed revoke that removes the local account anyway. An empty
    // post-list cannot turn this uncertain provider revocation into success.
    std::fs::write(
        &script,
        body.replace(
            &format!("touch {} ;;", tools.path().join("revoked").display()),
            &format!(
                "touch {}; exit 1 ;;",
                tools.path().join("revoked").display()
            ),
        ),
    )
    .unwrap();
    let sdk = Sdk::open(root.path(), script, private_proxy("10.42.17.2").unwrap()).unwrap();
    assert_eq!(
        sdk.cleanup_login_credentials().await,
        Err(Error::Unavailable)
    );
    std::fs::write(tools.path().join("revoked"), b"").unwrap();
    std::fs::write(&log, b"").unwrap();
    // A retry cannot reclassify the failed revoke by observing local absence.
    assert_eq!(
        sdk.cleanup_login_credentials().await,
        Err(Error::Unavailable)
    );
    assert!(calls(&log).is_empty());
    // An unrelated genuinely empty new SDK configuration needs no revocation.
    let clean = Sdk::open(
        root.path(),
        sdk.gcloud.clone(),
        private_proxy("10.42.17.2").unwrap(),
    )
    .unwrap();
    assert_eq!(clean.cleanup_login_credentials().await, Ok(()));
    assert_eq!(calls(&log), ["auth list --format=json"]);
}

const PROJECT: &str = "moosuri-0123456789abcdef0123";

fn project_json(name: &str, labels: &str) -> String {
    format!(
        r#"{{"projectId":"{PROJECT}","name":"{name}","lifecycleState":"ACTIVE","labels":{labels}}}"#
    )
}
const OURS: &str = r#"{"moosuri-purpose":"oauth","moosuri-setup":"setup-0123abcd"}"#;

/// Synthetic gcloud that logs its arguments and prints `stdout` (or fails).
fn probe_gcloud(dir: &Path, stdout: &str, exit: i32) -> (PathBuf, PathBuf) {
    let log = dir.join("calls.log");
    let script = dir.join("gcloud");
    let body = format!(
        "#!/bin/sh\necho \"$*\" >> {}\ncat <<'EOF'\n{stdout}\nEOF\nexit {exit}\n",
        log.display()
    );
    std::fs::write(&script, body).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    (script, log)
}

async fn probe(stdout: &str, exit: i32) -> (Result<ProjectPresence, Error>, Vec<String>) {
    let tools = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (script, log) = probe_gcloud(tools.path(), stdout, exit);
    let sdk = Sdk::open(root.path(), script, private_proxy("10.42.17.2").unwrap()).unwrap();
    let id = ProjectId::parse(PROJECT).unwrap();
    let setup = SetupLabel::parse("setup-0123abcd").unwrap();
    let result = sdk
        .probe_project(&id, &setup, &CancellationToken::new())
        .await;
    (result, calls(&log))
}

#[tokio::test]
async fn probe_absent_only_for_successful_empty_list_with_fixed_arguments() {
    let (result, seen) = probe("[]", 0).await;
    assert_eq!(result, Ok(ProjectPresence::Absent));
    assert_eq!(
        seen,
        [format!(
            "projects list --filter=projectId={PROJECT} --format=json"
        )]
    );
}

#[tokio::test]
async fn probe_owned_requires_exact_single_verified_project() {
    let (result, _) = probe(&format!("[{}]", project_json(PROJECT, OURS)), 0).await;
    assert_eq!(result, Ok(ProjectPresence::Owned));
}

#[tokio::test]
async fn probe_never_reports_absent_for_foreign_multiple_malformed_or_failed() {
    let ours = project_json(PROJECT, OURS);
    let foreign = project_json(PROJECT, r#"{"team":"x"}"#);
    let cases = [
        (format!("[{foreign}]"), Error::Blocked),
        (
            format!("[{}]", project_json("other-name", OURS)),
            Error::Blocked,
        ),
        (format!("[{ours},{ours}]"), Error::Blocked),
        (format!("[{ours},{foreign}]"), Error::Blocked),
        ("{}".to_owned(), Error::Unavailable),
        ("[1]".to_owned(), Error::Unavailable),
        ("not json".to_owned(), Error::Unavailable),
        (String::new(), Error::Unavailable),
    ];
    for (out, expected) in cases {
        assert_eq!(probe(&out, 0).await.0, Err(expected), "{out}");
    }
    // Command failure is Unavailable even when it printed an empty list.
    assert_eq!(probe("[]", 1).await.0, Err(Error::Unavailable));
}

#[tokio::test]
async fn stop_kills_and_reaps_login_group_before_cleanup() {
    let tools = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (script, log) = fake_gcloud(tools.path(), "exec sleep 30");
    let sdk = Sdk::open(root.path(), script, private_proxy("10.42.17.2").unwrap()).unwrap();
    let handle = synthetic_login(&sdk);
    let pid = handle.group.0.id().unwrap() as i32;
    let started = std::time::Instant::now();
    assert_eq!(handle.stop().await, Ok(()));
    assert!(started.elapsed() < Duration::from_secs(5));
    // SAFETY: signal 0 only probes for the process group.
    assert_ne!(unsafe { libc::kill(-pid, 0) }, 0);
    assert_eq!(sdk.cleanup_login_credentials().await, Ok(()));
    assert!(calls(&log).contains(&"auth revoke --all --quiet".to_owned()));
}

#[tokio::test]
async fn output_is_bounded_and_work_is_cancellable() {
    let cancel = CancellationToken::new();
    let mut endless = Command::new("/usr/bin/yes");
    endless
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    assert_eq!(
        run(endless, 4096, Duration::from_secs(10), &cancel)
            .await
            .unwrap_err(),
        Error::Blocked
    );

    let sleeper = || {
        let mut c = Command::new("/usr/bin/sleep");
        c.env_clear()
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        c
    };
    let started = std::time::Instant::now();
    assert_eq!(
        run(sleeper(), 16, Duration::from_millis(100), &cancel)
            .await
            .unwrap_err(),
        Error::Unavailable
    );
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        token.cancel();
    });
    assert_eq!(
        run(sleeper(), 16, Duration::from_secs(20), &cancel)
            .await
            .unwrap_err(),
        Error::Unavailable
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}
