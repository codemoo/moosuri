//! Read-only DNS and an ephemeral HTTPS challenge prove that this domain reaches this server.
use super::*;
use axum::{Router, http::header, routing::get};

pub(super) async fn addresses(host: &str) -> Result<Vec<SocketAddr>> {
    #[cfg(feature = "test-fixtures")]
    if let Ok(value) = std::env::var("MOOSURI_PUBLIC_TEST_ADDRESS") {
        let address: SocketAddr = value.parse().map_err(|_| "public_fixture_invalid")?;
        if !address.ip().is_loopback() {
            return Err("public_fixture_invalid".into());
        }
        return Ok(vec![address]);
    }
    let found = tokio::time::timeout(Duration::from_secs(4), tokio::net::lookup_host((host, 443)))
        .await
        .map_err(|_| "public_dns_lookup_timeout")?
        .map_err(|_| "public_dns_not_registered")?;
    let addresses: Vec<_> = found.take(33).collect();
    if addresses.is_empty()
        || addresses.len() > 32
        || addresses
            .iter()
            .any(|a| !moosuri_connectors::web::public_address(a.ip()))
    {
        return Err("public_dns_must_resolve_to_public_server".into());
    }
    Ok(addresses)
}
fn client(host: &str, addresses: &[SocketAddr]) -> Result<reqwest::Client> {
    #[allow(unused_mut)]
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .resolve_to_addrs(host, addresses);
    #[cfg(feature = "test-fixtures")]
    if let Ok(path) = std::env::var("MOOSURI_PUBLIC_TEST_CA") {
        builder = builder.add_root_certificate(
            reqwest::Certificate::from_pem(&read(Path::new(&path), 65536, true)?)
                .map_err(|_| "public_fixture_invalid")?,
        );
    }
    builder
        .build()
        .map_err(|_| "public_probe_unavailable".into())
}
async fn fetch(client: &reqwest::Client, url: &str, expected: &str) -> Result<bool> {
    let Ok(mut response) = client.get(url).send().await else {
        return Ok(false);
    };
    if response.status() != reqwest::StatusCode::OK {
        return Err("public_domain_reaches_another_or_unready_server".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "public_connection_probe_failed")?
    {
        if body.len() + chunk.len() > 128 {
            return Err("public_domain_reaches_another_server".into());
        }
        body.extend_from_slice(&chunk);
    }
    if body != expected.as_bytes() {
        return Err("public_domain_reaches_another_server".into());
    }
    Ok(true)
}
pub(super) async fn verify(request: &Request, data: &Path, backend: u16) -> Result<()> {
    let addresses = addresses(&request.host).await?;
    if request.tls_port.is_some() && addresses.iter().any(SocketAddr::is_ipv6) {
        return Err("public_native_tls_requires_ipv4_dns_use_existing_proxy_for_ipv6".into());
    }
    let client = client(&request.host, &addresses)?;
    // Independent random path and response prevent an unrelated echo/proxy from passing.
    let path = format!("/.well-known/moosuri-connection/{}", uuid::Uuid::new_v4());
    let expected = uuid::Uuid::new_v4().to_string();
    let body = expected.clone();
    let router = Router::new().route(
        &path,
        get(move || {
            let body = body.clone();
            async move { ([(header::CACHE_CONTROL, "no-store")], body) }
        }),
    );
    let (tls, http) = if request.tls_port.is_some() {
        (
            Some(tls::Prepared::for_request(request, data)?.start(router)),
            None,
        )
    } else {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, backend))
            .await
            .map_err(|_| "public_proxy_backend_port_busy")?;
        (
            None,
            Some(tokio::spawn(
                async move { axum::serve(listener, router).await },
            )),
        )
    };
    println!(
        "Checking registered DNS and this server's HTTPS route (up to120s). No assistant data is served before this check passes."
    );
    #[allow(unused_mut)]
    let mut budget = Duration::from_secs(120);
    #[cfg(feature = "test-fixtures")]
    if std::env::var_os("MOOSURI_PUBLIC_TEST_ADDRESS").is_some() {
        budget = Duration::from_secs(5);
    }
    let url = format!("{}{path}", request.origin());
    let result = tokio::time::timeout(budget, async {
        loop {
            if fetch(&client, &url, &expected).await? {
                return Ok(());
            }
            if tls.as_ref().is_some_and(|w| !w.healthy())
                || http.as_ref().is_some_and(|w| w.is_finished())
            {
                return Err("public_verification_listener_stopped".into());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    })
    .await
    .unwrap_or_else(|_| Err("public_connection_unverified_check_dns_tcp443_and_tls".into()));
    if let Some(w) = tls {
        w.shutdown().await;
    }
    if let Some(w) = http {
        w.abort();
        let _ = w.await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn unrelated_or_reflecting_server_cannot_pass_domain_check() {
        let app =
            Router::new().fallback(|uri: axum::http::Uri| async move { uri.path().to_owned() });
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = l.local_addr().unwrap();
        let server = tokio::spawn(async {
            axum::serve(l, app).await.unwrap();
        });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        assert!(
            fetch(
                &client,
                &format!("http://{address}/random-path"),
                "independent-random-response"
            )
            .await
            .is_err()
        );
        server.abort();
    }
}
