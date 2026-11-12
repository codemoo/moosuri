//! A forward proxy for a dedicated browser network namespace. This is not a
//! substitute for namespace firewall rules: direct browser egress MUST be denied.
use super::{
    BrowserError,
    net::{public_connect, public_url},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

const MAX_CONNECTIONS: usize = 32;
const MAX_HEADER: usize = 16 * 1024;
const MAX_TRANSFER: u64 = 32 * 1024 * 1024;

/// Run only on the private browser/proxy network; do not publish this listener.
pub async fn serve(
    listener: TcpListener,
    cancellation: CancellationToken,
) -> Result<(), BrowserError> {
    serve_observed(listener, cancellation, |_| {}).await
}
async fn serve_observed(
    listener: TcpListener,
    cancellation: CancellationToken,
    observe: impl Fn(usize),
) -> Result<(), BrowserError> {
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! { biased;
            _ = cancellation.cancelled() => { tasks.abort_all(); while tasks.join_next().await.is_some() {} return Ok(()); },
            // Completed JoinSet entries still occupy memory. Reap them before
            // accepting, and bound the entire tracked set, not only live permits.
            _ = tasks.join_next(), if !tasks.is_empty() => {},
            accepted = listener.accept(), if tasks.len()<MAX_CONNECTIONS => {
                let (mut stream, _) = accepted.map_err(|_| BrowserError::Network)?;
                tasks.spawn(async move {
                    let _ = tokio::time::timeout(std::time::Duration::from_secs(30), async {
                        if let Err(error) = relay(&mut stream).await {
                            let status = match error { BrowserError::ForbiddenDestination | BrowserError::InvalidUrl => "403 Forbidden", _ => "502 Bad Gateway" };
                            let _ = stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await;
                        }
                    }).await;
                });
            },
        }
        observe(tasks.len());
    }
}

