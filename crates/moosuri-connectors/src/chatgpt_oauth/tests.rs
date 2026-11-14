// PUBLIC synthetic signing fixture, created solely for these tests. Never use
// synthetic-test-key.pk8 as a credential. Its private bytes are intentionally public.
use super::*;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};

const MODULUS: &str = "qz_j6Fsqd5usp23J5PcA5s5MYVXGUAO58aWgw8J4RRcEylrPcaR7xTet78qNrRlu5XBlgSGLRDMtoJKeuBz5yE6uALSivvwwyRkpdOHItrQbm84pB8IuPveGpUA_HG-6Tw_SHmJlAxKlRjIwm7EvbiRpvs7j6U07KxFK0R-BhPS-Q5-lUICqCaudBWNyJtOUUFGYoiBBUaWKeVnMNkVFTlpMJdfq4kC1WynkMi-CGm3rNybXHTh5-cybsSDb_Erwi5TDrhksFjU4SDGHbjoQt4mWQhlbdFFEjnTzCFvcK0aOv9fmcDys9GqgBVX6vG-1ITTS9BCNvVGoWGa5dAkLXQ";
fn jwks() -> Value {
    json!({"keys":[{"kty":"RSA","kid":"synthetic-key","alg":"RS256","use":"sig","key_ops":["verify"],"n":MODULUS,"e":"AQAB"}]})
}
fn claims(nonce: &str) -> Value {
    let now = unix_now().unwrap();
    json!({"iss":ISSUER,"sub":"synthetic-subject","aud":"issued-synthetic-client","iat":now,"exp":now+3600,"nonce":nonce})
}
fn sign_with_header(header: Value, payload: &str) -> String {
    let key = RsaKeyPair::from_pkcs8(include_bytes!("synthetic-test-key.pk8")).unwrap();
    let input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap()),
        URL_SAFE_NO_PAD.encode(payload)
    );
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        input.as_bytes(),
        &mut signature,
    )
    .unwrap();
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
}
fn signed(claims: &Value) -> String {
    sign_with_header(
        json!({"alg":"RS256","kid":"synthetic-key","typ":"JWT"}),
        &claims.to_string(),
    )
}
fn discovery() -> Value {
    json!({
        "issuer":ISSUER,"authorization_endpoint":"https://auth.openai.com/api/accounts/authorize",
        "token_endpoint":"https://auth.openai.com/api/accounts/oauth/token",
        "revocation_endpoint":"https://auth.openai.com/api/accounts/oauth/revoke",
        "jwks_uri":"https://auth.openai.com/.well-known/jwks.json",
        "id_token_signing_alg_values_supported":["RS256"],"code_challenge_methods_supported":["S256"],
        "token_endpoint_auth_methods_supported":["none"]
    })
}
fn token(nonce: &str, scopes: &str) -> Value {
    json!({"access_token":"synthetic-access","refresh_token":"synthetic-refresh",
    "id_token":signed(&claims(nonce)),"token_type":"Bearer","expires_in":3600,"scope":scopes,
    "earliest_refresh_at":{"opaque":"not-guessed"}})
}
fn callback(attempt: &AuthorizationAttempt, extra: &[(&str, &str)]) -> String {
    let mut pairs = url::form_urlencoded::Serializer::new(String::new());
    pairs.append_pair("state", &attempt.state);
    for (k, v) in extra {
        pairs.append_pair(k, v);
    }
    format!("{}?{}", attempt.redirect, pairs.finish())
}
fn successful_callback(attempt: &AuthorizationAttempt) -> String {
    callback(
        attempt,
        &[
            ("code", "synthetic-code+&="),
            ("client_id", "issued-synthetic-client"),
        ],
    )
}
struct Request {
    method: String,
    path: String,
    headers: String,
    body: String,
}
struct Response {
    wire: Vec<u8>,
    delay: Duration,
}
impl Response {
    fn json(status: u16, value: Value) -> Self {
        Self::raw(status, "application/json", value.to_string().as_bytes())
    }
    fn raw(status: u16, content_type: &str, body: &[u8]) -> Self {
        let mut wire = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
        wire.extend_from_slice(body);
        Self {
            wire,
            delay: Duration::ZERO,
        }
    }
}
struct Fixture {
    origin: String,
    send: mpsc::UnboundedSender<Response>,
    requests: mpsc::UnboundedReceiver<Request>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (send, mut responses) = mpsc::unbounded_channel::<Response>();
        let (captured, requests) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut bytes = Vec::new();
                let (end, length) = loop {
                    let mut buf = [0u8; 4096];
                    let n = stream.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&bytes[..end]);
                        let length = headers
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|s| s.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if bytes.len() >= end + 4 + length {
                            break (end, length);
                        }
                    }
                };
                let headers = String::from_utf8(bytes[..end].to_vec()).unwrap();
                let mut first = headers.lines().next().unwrap().split(' ');
                let method = first.next().unwrap().to_owned();
                let path = first.next().unwrap().to_owned();
                let body = String::from_utf8(bytes[end + 4..end + 4 + length].to_vec()).unwrap();
                captured
                    .send(Request {
                        method,
                        path,
                        headers,
                        body,
                    })
                    .unwrap();
                let Some(response) = responses.recv().await else {
                    break;
                };
                tokio::time::sleep(response.delay).await;
                let _ = stream.write_all(&response.wire).await;
            }
        });
        Self {
            origin,
            send,
            requests,
            task,
        }
    }
    fn transport(&self) -> transport::Transport {
        transport::Transport {
            fixture: Some(self.origin.clone()),
            deadline: Duration::from_secs(3),
            body_limit: None,
        }
    }
    fn client(&self) -> ChatGptAuthClient {
        ChatGptAuthClient {
            transport: self.transport(),
        }
    }
    fn queue(&self, response: Response) {
        self.send.send(response).unwrap();
    }
    async fn next(&mut self) -> Request {
        tokio::time::timeout(Duration::from_secs(2), self.requests.recv())
            .await
            .unwrap()
            .unwrap()
    }
}
fn form(request: &Request) -> std::collections::BTreeMap<String, String> {
    url::form_urlencoded::parse(request.body.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}
async fn connected(fixture: &Fixture, scopes: &str) -> TokenBundle {
    let client = fixture.client();
    let (attempt, _) = client.begin_registration("synthetic-host", 34567).unwrap();
    let callback = successful_callback(&attempt);
    let nonce = attempt.nonce.to_string();
    let pending = attempt.accept_callback(&callback).unwrap();
    fixture.queue(Response::json(200, token(&nonce, scopes)));
    fixture.queue(Response::json(200, jwks()));
    client.exchange(pending).await.unwrap()
}

#[tokio::test]
async fn discovery_pins_every_advertised_endpoint_and_required_mechanism() {
    let mut fixture = Fixture::new().await;
    fixture.queue(Response::json(200, discovery()));
    assert!(
        ChatGptAuthClient::discover_using(fixture.transport())
            .await
            .is_ok()
    );
    assert_eq!(
        fixture.next().await.path,
        "/.well-known/openid-configuration"
    );
    for (field, value) in [
        ("issuer", json!("https://other.example")),
        (
            "authorization_endpoint",
            json!("https://auth.openai.com/authorize"),
        ),
        (
            "token_endpoint",
            json!("https://auth.openai.com/api/accounts/oauth/token?secret=1"),
        ),
        ("revocation_endpoint", json!("https://other.example/revoke")),
        (
            "jwks_uri",
            json!("http://auth.openai.com/.well-known/jwks.json"),
        ),
        ("id_token_signing_alg_values_supported", json!(["HS256"])),
        ("code_challenge_methods_supported", json!(["plain"])),
        (
            "token_endpoint_auth_methods_supported",
            json!(["client_secret_basic"]),
        ),
    ] {
        let mut doc = discovery();
        doc[field] = value;
        fixture.queue(Response::json(200, doc));
        assert!(
            matches!(
                ChatGptAuthClient::discover_using(fixture.transport()).await,
                Err(AuthError::DestinationDenied)
            ),
            "{field}"
        );
        fixture.next().await;
    }
}
#[tokio::test]
async fn authorization_pkce_callback_and_exact_code_exchange_form() {
    let mut fixture = Fixture::new().await;
    let client = fixture.client();
    let (attempt, url) = client.begin_registration("synthetic-host", 34567).unwrap();
    let parsed = Url::parse(url.for_system_browser()).unwrap();
    assert_eq!(
        (parsed.scheme(), parsed.host_str(), parsed.path()),
        ("https", Some("auth.openai.com"), "/api/accounts/authorize")
    );
    let q: std::collections::BTreeMap<_, _> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert_eq!(q.len(), 11);
    assert_eq!(q["client_id"], DYNAMIC_CLIENT);
    assert_eq!(q["agent_name_hint"], "Moosuri");
    assert_eq!(q["ext_agent_host_id"], "synthetic-host");
    assert_eq!(q["scope"], SCOPES);
    assert_eq!(q["resource"], RESOURCE);
    assert_eq!(q["response_type"], "code");
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(
        q["code_challenge"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(attempt.verifier.as_bytes()))
    );
    assert_eq!(attempt.verifier.len(), 43);
    assert_ne!(*attempt.state, *attempt.nonce);
    let verifier = attempt.verifier.to_string();
    let cb = successful_callback(&attempt);
    let pending = attempt.accept_callback(&cb).unwrap();
    assert_eq!(
        pending.registration().client_id(),
        "issued-synthetic-client"
    );
    fixture.queue(Response::json(200, token(&q["nonce"], SCOPES)));
    fixture.queue(Response::json(200, jwks()));
    let bundle = client.exchange(pending).await.unwrap();
    assert_eq!(bundle.identity().subject(), "synthetic-subject");
    assert_eq!(bundle.plan_grant(), PlanGrant::Enabled);
    assert_eq!(bundle.expires_at_unix() - bundle.received_at_unix(), 3600);
    let request = fixture.next().await;
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", "/api/accounts/oauth/token")
    );
    assert!(
        request
            .headers
            .to_ascii_lowercase()
            .contains("content-type: application/x-www-form-urlencoded")
    );
    assert!(
        !request
            .headers
            .to_ascii_lowercase()
            .contains("authorization:")
    );
    assert_eq!(
        form(&request),
        std::collections::BTreeMap::from([
            ("grant_type".into(), "authorization_code".into()),
            ("client_id".into(), "issued-synthetic-client".into()),
            ("code".into(), "synthetic-code+&=".into()),
            ("code_verifier".into(), verifier),
            (
                "redirect_uri".into(),
                "http://127.0.0.1:34567/auth/callback".into()
            ),
            ("resource".into(), RESOURCE.into())
        ])
    );
    assert_eq!(fixture.next().await.path, "/.well-known/jwks.json");
    bundle.with_secrets_for_sealing(|s| {
        assert_eq!(
            s.earliest_refresh_at,
            Some(&json!({"opaque":"not-guessed"}))
        )
    });
}
#[tokio::test]
async fn callback_rejects_missing_wrong_duplicate_ambiguous_and_error_parameters() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    for case in [
        "missing-state",
        "wrong-state",
        "duplicate-state",
        "duplicate-code",
        "missing-client",
        "dynamic-client",
        "error",
        "code-and-error",
        "bad-encoding",
        "encoded-duplicate",
        "wrong-port",
        "localhost",
        "normalized-ip",
        "fragment",
        "missing-code",
    ] {
        let (attempt, _) = client.begin_registration("synthetic-host", 34567).unwrap();
        let cb = successful_callback(&attempt);
        let cb = match case {
            "missing-state" => format!(
                "{}?code=x&client_id=issued-synthetic-client",
                attempt.redirect
            ),
            "wrong-state" => cb.replace(attempt.state.as_str(), "synthetic-wrong"),
            "duplicate-state" => format!("{cb}&state={}", attempt.state.as_str()),
            "duplicate-code" => format!("{cb}&code=x"),
            "missing-client" => callback(&attempt, &[("code", "x")]),
            "dynamic-client" => callback(&attempt, &[("code", "x"), ("client_id", DYNAMIC_CLIENT)]),
            "error" => callback(
                &attempt,
                &[
                    ("error", "access_denied"),
                    ("error_description", "synthetic-secret"),
                ],
            ),
            "code-and-error" => format!("{cb}&error=access_denied"),
            "bad-encoding" => format!("{cb}&scope=%FF"),
            "encoded-duplicate" => format!("{cb}&%73tate={}", attempt.state.as_str()),
            "wrong-port" => cb.replace(":34567/", ":34568/"),
            "localhost" => cb.replace("127.0.0.1", "localhost"),
            "normalized-ip" => cb.replace("127.0.0.1", "127.1"),
            "fragment" => format!("{cb}#x"),
            "missing-code" => callback(&attempt, &[("client_id", "issued-synthetic-client")]),
            _ => unreachable!(),
        };
        let error = attempt.accept_callback(&cb).err().unwrap();
        assert!(
            !format!("{error:?} {error}").contains("synthetic-secret"),
            "{case}"
        );
    }
}
#[tokio::test]
async fn expired_attempt_and_pending_code_are_consumed_without_network() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let (mut attempt, _) = client.begin_registration("synthetic-host", 34567).unwrap();
    let cb = successful_callback(&attempt);
    attempt.started = Instant::now() - ATTEMPT_TTL;
    assert!(matches!(
        attempt.accept_callback(&cb),
        Err(AuthError::AttemptExpired)
    ));
    let (attempt, _) = client.begin_registration("synthetic-host", 34567).unwrap();
    let cb = successful_callback(&attempt);
    let mut pending = attempt.accept_callback(&cb).unwrap();
    pending.started = Instant::now() - ATTEMPT_TTL;
    assert!(matches!(
        client.exchange(pending).await,
        Err(AuthError::AttemptExpired)
    ));
}
#[tokio::test]
async fn returning_login_pins_client_host_and_subject_and_omits_registration_hint() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let bundle = connected(&fixture, SCOPES).await;
    let (attempt, url) = client.begin_returning(bundle.identity(), 45678).unwrap();
    let parsed = Url::parse(url.for_system_browser()).unwrap();
    let pairs: std::collections::BTreeMap<_, _> = parsed.query_pairs().collect();
    assert_eq!(pairs["client_id"], "issued-synthetic-client");
    assert_eq!(pairs["ext_agent_host_id"], "synthetic-host");
    assert!(!pairs.contains_key("agent_name_hint"));
    let cb = callback(
        &attempt,
        &[("code", "x"), ("client_id", "different-client")],
    );
    assert!(matches!(
        attempt.accept_callback(&cb),
        Err(AuthError::RegistrationMismatch)
    ));
    let (attempt, _) = client.begin_returning(bundle.identity(), 45678).unwrap();
    let cb = successful_callback(&attempt);
    let mut c = claims(&attempt.nonce);
    c["sub"] = json!("different-subject");
    let mut t = token(&attempt.nonce, SCOPES);
    t["id_token"] = json!(signed(&c));
    fixture.queue(Response::json(200, t));
    fixture.queue(Response::json(200, jwks()));
    assert!(matches!(
        client.exchange(attempt.accept_callback(&cb).unwrap()).await,
        Err(AuthError::RegistrationMismatch)
    ));
}
#[test]
fn real_rs256_signed_claims_reject_identity_and_algorithm_confusion() {
    let now = unix_now().unwrap();
    let base = claims("synthetic-nonce");
    let keys = jwks().to_string();
    assert_eq!(
        jwt::verify(
            &signed(&base),
            keys.as_bytes(),
            "issued-synthetic-client",
            "synthetic-nonce",
            true,
            now
        )
        .unwrap(),
        "synthetic-subject"
    );
    for (field, value) in [
        ("iss", json!("https://other.example")),
        ("aud", json!("wrong-client")),
        ("sub", json!("")),
        ("exp", json!(now)),
        ("iat", json!(now + 61)),
        ("nonce", json!("wrong")),
        ("nbf", json!(now + 1)),
        ("azp", json!("wrong")),
        ("aud", json!(["issued-synthetic-client", "other"])),
        (
            "aud",
            json!(["issued-synthetic-client", "issued-synthetic-client"]),
        ),
    ] {
        let mut c = base.clone();
        c[field] = value;
        assert!(
            jwt::verify(
                &signed(&c),
                keys.as_bytes(),
                "issued-synthetic-client",
                "synthetic-nonce",
                true,
                now
            )
            .is_err(),
            "{field}"
        );
    }
    for field in ["iss", "aud", "sub", "exp", "iat", "nonce"] {
        let mut c = base.clone();
        c.as_object_mut().unwrap().remove(field);
        assert!(
            jwt::verify(
                &signed(&c),
                keys.as_bytes(),
                "issued-synthetic-client",
                "synthetic-nonce",
                true,
                now
            )
            .is_err(),
            "missing {field}"
        );
    }
    for header in [
        json!({"alg":"none","kid":"synthetic-key"}),
        json!({"alg":"HS256","kid":"synthetic-key"}),
        json!({"alg":"RS256","kid":"absent"}),
        json!({"alg":"RS256","kid":"synthetic-key","jku":"https://other.example"}),
        json!({"alg":"RS256","kid":"synthetic-key","crit":["unrecognized"]}),
    ] {
        assert!(
            jwt::verify(
                &sign_with_header(header, &base.to_string()),
                keys.as_bytes(),
                "issued-synthetic-client",
                "synthetic-nonce",
                true,
                now
            )
            .is_err()
        );
    }
    let mut tampered = signed(&base).into_bytes();
    let last = tampered.len() - 3;
    tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
    assert!(
        jwt::verify(
            std::str::from_utf8(&tampered).unwrap(),
            keys.as_bytes(),
            "issued-synthetic-client",
            "synthetic-nonce",
            true,
            now
        )
        .is_err()
    );
    let duplicate = format!(
        "{},\"sub\":\"attacker\"}}",
        base.to_string().trim_end_matches('}')
    );
    assert!(
        jwt::verify(
            &sign_with_header(json!({"alg":"RS256","kid":"synthetic-key"}), &duplicate),
            keys.as_bytes(),
            "issued-synthetic-client",
            "synthetic-nonce",
            true,
            now
        )
        .is_err()
    );
}
#[test]
fn real_rs256_jwks_rejects_wrong_key_usage_private_keys_and_ambiguous_ids() {
    let now = unix_now().unwrap();
    let token = signed(&claims("synthetic-nonce"));
    for (field, value) in [
        ("use", json!("enc")),
        ("alg", json!("HS256")),
        ("kty", json!("EC")),
        ("key_ops", json!(["sign"])),
        ("n", json!("AQAB")),
        ("e", json!("AAEAAQ")),
        ("d", json!("synthetic-private")),
        ("dp", json!("synthetic-private")),
    ] {
        let mut keys = jwks();
        keys["keys"][0][field] = value;
        assert!(
            jwt::verify(
                &token,
                keys.to_string().as_bytes(),
                "issued-synthetic-client",
                "synthetic-nonce",
                true,
                now
            )
            .is_err(),
            "{field}"
        );
    }
    let mut keys = jwks();
    let duplicate = keys["keys"][0].clone();
    keys["keys"].as_array_mut().unwrap().push(duplicate);
    assert!(
        jwt::verify(
            &token,
            keys.to_string().as_bytes(),
            "issued-synthetic-client",
            "synthetic-nonce",
            true,
            now
        )
        .is_err()
    );
    let mut c = claims("synthetic-nonce");
    c["aud"] = json!(["issued-synthetic-client", "other"]);
    c["azp"] = json!("issued-synthetic-client");
    assert!(
        jwt::verify(
            &signed(&c),
            jwks().to_string().as_bytes(),
            "issued-synthetic-client",
            "synthetic-nonce",
            true,
            now
        )
        .is_ok()
    );
}
#[tokio::test]
async fn granted_response_scopes_control_plan_and_callback_scope_is_not_evidence() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let (attempt, _) = client.begin_registration("synthetic-host", 34567).unwrap();
    let cb = callback(
        &attempt,
        &[
            ("code", "x"),
            ("client_id", "issued-synthetic-client"),
            ("scope", SCOPES),
        ],
    );
    fixture.queue(Response::json(
        200,
        token(&attempt.nonce, "openid profile email offline_access"),
    ));
    fixture.queue(Response::json(200, jwks()));
    let bundle = client
        .exchange(attempt.accept_callback(&cb).unwrap())
        .await
        .unwrap();
    assert_eq!(bundle.plan_grant(), PlanGrant::SignedInPlanDisabled);
    assert_eq!(bundle.plan_access_token(), Err(AuthError::PlanDisabled));
    let (attempt, _) = client.begin_registration("synthetic-host", 34567).unwrap();
    let cb = successful_callback(&attempt);
    let mut t = token(&attempt.nonce, SCOPES);
    t.as_object_mut().unwrap().remove("scope");
    fixture.queue(Response::json(200, t));
    fixture.queue(Response::json(200, jwks()));
    assert!(matches!(
        client.exchange(attempt.accept_callback(&cb).unwrap()).await,
        Err(AuthError::InvalidResponse)
    ));
}
#[tokio::test]
async fn refresh_omits_scope_replaces_rotating_bundle_and_retains_verified_id_when_omitted() {
    let mut fixture = Fixture::new().await;
    let client = fixture.client();
    let bundle = connected(&fixture, SCOPES).await;
    fixture.next().await;
    fixture.next().await;
    fixture.queue(Response::json(200,json!({"access_token":"synthetic-new-access","refresh_token":"synthetic-new-refresh","token_type":"Bearer","expires_in":42,"earliest_refresh_at":"opaque-time"})));
    let replacement = client.refresh(&bundle).await.unwrap();
    assert_eq!(replacement.identity(), bundle.identity());
    assert_eq!(replacement.plan_grant(), PlanGrant::Enabled);
    assert_eq!(
        replacement.expires_at_unix() - replacement.received_at_unix(),
        42
    );
    replacement.with_secrets_for_sealing(|s| {
        assert_eq!(s.refresh_token, "synthetic-new-refresh");
        assert_eq!(s.access_token, "synthetic-new-access");
        assert_eq!(s.earliest_refresh_at, Some(&json!("opaque-time")));
    });
    bundle.with_secrets_for_sealing(|s| assert_eq!(s.refresh_token, "synthetic-refresh"));
    let request = fixture.next().await;
    assert_eq!(request.path, "/api/accounts/oauth/token");
    assert_eq!(
        form(&request),
        std::collections::BTreeMap::from([
            ("grant_type".into(), "refresh_token".into()),
            ("client_id".into(), "issued-synthetic-client".into()),
            ("refresh_token".into(), "synthetic-refresh".into()),
            ("resource".into(), RESOURCE.into())
        ])
    );
}
#[tokio::test]
async fn refresh_verifies_new_identity_nonce_and_granted_scope() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let bundle = connected(&fixture, SCOPES).await;
    for field in ["sub", "nonce", "aud"] {
        let mut c = claims(&bundle.nonce);
        c[field] = json!("synthetic-wrong");
        let mut t = token(&bundle.nonce, SCOPES);
        t["id_token"] = json!(signed(&c));
        fixture.queue(Response::json(200, t));
        fixture.queue(Response::json(200, jwks()));
        assert!(client.refresh(&bundle).await.is_err(), "{field}");
    }
    let mut t = token(&bundle.nonce, "openid profile");
    t.as_object_mut().unwrap().remove("earliest_refresh_at");
    let mut c = claims(&bundle.nonce);
    c.as_object_mut().unwrap().remove("nonce");
    t["id_token"] = json!(signed(&c));
    fixture.queue(Response::json(200, t));
    fixture.queue(Response::json(200, jwks()));
    let replacement = client.refresh(&bundle).await.unwrap();
    assert_eq!(replacement.plan_grant(), PlanGrant::SignedInPlanDisabled);
    replacement.with_secrets_for_sealing(|s| assert!(s.earliest_refresh_at.is_none()));
}
#[tokio::test]
async fn refresh_errors_are_closed_and_temporary_failures_preserve_the_old_bundle() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let bundle = connected(&fixture, SCOPES).await;
    for (code, reason) in [
        ("invalid_grant", ReauthReason::InvalidGrant),
        ("invalid_refresh_token", ReauthReason::InvalidRefreshToken),
        ("token_expired", ReauthReason::TokenExpired),
        ("refresh_token_expired", ReauthReason::RefreshTokenExpired),
        (
            "refresh_token_invalidated",
            ReauthReason::RefreshTokenInvalidated,
        ),
        ("refresh_token_reused", ReauthReason::RefreshTokenReused),
    ] {
        fixture.queue(Response::json(
            400,
            json!({"error":code,"error_description":"synthetic-secret"}),
        ));
        let error = client.refresh(&bundle).await.err().unwrap();
        assert_eq!(error, AuthError::ReauthorizationRequired(reason));
        assert!(!format!("{error:?} {error}").contains("synthetic-secret"));
    }
    for status in [429, 503] {
        fixture.queue(Response::json(
            status,
            json!({"error":"invalid_grant","error_description":"synthetic-secret"}),
        ));
        assert!(
            matches!(client.refresh(&bundle).await,Err(AuthError::TemporaryHttp(s)) if s==status)
        );
    }
    fixture.queue(Response::json(
        400,
        json!({"error":"synthetic-secret","error_description":"synthetic-access"}),
    ));
    assert!(matches!(
        client.refresh(&bundle).await,
        Err(AuthError::HttpRejected(400))
    ));
    bundle.with_secrets_for_sealing(|s| assert_eq!(s.refresh_token, "synthetic-refresh"));
}
#[tokio::test]
async fn revoke_posts_exact_refresh_token_form_accepts_empty_200_and_reports_remote_failure() {
    let mut fixture = Fixture::new().await;
    let client = fixture.client();
    let bundle = connected(&fixture, SCOPES).await;
    fixture.next().await;
    fixture.next().await;
    fixture.queue(Response::raw(200, "text/plain", b""));
    assert!(client.revoke(&bundle).await.is_ok());
    let request = fixture.next().await;
    assert_eq!(request.path, "/api/accounts/oauth/revoke");
    assert_eq!(
        form(&request),
        std::collections::BTreeMap::from([
            ("token".into(), "synthetic-refresh".into()),
            ("token_type_hint".into(), "refresh_token".into()),
            ("client_id".into(), "issued-synthetic-client".into())
        ])
    );
    fixture.queue(Response::raw(503, "text/plain", b"synthetic-secret"));
    assert_eq!(
        client.revoke(&bundle).await,
        Err(AuthError::TemporaryHttp(503))
    );
}
#[tokio::test]
async fn bounded_http_rejects_redirect_oversize_deadline_partial_body_and_non_json() {
    let mut fixture = Fixture::new().await;
    fixture.queue(Response {wire:b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/token?synthetic-secret\r\nContent-Length: 0\r\n\r\n".to_vec(),delay:Duration::ZERO});
    assert!(matches!(
        ChatGptAuthClient::discover_using(fixture.transport()).await,
        Err(AuthError::HttpRejected(302))
    ));
    fixture.next().await;
    fixture.queue(Response::json(200, discovery()));
    let mut transport = fixture.transport();
    transport.body_limit = Some(8);
    assert!(matches!(
        ChatGptAuthClient::discover_using(transport).await,
        Err(AuthError::LimitExceeded)
    ));
    fixture.next().await;
    fixture.queue(Response {wire:b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\na\r\n0123456789\r\na\r\n0123456789\r\n0\r\n\r\n".to_vec(),delay:Duration::ZERO});
    let mut transport = fixture.transport();
    transport.body_limit = Some(15);
    assert!(matches!(
        ChatGptAuthClient::discover_using(transport).await,
        Err(AuthError::LimitExceeded)
    ));
    fixture.next().await;
    fixture.queue(Response {
        wire: b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n{}"
            .to_vec(),
        delay: Duration::ZERO,
    });
    assert!(matches!(
        ChatGptAuthClient::discover_using(fixture.transport()).await,
        Err(AuthError::Transport)
    ));
    fixture.next().await;
    fixture.queue(Response::raw(200, "text/plain", b"synthetic-secret"));
    assert!(matches!(
        ChatGptAuthClient::discover_using(fixture.transport()).await,
        Err(AuthError::InvalidResponse)
    ));
    fixture.next().await;
    let mut response = Response::json(200, discovery());
    response.delay = Duration::from_millis(150);
    fixture.queue(response);
    let mut transport = fixture.transport();
    transport.deadline = Duration::from_millis(30);
    assert!(matches!(
        ChatGptAuthClient::discover_using(transport).await,
        Err(AuthError::Timeout)
    ));
}
#[test]
fn token_response_rejects_duplicate_authority_fields_invalid_scope_and_non_bearer() {
    for body in [
        r#"{"access_token":"synthetic-a","access_token":"synthetic-b","refresh_token":"synthetic-r","token_type":"Bearer","expires_in":1}"#,
        r#"{"access_token":"synthetic-a","refresh_token":"synthetic-r","token_type":"Basic","expires_in":1}"#,
        r#"{"access_token":"synthetic-a","refresh_token":"synthetic-r","token_type":"Bearer","expires_in":0}"#,
    ] {
        assert!(matches!(
            parse_token(body.as_bytes()),
            Err(AuthError::InvalidResponse)
        ));
    }
    for scopes in [
        "",
        "openid  profile",
        "openid openid",
        "openid\tprofile",
        "scope\\bad",
    ] {
        assert!(parse_scopes(scopes).is_err());
    }
}

#[tokio::test]
async fn complete_deadline_covers_a_stalled_partial_body_and_expired_access_requires_refresh() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = [0u8; 4096];
        let _ = socket.read(&mut request).await.unwrap();
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1000\r\n\r\n{").await.unwrap();
        tokio::time::sleep(Duration::from_secs(1)).await;
    });
    let transport = transport::Transport {
        fixture: Some(origin),
        deadline: Duration::from_millis(30),
        body_limit: None,
    };
    assert!(matches!(
        ChatGptAuthClient::discover_using(transport).await,
        Err(AuthError::Timeout)
    ));
    task.abort();
    let fixture = Fixture::new().await;
    let mut bundle = connected(&fixture, SCOPES).await;
    bundle.expires_at = unix_now().unwrap();
    assert_eq!(bundle.plan_access_token(), Err(AuthError::AccessExpired));
}
#[test]
fn auth_dns_policy_rejects_reserved_mapped_and_non_global_destinations() {
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "169.254.169.254",
        "100.64.0.1",
        "168.63.129.16",
        "192.0.2.1",
        "198.18.0.1",
        "0.0.0.0",
        "224.0.0.1",
        "::1",
        "::ffff:127.0.0.1",
        "fc00::1",
        "fe80::1",
        "2001:db8::1",
        "2001:2::1",
        "2001:20::1",
        "2002::1",
        "3fff::1",
    ] {
        assert!(!transport::public_address(ip.parse().unwrap()), "{ip}");
    }
    for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(transport::public_address(ip.parse().unwrap()), "{ip}");
    }
}

