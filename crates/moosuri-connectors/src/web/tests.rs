use super::*;
use axum::{
    Router,
    body::Body,
    http::{Request, Response as HttpResponse, StatusCode},
    routing::get,
};
use futures_util::StreamExt;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Fixture {
    endpoint: Url,
    task: tokio::task::JoinHandle<()>,
    requests: Arc<AtomicUsize>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(
        status: StatusCode,
        headers: Vec<(&'static str, String)>,
        payload: String,
    ) -> Self {
        let requests = Arc::new(AtomicUsize::new(0));
        let count = requests.clone();
        let app = Router::new().fallback(get(move |request: Request<Body>| {
            let headers = headers.clone();
            let payload = payload.clone();
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                assert!(request.headers().get("authorization").is_none());
                assert!(request.headers().get("cookie").is_none());
                let mut response = HttpResponse::builder().status(status);
                for (key, value) in headers {
                    response = response.header(key, value);
                }
                response.body(Body::from(payload)).unwrap()
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            endpoint,
            task,
            requests,
        }
    }
    fn client(&self, key: Option<&str>) -> WebClient {
        let mut client = WebClient::new(WebConfig {
            brave_api_key: key.map(str::to_owned),
        })
        .unwrap();
        // This exception exists only in this cfg(test) constructor, is exact-origin,
        // and cannot authorize an arbitrary redirect or production URL.
        client.test_endpoint = Some(self.endpoint.clone());
        client
    }
    async fn read(&self) -> Result<WebPage, WebError> {
        let client = self.client(None);
        client
            .bounded(
                &CancellationToken::new(),
                client.read_inner(self.endpoint.clone()),
            )
            .await
    }
}

#[test]
fn public_address_policy_rejects_nonpublic_and_embedded_destinations() {
    for raw in [
        "168.63.129.16",
        "::ffff:168.63.129.16",
        "0.0.0.0",
        "0.1.2.3",
        "10.1.2.3",
        "127.1.2.3",
        "169.254.169.254",
        "100.100.100.200",
        "100.64.0.1",
        "172.16.0.1",
        "192.168.1.1",
        "192.0.2.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "240.0.0.1",
        "255.255.255.255",
        "::",
        "::1",
        "fe80::1",
        "fd00:ec2::254",
        "ff02::1",
        "::ffff:127.0.0.1",
        "::ffff:169.254.169.254",
        "64:ff9b::7f00:1",
        "2001:db8::1",
        "2002:7f00:1::",
        "3fff::1",
    ] {
        assert!(!public_address(raw.parse().unwrap()), "{raw}");
    }
    for raw in [
        "8.8.8.8",
        "1.1.1.1",
        "2606:4700:4700::1111",
        "::ffff:8.8.8.8",
    ] {
        assert!(public_address(raw.parse().unwrap()), "{raw}");
    }
    assert_eq!(
        validate_addresses(
            &[
                "8.8.8.8:443".parse().unwrap(),
                "127.0.0.1:443".parse().unwrap()
            ],
            false
        ),
        Err(WebError::ForbiddenDestination)
    );
}

#[test]
fn url_policy_rejects_credentials_schemes_ports_and_local_names() {
    for raw in [
        "file:///etc/passwd",
        "javascript:alert(1)",
        "data:text/plain,hello",
        "ftp://example.com",
        "https://u:p@example.com",
        "https://@example.com",
        "https://example.com:8443/",
        "https://example.com/\n",
        "https://localhost/",
        "http://a.local/",
        "http://example.internal/",
        "http://singlelabel/",
        "https://a.home.arpa/",
    ] {
        assert!(parse_url(raw).is_err(), "{raw}");
    }
    assert_eq!(
        parse_url("https://example.com/path#fragment")
            .unwrap()
            .as_str(),
        "https://example.com/path"
    );
    assert!(parse_url(&format!("https://example.com/{}", "x".repeat(MAX_URL))).is_err());
}

#[test]
fn html_extracts_text_entities_title_and_omits_nonvisible_content() {
    let (title, text, truncated) = extract_html(
        "<title>A &amp; B</title><style>bad1</style><script>bad2</script><main><h1>Hello</h1><p>World &amp; friends</p><div hidden>bad3</div><span aria-hidden='true'>visible</span><div style='display: none'>bad5</div><template>bad6</template><noscript>bad7</noscript><p>끝</p></main>",
    );
    assert_eq!(title, "A & B");
    assert_eq!(text, "Hello World & friends visible 끝");
    assert!(!truncated);
    let (_, text, truncated) = extract_html_with_cap(&format!("<p>{}</p>", "가".repeat(5000)), 128);
    assert!(truncated);
    assert!(text.len() <= 128);
    assert!(!text.is_empty());
}

#[tokio::test]
async fn actual_http_extracts_no_script_and_is_bounded() {
    let fixture = Fixture::new(
        StatusCode::OK,
        vec![("content-type", "text/html".into())],
        format!(
            "<title>Fixture</title><script>secret script</script><p>{}</p>",
            "\"가\\".repeat(5000)
        ),
    )
    .await;
    let page = fixture.read().await.unwrap();
    assert_eq!(page.title, "Fixture");
    assert!(!page.text.contains("secret"));
    assert!(page.truncated);
    assert!(page.warning.contains("no_javascript"));
    assert!(serde_json::to_vec(&page).unwrap().len() <= MAX_RESULT);
    assert_eq!(fixture.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn initial_and_redirect_private_canaries_receive_no_request() {
    let canary = Fixture::new(
        StatusCode::OK,
        vec![("content-type", "text/plain".into())],
        "canary".into(),
    )
    .await;
    let production = WebClient::new(WebConfig::default()).unwrap();
    assert!(
        production
            .read(canary.endpoint.as_str(), &CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(
        production
            .read("http://127.0.0.1/", &CancellationToken::new())
            .await
            .unwrap_err(),
        WebError::ForbiddenDestination
    );
    let redirect = Fixture::new(
        StatusCode::FOUND,
        vec![("location", canary.endpoint.to_string())],
        String::new(),
    )
    .await;
    assert!(redirect.read().await.is_err());
    assert_eq!(redirect.requests.load(Ordering::SeqCst), 1);
    assert_eq!(canary.requests.load(Ordering::SeqCst), 0);
    let metadata = Fixture::new(
        StatusCode::FOUND,
        vec![(
            "location",
            "http://169.254.169.254/latest/meta-data/".into(),
        )],
        String::new(),
    )
    .await;
    assert_eq!(
        metadata.read().await.unwrap_err(),
        WebError::ForbiddenDestination
    );
}

#[tokio::test]
async fn rejects_oversized_encoded_binary_and_bad_status_responses() {
    for (status, headers, payload, error) in [
        (
            StatusCode::OK,
            vec![("content-type", "text/plain".into())],
            "x".repeat(MAX_BODY + 1),
            WebError::TooLarge,
        ),
        (
            StatusCode::OK,
            vec![
                ("content-type", "text/plain".into()),
                ("content-encoding", "gzip".into()),
            ],
            "opaque".into(),
            WebError::UnsupportedContent,
        ),
        (
            StatusCode::OK,
            vec![("content-type", "application/octet-stream".into())],
            "binary".into(),
            WebError::UnsupportedContent,
        ),
        (
            StatusCode::FORBIDDEN,
            vec![],
            "sensitive provider error".into(),
            WebError::Status,
        ),
    ] {
        let fixture = Fixture::new(status, headers, payload).await;
        assert_eq!(fixture.read().await.unwrap_err(), error);
    }
}

#[tokio::test]
async fn streamed_body_limit_and_inflight_cancellation_are_enforced() {
    let reached = Arc::new(tokio::sync::Notify::new());
    let notify = reached.clone();
    let app = Router::new()
        .route(
            "/large",
            get(|| async {
                let chunks = futures_util::stream::iter(
                    (0..3).map(|_| Ok::<_, std::convert::Infallible>(vec![b'x'; MAX_BODY / 2])),
                );
                HttpResponse::builder()
                    .header("content-type", "text/plain")
                    .body(Body::from_stream(chunks))
                    .unwrap()
            }),
        )
        .route(
            "/held",
            get(move || {
                let notify = notify.clone();
                async move {
                    notify.notify_one();
                    let chunks = futures_util::stream::once(async {
                        Ok::<_, std::convert::Infallible>("first")
                    })
                    .chain(futures_util::stream::pending());
                    HttpResponse::builder()
                        .header("content-type", "text/plain")
                        .body(Body::from_stream(chunks))
                        .unwrap()
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
    let fixture = Fixture {
        endpoint,
        task: tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        }),
        requests: Arc::new(AtomicUsize::new(0)),
    };
    let client = fixture.client(None);
    assert_eq!(
        client
            .read_inner(fixture.endpoint.join("large").unwrap())
            .await
            .unwrap_err(),
        WebError::TooLarge
    );
    let cancel = CancellationToken::new();
    let control = cancel.clone();
    let url = fixture.endpoint.join("held").unwrap();
    let pending =
        tokio::spawn(async move { client.bounded(&cancel, client.read_inner(url)).await });
    tokio::time::timeout(Duration::from_secs(1), reached.notified())
        .await
        .unwrap();
    control.cancel();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err(),
        WebError::Cancelled
    );
}

#[tokio::test]
async fn search_uses_actual_brave_shape_and_reports_missing_key() {
    let app = Router::new().fallback(get(|request: Request<Body>| async move {
        assert_eq!(request.headers().get("x-subscription-token").unwrap(), "synthetic-key");
        let query = request.uri().query().unwrap();
        assert!(query.contains("q=rust+research")); assert!(query.contains("count=5"));
        assert!(request.headers().get("authorization").is_none());
        HttpResponse::builder().header("content-type", "application/json").body(Body::from(r#"{"type":"search","web":{"results":[{"title":"Rust","url":"https://www.rust-lang.org/","description":"A <strong>real</strong> result"}]}}"#)).unwrap()
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = WebClient::new(WebConfig {
        brave_api_key: Some("synthetic-key".into()),
    })
    .unwrap();
    client.test_endpoint =
        Some(Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let result = client
        .search("rust research", &CancellationToken::new())
        .await
        .unwrap();
    task.abort();
    assert_eq!(result.provider, "brave");
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].description, "A real result");
    assert_eq!(
        WebClient::new(WebConfig::default())
            .unwrap()
            .search("rust", &CancellationToken::new())
            .await
            .unwrap_err(),
        WebError::Unavailable
    );
}

#[tokio::test]
async fn cancellation_and_provider_errors_do_not_expose_secret_or_remote_body() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    let client = WebClient::new(WebConfig::default()).unwrap();
    assert_eq!(
        client
            .read("https://example.com", &cancel)
            .await
            .unwrap_err(),
        WebError::Cancelled
    );
    let fixture = Fixture::new(
        StatusCode::OK,
        vec![("content-type", "application/json".into())],
        "synthetic-key private raw response".into(),
    )
    .await;
    let error = fixture
        .client(Some("synthetic-key"))
        .search("test", &CancellationToken::new())
        .await
        .unwrap_err();
    assert_eq!(error, WebError::InvalidResponse);
    assert!(!format!("{error:?} {error}").contains("synthetic-key"));
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        trigger.cancel();
    });
    assert_eq!(
        client
            .bounded(&cancel, std::future::pending::<Result<(), WebError>>())
            .await
            .unwrap_err(),
        WebError::Cancelled
    );
}

#[test]
fn aria_hidden_visible_negation_is_preserved() {
    let (_, text, truncated) =
        extract_html("<p><span aria-hidden='true'>not </span>refundable</p>");
    assert_eq!(text, "not refundable");
    assert!(!truncated);
}