async fn relay(client: &mut TcpStream) -> Result<(), BrowserError> {
    let mut bytes = Vec::with_capacity(2048);
    loop {
        if bytes.len() == MAX_HEADER {
            return Err(BrowserError::InvalidUrl);
        }
        let byte = client.read_u8().await.map_err(|_| BrowserError::Network)?;
        bytes.push(byte);
        if bytes.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let header = std::str::from_utf8(&bytes).map_err(|_| BrowserError::InvalidUrl)?;
    let (connect, url, request) = parse_request(header)?;
    let mut upstream = public_connect(&url).await?;
    if connect {
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await
            .map_err(|_| BrowserError::Network)?;
    } else {
        upstream
            .write_all(request.as_bytes())
            .await
            .map_err(|_| BrowserError::Network)?;
    }
    let (client_read, mut client_write) = client.split();
    let (server_read, mut server_write) = upstream.split();
    let outgoing = async {
        let result = tokio::io::copy(&mut client_read.take(MAX_TRANSFER), &mut server_write).await;
        let _ = server_write.shutdown().await;
        result
    };
    let incoming = async {
        let result = tokio::io::copy(&mut server_read.take(MAX_TRANSFER), &mut client_write).await;
        let _ = client_write.shutdown().await;
        result
    };
    tokio::try_join!(outgoing, incoming).map_err(|_| BrowserError::Network)?;
    Ok(())
}

fn parse_request(header: &str) -> Result<(bool, url::Url, String), BrowserError> {
    let mut lines = header.split("\r\n");
    let parts: Vec<_> = lines
        .next()
        .ok_or(BrowserError::InvalidUrl)?
        .split(' ')
        .collect();
    if parts.len() != 3 || !matches!(parts[2], "HTTP/1.0" | "HTTP/1.1") {
        return Err(BrowserError::InvalidUrl);
    }
    let connect = parts[0] == "CONNECT";
    if !connect && !matches!(parts[0], "GET" | "HEAD") {
        return Err(BrowserError::ForbiddenDestination);
    }
    let url = if connect {
        // CONNECT target must be a bare authority with exactly the HTTPS port.
        if !parts[1].ends_with(":443") || parts[1].contains(['/', '?', '#', '@', '\\']) {
            return Err(BrowserError::InvalidUrl);
        }
        public_url(&format!("https://{}/", parts[1]))?
    } else {
        let url = public_url(parts[1])?;
        if url.scheme() != "http" {
            return Err(BrowserError::InvalidUrl);
        }
        url
    };
    let mut path = url.path().to_string();
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
        parts[0],
        path,
        url.host_str().ok_or(BrowserError::InvalidUrl)?
    );
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').ok_or(BrowserError::InvalidUrl)?;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.chars().any(|c| c.is_control() && c != '\t')
        {
            return Err(BrowserError::InvalidUrl);
        }
        let name = name.to_ascii_lowercase();
        if name == "transfer-encoding" || (name == "content-length" && value.trim() != "0") {
            return Err(BrowserError::ForbiddenDestination);
        }
        // Drop hop-by-hop/authentication/upgrade headers and all unneeded headers.
        if matches!(
            name.as_str(),
            "accept"
                | "accept-language"
                | "accept-encoding"
                | "user-agent"
                | "range"
                | "if-none-match"
                | "if-modified-since"
        ) {
            request.push_str(&name);
            request.push(':');
            request.push_str(value);
            request.push_str("\r\n");
        }
    }
    request.push_str("\r\n");
    Ok((connect, url, request))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parser_denies_private_methods_and_authority_smuggling() {
        for request in [
            "CONNECT 127.0.0.1:443 HTTP/1.1\r\n\r\n",
            "CONNECT [::ffff:127.0.0.1]:443 HTTP/1.1\r\n\r\n",
            "CONNECT example.com:80 HTTP/1.1\r\n\r\n",
            "CONNECT example.com:443/x HTTP/1.1\r\n\r\n",
            "GET http://169.254.169.254/ HTTP/1.1\r\n\r\n",
            "POST http://example.com/ HTTP/1.1\r\n\r\n",
            "GET http://example.com/ HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n",
        ] {
            assert!(parse_request(request).is_err());
        }
        let (connect, url, _) =
            parse_request("CONNECT example.com:443 HTTP/1.1\r\nHost: ignored\r\n\r\n").unwrap();
        assert!(connect);
        assert_eq!(url.host_str(), Some("example.com"));
        let (_, _, request) = parse_request("GET http://example.com/a?q=b HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: private\r\nAuthorization: private\r\nAccept: text/html\r\n\r\n").unwrap();
        assert!(request.contains("Host: example.com"));
        assert!(!request.contains("private"));
    }
    #[tokio::test]
    async fn actual_proxy_rejects_private_http_and_connect_without_touching_canary() {
        let canary = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = proxy.local_addr().unwrap();
        let cancel = CancellationToken::new();
        let task = tokio::spawn(serve(proxy, cancel.clone()));
        for target in [
            format!(
                "GET http://{}/ HTTP/1.1\r\n\r\n",
                canary.local_addr().unwrap()
            ),
            "CONNECT 127.0.0.1:443 HTTP/1.1\r\n\r\n".into(),
        ] {
            let mut client = TcpStream::connect(address).await.unwrap();
            client.write_all(target.as_bytes()).await.unwrap();
            let mut result = Vec::new();
            client.read_to_end(&mut result).await.unwrap();
            assert!(result.starts_with(b"HTTP/1.1 403"));
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(30), canary.accept())
                .await
                .is_err()
        );
        cancel.cancel();
        task.await.unwrap().unwrap();
    }
    #[tokio::test]
    async fn sustained_accept_load_bounds_all_tracked_tasks() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let high = Arc::new(AtomicUsize::new(0));
        let observed = high.clone();
        let cancel = CancellationToken::new();
        let proxy = tokio::spawn(serve_observed(listener, cancel.clone(), move |n| {
            observed.fetch_max(n, Ordering::SeqCst);
        }));
        let mut clients = tokio::task::JoinSet::new();
        for _ in 0..128 {
            clients.spawn(async move {
                for _ in 0..4 {
                    let mut stream = TcpStream::connect(address).await.unwrap();
                    stream
                        .write_all(b"GET http://127.0.0.1/ HTTP/1.1\r\n\r\n")
                        .await
                        .unwrap();
                    let mut reply = Vec::new();
                    stream.read_to_end(&mut reply).await.unwrap();
                    assert!(reply.starts_with(b"HTTP/1.1 403"));
                }
            });
        }
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while let Some(result) = clients.join_next().await {
                result.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(high.load(Ordering::SeqCst) > 0);
        assert!(high.load(Ordering::SeqCst) <= MAX_CONNECTIONS);
        cancel.cancel();
        proxy.await.unwrap().unwrap();
    }
}