#[tokio::test]
async fn returning_callback_retains_client_when_omitted_and_rejects_supplied_mismatch() {
    let fixture = Fixture::new().await;
    let client = fixture.client();
    let bundle = connected(&fixture, SCOPES).await;
    for include_client in [false, true] {
        let (attempt, _) = client.begin_returning(bundle.identity(), 45678).unwrap();
        let nonce = attempt.nonce.to_string();
        let cb = if include_client {
            successful_callback(&attempt)
        } else {
            callback(&attempt, &[("code", "synthetic-returning-code")])
        };
        let pending = attempt
            .accept_callback(&cb)
            .expect("issued-client callback may omit client_id");
        assert_eq!(
            pending.registration().client_id(),
            bundle.identity().client_id()
        );
        assert_eq!(
            pending.registration().host_id(),
            bundle.identity().host_id()
        );
        fixture.queue(Response::json(200, token(&nonce, SCOPES)));
        fixture.queue(Response::json(200, jwks()));
        assert_eq!(
            client.exchange(pending).await.unwrap().identity(),
            bundle.identity()
        );
    }
    for wrong in ["different-client", "", DYNAMIC_CLIENT] {
        let (attempt, _) = client.begin_returning(bundle.identity(), 45678).unwrap();
        let cb = callback(&attempt, &[("code", "x"), ("client_id", wrong)]);
        assert!(attempt.accept_callback(&cb).is_err());
    }
    let (attempt, _) = client.begin_registration("synthetic-host", 45678).unwrap();
    let cb = callback(&attempt, &[("code", "x")]);
    assert!(matches!(
        attempt.accept_callback(&cb),
        Err(AuthError::RegistrationIncomplete)
    ));
    // Omission preserves the verified subject pin, not just the client mapping.
    let (attempt, _) = client.begin_returning(bundle.identity(), 45678).unwrap();
    let cb = callback(&attempt, &[("code", "x")]);
    let mut c = claims(&attempt.nonce);
    c["sub"] = json!("different-subject");
    let mut response = token(&attempt.nonce, SCOPES);
    response["id_token"] = json!(signed(&c));
    fixture.queue(Response::json(200, response));
    fixture.queue(Response::json(200, jwks()));
    assert!(matches!(
        client.exchange(attempt.accept_callback(&cb).unwrap()).await,
        Err(AuthError::RegistrationMismatch)
    ));
}

