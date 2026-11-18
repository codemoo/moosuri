use super::*;

const EMAIL: &str = "fixture@example.test";
const REDIRECT: &str = "https://assistant.example.test/api/v1/mail/oauth/callback";
const PROXY: &str = "http://127.0.0.1:9";

fn owner() -> VolumeOwner {
    VolumeOwner {
        account: Uuid::new_v4(),
        tenant: Uuid::new_v4(),
        volume: Uuid::new_v4(),
        generation: 3,
    }
}

fn secret(value: &str) -> SecretString {
    SecretString::new(value.into()).unwrap()
}

fn adapter(oauth: GmailClient, script: &str) -> LocalGws {
    let directory = private_directory().unwrap();
    let discovery_cache = seed_discovery(directory.path(), b"{}").unwrap();
    let executable = directory.path().join("synthetic-gws");
    fs::write(&executable, format!("#!/bin/sh\nset -eu\n{script}")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    LocalGws {
        revision: Uuid::new_v4(),
        app: OAuthClient {
            client_id: "fixture-client".into(),
            client_secret: secret("fixture-client-secret"),
            redirect_uri: REDIRECT.into(),
        },
        oauth,
        key: hmac::Key::new(hmac::HMAC_SHA256, &[7; 32]),
        executable,
        proxy: PROXY.into(),
        directory,
        discovery_cache,
    }
}

#[tokio::test]
async fn accepts_only_owner_bound_receipts_and_bounded_cli_output() {
    let gws = adapter(GmailClient::google(), "/usr/bin/head -c 131073 /dev/zero");
    let token = secret("fixture-access");
    let owner = Owner {
        account_id: Uuid::new_v4(),
        tenant_id: Uuid::new_v4(),
        volume_id: Uuid::new_v4(),
        generation: 1,
        peer_uid: unsafe { libc::geteuid() },
    };
    let expires = now().ok().unwrap() + 60;
    let receipt = gws
        .bind(owner, "access", EMAIL, &token, expires)
        .ok()
        .unwrap();
    assert_eq!(
        gws.verify(owner, "access", &receipt, &token)
            .ok()
            .as_deref(),
        Some(EMAIL)
    );
    assert!(
        gws.verify(
            Owner {
                generation: 2,
                ..owner
            },
            "access",
            &receipt,
            &token
        )
        .is_err()
    );
    assert!(gws.verify(owner, "refresh", &receipt, &token).is_err());
    assert!(
        gws.verify(owner, "access", &receipt, &secret("wrong-token"))
            .is_err()
    );
    let expired = gws.bind(owner, "access", EMAIL, &token, 0).ok().unwrap();
    assert!(gws.verify(owner, "access", &expired, &token).is_err());
    assert!(gws.read(GmailRead::Profile, &token).await.is_err());
}

#[test]
fn configuration_input_requires_a_private_regular_file() {
    let directory = private_directory().unwrap();
    assert_eq!(
        fs::metadata(directory.path()).unwrap().mode() & 0o777,
        0o700
    );
    let path = directory.path().join("gws-client.json");
    assert!(matches!(config_bytes(&path), Err("google_setup_required")));
    fs::write(&path, "{}\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(config_bytes(&path).is_err());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(&*config_bytes(&path).unwrap(), b"{}\n");
    let link = directory.path().join("linked.json");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(config_bytes(&link).is_err());
    fs::remove_file(&link).unwrap();
    fs::hard_link(&path, &link).unwrap();
    assert!(config_bytes(&path).is_err());
}

#[tokio::test]
async fn oauth_connector_uses_the_explicit_connect_proxy() {
    use tokio::{io::AsyncWriteExt, net::TcpListener};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let oauth = GmailClient::google_oauth_proxy(listener.local_addr().unwrap()).unwrap();
    let gws = adapter(oauth, "exit 1");
    let proxy = async {
        let (mut connection, _) = listener.accept().await.unwrap();
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") {
            header.push(connection.read_u8().await.unwrap());
            assert!(header.len() < 4096);
        }
        assert!(header.starts_with(b"CONNECT oauth2.googleapis.com:443 HTTP/1.1\r\n"));
        assert!(!String::from_utf8_lossy(&header).contains("fixture-client-secret"));
        connection
            .write_all(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
    };
    let code = secret("fixture-code");
    let verifier = secret(&"a".repeat(43));
    let exchange = gws.raw(
        owner(),
        Uuid::new_v4(),
        Some(gws.revision),
        Operation::Exchange {
            code: Secret(&code),
            verifier: Secret(&verifier),
        },
    );
    let (result, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(exchange, proxy)
    })
    .await
    .unwrap();
    assert!(result.is_err());
}

#[cfg(feature = "test-fixtures")]
#[tokio::test]
async fn exchange_refresh_and_inbox_use_only_the_current_access_token() {
    use crate::personal_google::{Authorization, BrokerReply, Description};
    use axum::{Json, Router, routing::post};

    let oauth_server = TcpListenerFixture::start(Router::new().route("/", post(|body: String| async move {
        let fields: std::collections::BTreeMap<_, _> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
        assert_eq!(fields["client_id"], "fixture-client");
        assert_eq!(fields["client_secret"], "fixture-client-secret");
        Json(match fields["grant_type"].as_str() {
            "authorization_code" => {
                assert_eq!(fields["redirect_uri"], REDIRECT);
                assert_eq!(fields["code_verifier"], "v".repeat(43));
                assert_eq!(fields["code"], "fixture-code");
                json!({"access_token":"old-access", "refresh_token":"fixture-refresh", "expires_in":3600, "token_type":"Bearer", "scope":READ_SCOPE})
            }
            "refresh_token" => {
                assert_eq!(fields["refresh_token"], "fixture-refresh");
                json!({"access_token":"new-access", "expires_in":3600, "token_type":"Bearer"})
            }
            other => panic!("unexpected synthetic grant type {other}"),
        })
    }))).await;

    let gws = adapter(
        GmailClient::synthetic_loopback(oauth_server.address).unwrap(),
        r#"
[ "$HOME" = "$GOOGLE_WORKSPACE_CLI_CONFIG_DIR" ]
[ "$HOME" -ef "$PWD" ]
[ -f .env ] && [ ! -s .env ]
[ -f cache/gmail_v1.json ]
[ "$HTTPS_PROXY" = 'http://127.0.0.1:9' ]
[ -z "${GOOGLE_APPLICATION_CREDENTIALS+x}" ]
[ -z "${GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE+x}" ]
[ -z "${GOOGLE_WORKSPACE_CLI_CLIENT_SECRET+x}" ]
case "$*" in
  'gmail users getProfile --params {"fields":"emailAddress","userId":"me"} --format json')
    case "$GOOGLE_WORKSPACE_CLI_TOKEN" in old-access|new-access) ;; *) exit 1 ;; esac
    printf '%s\n' '{"emailAddress":"fixture@example.test"}'
    ;;
  'gmail users messages list --params {"fields":"messages(id),nextPageToken","includeSpamTrash":false,"labelIds":["INBOX"],"maxResults":5,"userId":"me"} --format json')
    [ "$GOOGLE_WORKSPACE_CLI_TOKEN" = 'new-access' ]
    printf '%s\n' '{"messages":[{"id":"aa"},{"id":"bb"}],"nextPageToken":"next"}'
    ;;
  'gmail users messages get --params {"fields":"id,labelIds,internalDate,payload(headers)","format":"metadata","id":"aa","metadataHeaders":["Subject","From","Date"],"userId":"me"} --format json')
    [ "$GOOGLE_WORKSPACE_CLI_TOKEN" = 'new-access' ]
    printf '%s\n' '{"id":"aa","labelIds":["INBOX"],"internalDate":"1000","payload":{"headers":[{"name":"Subject","value":"First\nline"}]}}'
    ;;
  'gmail users messages get --params {"fields":"id,labelIds,internalDate,payload(headers)","format":"metadata","id":"bb","metadataHeaders":["Subject","From","Date"],"userId":"me"} --format json')
    [ "$GOOGLE_WORKSPACE_CLI_TOKEN" = 'new-access' ]
    printf '%s\n' '{"id":"bb","labelIds":["INBOX"],"internalDate":"1001","payload":{"headers":[{"name":"From","value":"Fixture <fixture@example.test>"}]}}'
    ;;
  *) exit 1 ;;
