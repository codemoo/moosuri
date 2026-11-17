//! Native TLS and certificate renewal share the server lifetime, with private atomic caches.
use super::*;
use async_trait::async_trait;
use axum_server::accept::Accept;
use futures_util::StreamExt;
use rustls_acme::{AccountCache, AcmeConfig, CertCache};
use std::{future::Future, io, pin::Pin};

pub(super) fn preflight(port: u16) -> Result<()> {
    std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port))
        .map(|_| ())
        .map_err(|e| match e.kind() {
            io::ErrorKind::PermissionDenied => {
                "public_tls_port_permission_use_8443_or_existing_proxy".into()
            }
            _ => "public_tls_port_busy_preserved".into(),
        })
}
pub(super) fn cache_directory(data: &Path) -> Result<PathBuf> {
    private_dir(data)?;
    let path = data
        .parent()
        .ok_or("public_certificate_parent_required")?
        .join(format!(
            ".moosuri-https-{}",
            hash(data.as_os_str().as_encoded_bytes())
        ));
    if !path.exists() {
        new_dir(&path)?;
    }
    private_dir(&path)?;
    Ok(path)
}
#[derive(Clone)]
struct Cache(PathBuf);
impl Cache {
    fn path(&self, kind: &str, subjects: &[String], directory: &str) -> PathBuf {
        self.0.join(format!(
            "{kind}-{}.pem",
            hash(&serde_json::to_vec(&(subjects, directory)).expect("string JSON"))
        ))
    }
    fn load(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        private_dir(&self.0).map_err(io::Error::other)?;
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
            Ok(_) => read(path, 256 * 1024, true)
                .map(Some)
                .map_err(io::Error::other),
        }
    }
    fn store(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        private_dir(&self.0).map_err(io::Error::other)?;
        if bytes.len() > 256 * 1024 {
            return Err(io::Error::other("certificate_cache_limit"));
        }
        if path.exists() {
            self.load(path)?;
        }
        atomic(path, bytes).map_err(io::Error::other)
    }
}
#[async_trait]
impl CertCache for Cache {
    type EC = io::Error;
    async fn load_cert(&self, d: &[String], url: &str) -> io::Result<Option<Vec<u8>>> {
        self.load(&self.path("certificate", d, url))
    }
    async fn store_cert(&self, d: &[String], url: &str, b: &[u8]) -> io::Result<()> {
        self.store(&self.path("certificate", d, url), b)
    }
}
#[async_trait]
impl AccountCache for Cache {
    type EA = io::Error;
    async fn load_account(&self, d: &[String], url: &str) -> io::Result<Option<Vec<u8>>> {
        self.load(&self.path("account", d, url))
    }
    async fn store_account(&self, d: &[String], url: &str, b: &[u8]) -> io::Result<()> {
        self.store(&self.path("account", d, url), b)
    }
}
#[derive(Clone)]
struct BoundedAccept(rustls_acme::axum::AxumAcceptor);
impl<I, S> Accept<I, S> for BoundedAccept
where
    I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Send + 'static,
{
    type Stream = <rustls_acme::axum::AxumAcceptor as Accept<I, S>>::Stream;
    type Service = S;
    type Future = Pin<Box<dyn Future<Output = io::Result<(Self::Stream, S)>> + Send>>;
    fn accept(&self, stream: I, service: S) -> Self::Future {
        let future = self.0.accept(stream, service);
        Box::pin(async move {
            tokio::time::timeout(Duration::from_secs(10), future)
                .await
                .map_err(|_| io::Error::other("tls_handshake_timeout"))?
        })
    }
}
pub struct Prepared {
    listener: std::net::TcpListener,
    state: rustls_acme::AcmeState<io::Error>,
}
impl Prepared {
    pub fn load(path: &Path, origin: &str, data: &Path) -> Result<Self> {
        let p = path.parent().ok_or("public_connection_receipt_invalid")?;
        if path.file_name() != Some(std::ffi::OsStr::new(NAME)) {
            return Err("public_connection_receipt_invalid".into());
        }
        let i = installation(p)?;
        let s = load(p)?;
        let r = Record::load(p)?.ok_or("public_connection_receipt_required")?;
        r.validate(p, &i, &s)?;
        if r.phase != Phase::Connected
            || r.request.origin() != origin
            || i.data_dir != data
            || !matches!(i.network, Network::Public)
        {
            return Err("public_connection_receipt_invalid".into());
        }
        Self::for_request(&r.request, data)
    }
    pub(super) fn for_request(request: &Request, data: &Path) -> Result<Self> {
        let port = request.tls_port.ok_or("public_native_tls_not_configured")?;
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::UNSPECIFIED, port))
            .map_err(|_| "public_tls_listen_failed_check_port_permissions")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "public_tls_listen_failed")?;
        let cache = Cache(cache_directory(data)?);
        let state = AcmeConfig::new([request.host.clone()])
            .cache(cache)
            .directory_lets_encrypt(true)
            .state();
        Ok(Self { listener, state })
    }
    pub fn start(mut self, app: axum::Router) -> Worker {
        let acceptor = BoundedAccept(self.state.axum_acceptor(self.state.default_rustls_config()));
        let handle = axum_server::Handle::new();
        let server_handle = handle.clone();
        let serving = tokio::spawn(async move {
            axum_server::from_tcp(self.listener)
                .map_err(|_| "public_tls_listener_failed")?
                .acceptor(acceptor)
                .handle(server_handle)
                .serve(app.into_make_service_with_connect_info::<SocketAddr>())
                .await
                .map_err(|_| "public_tls_server_failed")
        });
        let renewal = tokio::spawn(async move {
            while let Some(event) = self.state.next().await {
                // Raw ACME errors contain account/order details; keep logs categorical.
                match event {
                    Ok(
                        rustls_acme::EventOk::DeployedCachedCert
                        | rustls_acme::EventOk::DeployedNewCert,
                    ) => eprintln!("public_https_certificate_ready"),
                    Ok(_) => (),
                    Err(_) => {
                        eprintln!("public_https_certificate_pending_check_dns_tcp443_and_cache")
                    }
                }
            }
        });
        Worker {
            handle,
            serving,
            renewal,
        }
    }
}
pub struct Worker {
    handle: axum_server::Handle<std::net::SocketAddr>,
    serving: tokio::task::JoinHandle<std::result::Result<(), &'static str>>,
    renewal: tokio::task::JoinHandle<()>,
}
impl Worker {
    pub fn healthy(&self) -> bool {
        !self.serving.is_finished() && !self.renewal.is_finished()
    }
    pub fn stop(&self) {
        self.handle.graceful_shutdown(Some(Duration::from_secs(3)));
    }
    pub async fn shutdown(self) {
        self.stop();
        self.renewal.abort();
        let mut task = self.serving;
        if tokio::time::timeout(Duration::from_secs(4), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    async fn cached_tls_serves_real_https_and_private_keys_survive_restart() {
        let t = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        fs::set_permissions(t.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let cache = Cache(t.path().into());
        let domain = "assistant.example.test".to_owned();
        let generated = rcgen::generate_simple_self_signed(vec![domain.clone()]).unwrap();
        let bytes = format!(
            "{}{}",
            generated.key_pair.serialize_pem(),
            generated.cert.pem()
        );
        let directory = rustls_acme::acme::LETS_ENCRYPT_PRODUCTION_DIRECTORY;
        cache
            .store_cert(std::slice::from_ref(&domain), directory, bytes.as_bytes())
            .await
            .unwrap();
        let path = cache.path("certificate", std::slice::from_ref(&domain), directory);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        for _ in 0..2 {
            let mut state = AcmeConfig::new([domain.clone()])
                .cache(cache.clone())
                .directory_lets_encrypt(true)
                .state();
            assert!(matches!(
                state.next().await,
                Some(Ok(rustls_acme::EventOk::DeployedCachedCert))
            ));
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            listener.set_nonblocking(true).unwrap();
            let prepared = Prepared { listener, state };
            let app = axum::Router::new().route(
                "/app",
                axum::routing::get(|| async { "synthetic private app" }),
            );
            let worker = prepared.start(app);
            let client = reqwest::Client::builder()
                .no_proxy()
                .add_root_certificate(
                    reqwest::Certificate::from_pem(generated.cert.pem().as_bytes()).unwrap(),
                )
                .resolve(&domain, address)
                .timeout(Duration::from_secs(4))
                .build()
                .unwrap();
            let response = client
                .get(format!("https://{domain}:{}/app", address.port()))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert_eq!(response.text().await.unwrap(), "synthetic private app");
            let plain = reqwest::Client::builder()
                .no_proxy()
                .resolve(&domain, address)
                .timeout(Duration::from_secs(4))
                .build()
                .unwrap();
            assert!(
                plain
                    .get(format!("https://{domain}:{}/app", address.port()))
                    .send()
                    .await
                    .is_err(),
                "untrusted certificates must fail"
            );
            worker.shutdown().await;
            assert_eq!(
                cache
                    .load_cert(std::slice::from_ref(&domain), directory)
                    .await
                    .unwrap()
                    .unwrap(),
                bytes.as_bytes()
            );
        }
        let external = t.path().join("external");
        fs::write(&external, b"preserve").unwrap();
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&external, &path).unwrap();
        assert!(
            cache
                .load_cert(std::slice::from_ref(&domain), directory)
                .await
                .is_err()
        );
        assert!(
            cache
                .store_cert(std::slice::from_ref(&domain), directory, b"replacement")
                .await
                .is_err()
        );
        assert_eq!(fs::read(external).unwrap(), b"preserve");
    }
}
