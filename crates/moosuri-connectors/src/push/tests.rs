use super::*;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use jwt_compact::{Token, UntrustedToken};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn subscription() -> PushSubscription {
    let (_, p256dh) = generate_vapid().unwrap();
    PushSubscription {
        endpoint: "https://fcm.googleapis.com/fcm/send/synthetic".into(),
        p256dh,
        auth: "_ordMnz7uTCmrpBTeUV4Bw".into(), // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
    }
}
#[test]
fn maintained_crypto_signs_expected_vapid_claims_and_encrypts_bounded_payload() {
    let (secret, public) = generate_vapid().unwrap();
    assert_eq!(secret.len(), 32);
    let s = subscription();
    let payload = b"{\"v\":1,\"notification_id\":\"synthetic-opaque-id\"}";
    let request = signed(&s, &secret, "https://push-contact.example.test", payload).unwrap();
    assert_eq!(request.headers()["content-encoding"], "aes128gcm");
    assert_eq!(request.headers()["ttl"], "3600");
    let encrypted = request.body().unwrap().as_bytes().unwrap();
    assert!(encrypted.len() < 1024);
    assert!(!encrypted.windows(payload.len()).any(|v| v == payload));
    let authorization = request.headers()["authorization"].to_str().unwrap();
    let (token, key) = authorization
        .strip_prefix("vapid t=")
        .unwrap()
        .split_once(", k=")
        .unwrap();
    assert_eq!(key, public);
    let key = web_push_native::p256::ecdsa::VerifyingKey::from_sec1_bytes(
        &URL_SAFE_NO_PAD.decode(key).unwrap(),
    )
    .unwrap();
    let untrusted = UntrustedToken::new(token).unwrap();
    let verified: Token<serde_json::Value> = Es256.validator(&key).validate(&untrusted).unwrap();
    assert_eq!(
        verified.claims().custom["aud"],
        "https://fcm.googleapis.com"
    );
    assert_eq!(
        verified.claims().custom["sub"],
        "https://push-contact.example.test"
    );
    let expiry = verified.claims().expiration.unwrap();
    let now = chrono::Utc::now();
    assert!(
        expiry > now + chrono::Duration::minutes(59) && expiry <= now + chrono::Duration::hours(1)
    );
    assert!(
        signed(
            &s,
            &secret,
            "https://push-contact.example.test",
            &vec![0; 513]
        )
        .is_err()
    );
}
#[test]
fn destinations_are_push_service_paths_and_public_addresses_only() {
    for url in [
        "http://fcm.googleapis.com/fcm/send/id",
        "https://fcm.googleapis.com.evil.invalid/fcm/send/id",
        "https://fcm.googleapis.com/admin",
        "https://fcm.googleapis.com/fcm/send/id?secret=x",
        "https://u:p@fcm.googleapis.com/fcm/send/id",
        "https://fcm.googleapis.com:444/fcm/send/id",
        "https://localhost/fcm/send/id",
        "https://169.254.169.254/metadata",
        "https://[::1]/push",
    ] {
        assert!(endpoint(url).is_err());
    }
    for ip in [
        "127.0.0.1",
        "10.0.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "::1",
        "::ffff:127.0.0.1",
        "fc00::1",
        "fe80::1",
        "64:ff9b::7f00:1",
        "2002:7f00:1::",
    ] {
        assert!(!public_address(ip.parse().unwrap()));
    }
    assert!(endpoint("https://updates.push.services.mozilla.com/wpush/v2/opaque").is_ok());
    assert!(endpoint("https://web.push.apple.com/opaqueToken").is_ok());
}
async fn prepared_to(url: &str) -> PreparedPush {
    let (secret, _) = generate_vapid().unwrap();
    let mut request = signed(
        &subscription(),
        &secret,
        "https://push-contact.example.test",
        b"{\"v\":1}",
    )
    .unwrap();
    *request.url_mut() = Url::parse(url).unwrap();
    PreparedPush {
        client: reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(2))
            .pool_max_idle_per_host(0)
            .build()
            .unwrap(),
        request,
    }
}
#[tokio::test]
async fn synthetic_http_receipts_are_not_display_receipts_and_redirects_never_follow() {
    let count = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let canary = Router::new()
        .route(
            "/canary",
            post(|State(count): State<Arc<AtomicUsize>>| async move {
                count.fetch_add(1, Ordering::SeqCst);
                StatusCode::OK
            }),
        )
        .route(
            "/accepted",
            post(|headers: HeaderMap, body: Bytes| async move {
                assert_eq!(headers["content-encoding"], "aes128gcm");
                assert!(body.len() < 1024);
                StatusCode::ACCEPTED
            }),
        )
        .route("/expired", post(|| async { StatusCode::GONE }))
        .route(
            "/backoff",
            post(|| async {
                (
                    [(axum::http::header::RETRY_AFTER, "120")],
                    StatusCode::TOO_MANY_REQUESTS,
                )
            }),
        );
    let destination = base.clone();
    let app = canary
        .route(
            "/redirect",
            post(move || {
                let destination = destination.clone();
                async move {
                    Response::builder()
                        .status(302)
                        .header("location", format!("{destination}/canary"))
                        .body(axum::body::Body::empty())
                        .unwrap()
                        .into_response()
                }
            }),
        )
        .with_state(count.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let stop = CancellationToken::new();
    assert_eq!(
        prepared_to(&format!("{base}/accepted"))
            .await
            .send(&stop)
            .await,
        PushOutcome::Accepted
    );
    assert_eq!(
        prepared_to(&format!("{base}/expired"))
            .await
            .send(&stop)
            .await,
        PushOutcome::Expired
    );
    assert_eq!(
        prepared_to(&format!("{base}/backoff"))
            .await
            .send(&stop)
            .await,
        PushOutcome::Retry(120)
    );
    assert_eq!(
        prepared_to(&format!("{base}/redirect"))
            .await
            .send(&stop)
            .await,
        PushOutcome::Rejected
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let mut invalid = subscription();
    invalid.endpoint = format!("{base}/canary");
    let (secret, _) = generate_vapid().unwrap();
    assert!(
        PreparedPush::prepare(&invalid, &secret, "https://push-contact.example.test", b"x")
            .await
            .is_err()
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    task.abort();
}