esac
"#,
    );

    #[derive(Deserialize)]
    struct Grant {
        access_token: SecretString,
        refresh_token: Option<SecretString>,
        access_binding: String,
        refresh_binding: String,
        scopes: [String; 1],
    }
    let owner = owner();
    let id = Uuid::new_v4();
    let call = |revision, operation| gws.raw(owner, id, revision, operation);
    let revision = Some(gws.revision);
    let described: BrokerReply<Description> =
        decode(&call(None, Operation::Describe).await.ok().unwrap()).unwrap();
    assert_eq!(described.result.scope, READ_SCOPE);
    let verifier = secret(&"v".repeat(43));
    let state = "b".repeat(64);
    let consent: BrokerReply<Authorization> = decode(
        &call(
            revision,
            Operation::AuthorizationUrl {
                state: &state,
                verifier: Secret(&verifier),
            },
        )
        .await
        .ok()
        .unwrap(),
    )
    .unwrap();
    let consent = url::Url::parse(&consent.result.authorization_url).unwrap();
    assert_eq!(consent.host_str(), Some("accounts.google.com"));
    assert!(
        consent
            .query_pairs()
            .any(|(name, value)| name == "scope" && value == READ_SCOPE)
    );

    let code = secret("fixture-code");
    let initial: BrokerReply<Grant> = decode(
        &call(
            revision,
            Operation::Exchange {
                code: Secret(&code),
                verifier: Secret(&verifier),
            },
        )
        .await
        .ok()
        .unwrap(),
    )
    .unwrap();
    let refresh = initial.result.refresh_token.unwrap();
    let refreshed: BrokerReply<Grant> = decode(
        &call(
            revision,
            Operation::Refresh {
                refresh_token: Secret(&refresh),
                refresh_binding: &initial.result.refresh_binding,
            },
        )
        .await
        .ok()
        .unwrap(),
    )
    .unwrap();
    assert!(refreshed.result.refresh_token.is_none());
    assert_eq!(refreshed.result.scopes, [READ_SCOPE]);
    let access_token = &refreshed.result.access_token;
    let access_binding = &refreshed.result.access_binding;
    assert!(
        call(
            revision,
            Operation::InboxPage {
                access_token: Secret(access_token),
                access_binding: &initial.result.access_binding,
            }
        )
        .await
        .is_err()
    );
    let inbox: BrokerReply<Inbox> = decode(
        &call(
            revision,
            Operation::InboxPage {
                access_token: Secret(access_token),
                access_binding,
            },
        )
        .await
        .ok()
        .unwrap(),
    )
    .unwrap();
    assert_eq!(inbox.result.mailbox, EMAIL);
    assert!(inbox.result.has_more && inbox.result.valid());
    assert_eq!(
        inbox
            .result
            .messages
            .iter()
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>(),
        ["bb", "aa"]
    );
    assert_eq!(inbox.result.messages[1].subject, "First line");
}

#[cfg(feature = "test-fixtures")]
struct TcpListenerFixture {
    address: SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

#[cfg(feature = "test-fixtures")]
impl TcpListenerFixture {
    async fn start(router: axum::Router) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { address, task }
    }
}

#[cfg(feature = "test-fixtures")]
impl Drop for TcpListenerFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