#[tokio::test]
async fn failed_or_expired_first_exchange_recovers_with_retained_issued_registration() {
    for expired in [false, true] {
        let mut fixture = Fixture::new().await;
        let client = fixture.client();
        let (first, _) = client.begin_registration("synthetic-host", 34567).unwrap();
        let old_state = first.state.to_string();
        let old_nonce = first.nonce.to_string();
        let old_verifier = first.verifier.to_string();
        let cb = successful_callback(&first);
        let mut pending = first.accept_callback(&cb).unwrap();
        let retained = pending.registration().clone();
        if expired {
            pending.started = Instant::now() - ATTEMPT_TTL;
        } else {
            fixture.queue(Response::json(400, json!({"error":"invalid_grant"})));
        }
        let error = client.exchange(pending).await.err().unwrap();
        assert_eq!(
            error,
            if expired {
                AuthError::AttemptExpired
            } else {
                AuthError::ReauthorizationRequired(ReauthReason::InvalidGrant)
            }
        );
        if !expired {
            fixture.next().await;
        }
        // Recovery cannot replace a selected issued client, even without a verified subject yet.
        let (wrong, _) = client
            .begin_reauthorization(retained.clone(), 45678)
            .unwrap();
        let cb = callback(&wrong, &[("code", "x"), ("client_id", "different-client")]);
        assert!(matches!(
            wrong.accept_callback(&cb),
            Err(AuthError::RegistrationMismatch)
        ));
        let (retry, url) = client.begin_reauthorization(retained, 45678).unwrap();
        assert_ne!(retry.state.as_str(), old_state);
        assert_ne!(retry.nonce.as_str(), old_nonce);
        assert_ne!(retry.verifier.as_str(), old_verifier);
        let parsed = Url::parse(url.for_system_browser()).unwrap();
        let q: std::collections::BTreeMap<_, _> = parsed.query_pairs().collect();
        assert_eq!(q["client_id"], "issued-synthetic-client");
        assert_eq!(q["ext_agent_host_id"], "synthetic-host");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:45678/auth/callback");
        assert!(!q.contains_key("agent_name_hint"));
        assert_eq!(
            q["code_challenge"],
            URL_SAFE_NO_PAD.encode(Sha256::digest(retry.verifier.as_bytes()))
        );
        // Official reauthorization callback omits client_id; retain the exact pending mapping.
        let cb = callback(&retry, &[("code", "synthetic-new-code")]);
        let pending = retry.accept_callback(&cb).unwrap();
        assert_eq!(
            pending.registration().client_id(),
            "issued-synthetic-client"
        );
        fixture.queue(Response::json(200, token(&pending.nonce, SCOPES)));
        fixture.queue(Response::json(200, jwks()));
        let bundle = client.exchange(pending).await.unwrap();
        assert_eq!(bundle.identity().subject(), "synthetic-subject");
        assert_eq!(bundle.identity().client_id(), "issued-synthetic-client");
        assert_eq!(bundle.identity().host_id(), "synthetic-host");
        let request = fixture.next().await;
        assert_eq!(form(&request)["client_id"], "issued-synthetic-client");
        assert_eq!(form(&request)["code"], "synthetic-new-code");
        assert_eq!(
            form(&request)["redirect_uri"],
            "http://127.0.0.1:45678/auth/callback"
        );
    }
}

