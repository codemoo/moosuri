//! One bounded probe shared by setup, CLI and the desktop menu.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Observation {
    Ready,
    Unavailable,
    OtherServer,
}
impl Observation {
    pub(super) fn require_ready(self) -> Result<()> {
        match self {
            Self::Ready => Ok(()),
            Self::Unavailable => Err("setup_service_not_ready_check_moosuri_status".into()),
            Self::OtherServer => Err("setup_port_occupied_by_another_server".into()),
        }
    }
}

pub(super) struct Readiness {
    client: reqwest::Client,
    authority: String,
    base: String,
    identity: String,
}
impl Readiness {
    pub(super) fn new(i: &Installation, s: &State) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .map_err(error)?,
            authority: i
                .origin
                .split_once("://")
                .ok_or("setup_origin_invalid")?
                .1
                .into(),
            base: format!("http://127.0.0.1:{}", s.port),
            identity: uuid::Uuid::parse_str(&i.id).map_err(error)?.to_string(),
        })
    }
    pub(super) async fn observe(&self) -> Observation {
        tokio::time::timeout(Duration::from_secs(2), self.probe())
            .await
            .unwrap_or(Observation::Unavailable)
    }
    async fn probe(&self) -> Observation {
        let Ok(r) = self
            .client
            .get(format!("{}/health/ready", self.base))
            .header(reqwest::header::HOST, &self.authority)
            .send()
            .await
        else {
            return Observation::Unavailable;
        };
        if r.headers()
            .get("x-moosuri-runtime")
            .and_then(|v| v.to_str().ok())
            != Some(self.identity.as_str())
        {
            return Observation::OtherServer;
        }
        if r.status() != reqwest::StatusCode::NO_CONTENT {
            return Observation::Unavailable;
        }
        let Ok(mut r) = self
            .client
            .get(format!("{}/app", self.base))
            .header(reqwest::header::HOST, &self.authority)
            .send()
            .await
        else {
            return Observation::Unavailable;
        };
        if r.status() != reqwest::StatusCode::OK
            || r.headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .is_none_or(|h| h.split(';').next() != Some("text/html"))
        {
            return Observation::Unavailable;
        }
        let mut body = Vec::new();
        loop {
            match r.chunk().await {
                Ok(Some(chunk)) if body.len() + chunk.len() <= 65536 => {
                    body.extend_from_slice(&chunk)
                }
                Ok(None) => {
                    return if String::from_utf8_lossy(&body).contains("manifest.webmanifest") {
                        Observation::Ready
                    } else {
                        Observation::Unavailable
                    };
                }
                _ => return Observation::Unavailable,
            }
        }
    }
    pub(super) async fn wait(&self, budget: Duration) -> Observation {
        tokio::time::timeout(budget, async {
            loop {
                match self.observe().await {
                    Observation::Unavailable => {
                        tokio::time::sleep(Duration::from_millis(250)).await
                    }
                    result => return result,
                }
            }
        })
        .await
        .unwrap_or(Observation::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        http::{StatusCode, header},
        response::IntoResponse,
        routing::get,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    };

    #[tokio::test]
    async fn readiness_requires_the_real_contract_bounds_body_and_total_wait() {
        let mode = Arc::new(AtomicU8::new(0));
        let health = mode.clone();
        let app = mode.clone();
        let router = Router::new()
            .route(
                "/health/ready",
                get(move || {
                    let mode = health.clone();
                    async move {
                        let status = match mode.load(Ordering::Relaxed) {
                            1 => StatusCode::OK,
                            5 => StatusCode::SERVICE_UNAVAILABLE,
                            3 => {
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                StatusCode::NO_CONTENT
                            }
                            _ => StatusCode::NO_CONTENT,
                        };
                        let identity = if mode.load(Ordering::Relaxed) == 4 {
                            "other-installation"
                        } else {
                            "synthetic-installation"
                        };
                        (status, [("x-moosuri-runtime", identity)])
                    }
                }),
            )
            .route(
                "/app",
                get(move || {
                    let mode = app.clone();
                    async move {
                        let body = if mode.load(Ordering::Relaxed) == 2 {
                            format!("manifest.webmanifest{}", "x".repeat(65536))
                        } else {
                            "<html>manifest.webmanifest</html>".into()
                        };
                        ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async {
            axum::serve(listener, router).await.unwrap();
        });
        let probe = Readiness {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            authority: "assistant.example.test".into(),
            base,
            identity: "synthetic-installation".into(),
        };
        assert_eq!(probe.observe().await, Observation::Ready);
        mode.store(1, Ordering::Relaxed);
        assert!(
            probe.observe().await != Observation::Ready,
            "a generic successful endpoint is not readiness"
        );
        mode.store(2, Ordering::Relaxed);
        assert!(
            probe.observe().await != Observation::Ready,
            "an oversized page is never buffered unboundedly"
        );
        mode.store(3, Ordering::Relaxed);
        let began = std::time::Instant::now();
        assert_eq!(
            probe.wait(Duration::from_millis(150)).await,
            Observation::Unavailable
        );
        assert!(began.elapsed() < Duration::from_secs(1));
        mode.store(5, Ordering::Relaxed);
        assert_eq!(probe.observe().await, Observation::Unavailable);
        mode.store(0, Ordering::Relaxed);
        assert_eq!(probe.observe().await, Observation::Ready);
        mode.store(4, Ordering::Relaxed);
        assert_eq!(
            probe.wait(Duration::from_secs(2)).await,
            Observation::OtherServer
        );
        server.abort();
    }
}