#[tokio::test]
async fn authenticated_local_seal_reopens_verified_carrier_without_unchecked_identity_import() {
    let fixture = Fixture::new().await;
    let bundle = connected(&fixture, SCOPES).await;
    let sealer = sealed::SessionSealer::new(Zeroizing::new([42; 32]));
    let context = b"synthetic/runtime/owner/binding/revision/generation";
    let cipher = sealer.seal(&bundle, context).unwrap();
    assert!(!cipher.windows(16).any(|w| w == b"synthetic-access"));
    let reopened = sealer.open(&cipher, context).unwrap();
    assert_eq!(reopened.identity(), bundle.identity());
    assert_eq!(
        reopened.plan_access_token().unwrap(),
        bundle.plan_access_token().unwrap()
    );
    assert!(sealer.open(&cipher, b"other owner").is_err());
    assert!(
        sealed::SessionSealer::new(Zeroizing::new([43; 32]))
            .open(&cipher, context)
            .is_err()
    );
    let mut tampered = cipher;
    tampered[25] ^= 1;
    assert!(sealer.open(&tampered, context).is_err());
    let identity = sealer
        .seal_identity(bundle.identity(), b"synthetic identity owner")
        .unwrap();
    assert_eq!(
        sealer
            .open_identity(&identity, b"synthetic identity owner")
            .unwrap(),
        *bundle.identity()
    );
    assert!(sealer.open_identity(&identity, b"other owner").is_err());
}

#[tokio::test]
async fn refresh_http200_followed_by_jwks_temporary_failure_is_rotation_unknown() {
    for status in [429, 503] {
        let fixture = Fixture::new().await;
        let client = fixture.client();
        let bundle = connected(&fixture, SCOPES).await;
        fixture.queue(Response::json(200, token(&bundle.nonce, SCOPES)));
        fixture.queue(Response::json(
            status,
            json!({"error":"temporary","error_description":"synthetic-secret"}),
        ));
        let error = client.refresh(&bundle).await.err().unwrap();
        assert_eq!(error, AuthError::RefreshOutcomeUnknown);
        assert!(!format!("{error:?} {error}").contains("synthetic-secret"));
    }
}

#[tokio::test]
async fn external_handoff_rechecks_fresh_signature_pins_commitment_and_context() {
    use super::handoff::*;
    let f = Fixture::new().await;
    let context = HandoffContext {
        version: 1,
        ticket: "synthetic-ticket".into(),
        runtime: "synthetic-target-runtime".into(),
        tenant: "synthetic-tenant".into(),
        account: "synthetic-account".into(),
        target_host: "urn:uuid:synthetic-target".into(),
        binding: "synthetic-binding".into(),
        expected_revision: 0,
        staged_revision: 1,
    };
    let key = HandoffKey::generate().unwrap();
    let client = f.client();
    let (attempt, _) = client
        .begin_registration("urn:uuid:synthetic-source", 12345)
        .unwrap();
    let committed = attempt.handoff_nonce_commitment(&context.ticket).unwrap();
    let nonce = attempt.nonce.to_string();
    let callback = successful_callback(&attempt);
    let pending = attempt.accept_callback(&callback).unwrap();
    f.queue(Response::json(200, token(&nonce, SCOPES)));
    f.queue(Response::json(200, jwks()));
    let bundle = client.exchange(pending).await.unwrap();
    let envelope = seal_handoff(bundle, &context, &key).unwrap();
    let mut expected = HandoffExpectation {
        context: context.clone(),
        commitment: committed,
        client_id: "issued-synthetic-client".into(),
        subject: Some("synthetic-subject".into()),
        source_host: "urn:uuid:synthetic-source".into(),
        issued_at: unix_now().unwrap(),
    };
    expected.commitment[0] ^= 1;
    assert!(
        client
            .verify_handoff(&envelope, &key, &expected)
            .await
            .is_err()
    );
    expected.commitment = committed;
    expected.context.runtime = "another-runtime".into();
    assert!(
        client
            .verify_handoff(&envelope, &key, &expected)
            .await
            .is_err()
    );
    expected.context = context.clone();
    expected.subject = Some("another-subject".into());
    f.queue(Response::json(200, jwks()));
    assert!(
        client
            .verify_handoff(&envelope, &key, &expected)
            .await
            .is_err()
    );
    expected.subject = Some("synthetic-subject".into());
    f.queue(Response::json(200, jwks()));
    let imported = client
        .verify_handoff(&envelope, &key, &expected)
        .await
        .unwrap()
        .into_target_bundle(&context, &envelope_digest(&envelope))
        .unwrap();
    assert_eq!(imported.identity().host_id(), context.target_host);
    assert_eq!(imported.identity().client_id(), "issued-synthetic-client");
    let wrong = HandoffKey::generate().unwrap();
    assert!(
        client
            .verify_handoff(&envelope, &wrong, &expected)
            .await
            .is_err()
    );
    let mut bad = envelope.clone();
    bad[30] ^= 1;
    assert!(client.verify_handoff(&bad, &key, &expected).await.is_err());
    // A local carrier labelled verified is still independently checked at target.
    for mutate in [
        "issuer",
        "audience",
        "nonce",
        "expired",
        "stale",
        "signature",
    ] {
        let mut b = TokenBundle {
            identity: imported.identity.clone(),
            access: Zeroizing::new("synthetic-access".into()),
            refresh: Zeroizing::new("synthetic-refresh".into()),
            id_token: Zeroizing::new(String::new()),
            nonce: Zeroizing::new(nonce.clone()),
            scopes: imported.scopes.clone(),
            received_at: unix_now().unwrap(),
            expires_at: unix_now().unwrap() + 3600,
            earliest_refresh_at: None,
        };
        b.identity.host_id = "urn:uuid:synthetic-source".into();
        let mut c = claims(&nonce);
        match mutate {
            "issuer" => c["iss"] = json!("https://attacker.example"),
            "audience" => c["aud"] = json!("another-client"),
            "nonce" => c["nonce"] = json!("chosen-after-commit"),
            "expired" => {
                c["iat"] = json!(unix_now().unwrap() - 100);
                c["exp"] = json!(unix_now().unwrap() - 1);
            }
            "stale" => c["iat"] = json!(unix_now().unwrap() - 700),
            _ => (),
        };
        let mut id = signed(&c);
        if mutate == "signature" {
            id.pop();
            id.push('x');
        }
        b.id_token = Zeroizing::new(id);
        let cipher = seal_handoff(b, &context, &key).unwrap();
        f.queue(Response::json(200, jwks()));
        assert!(
            client
                .verify_handoff(&cipher, &key, &expected)
                .await
                .is_err(),
            "{mutate}"
        );
    }
}
