//! One bounded probe shared by setup, CLI and the desktop menu.
use super::*;

pub(super) struct Readiness {
    client: reqwest::Client,
    authority: String,
    base: String,
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
        })
    }
    pub(super) async fn check(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(2), self.probe())
            .await
            .unwrap_or(false)
    }
    async fn probe(&self) -> bool {
        let Ok(r) = self
            .client
            .get(format!("{}/health/ready", self.base))
            .header(reqwest::header::HOST, &self.authority)
            .send()
            .await
        else {
            return false;
        };
        if r.status() != reqwest::StatusCode::NO_CONTENT {
            return false;
        }
        let Ok(mut r) = self
            .client
            .get(format!("{}/app", self.base))
            .header(reqwest::header::HOST, &self.authority)
            .send()
            .await
        else {
            return false;
        };
        if r.status() != reqwest::StatusCode::OK
            || r.headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .is_none_or(|h| h.split(';').next() != Some("text/html"))
        {
            return false;
        }
        let mut body = Vec::new();
        loop {
            match r.chunk().await {
                Ok(Some(chunk)) if body.len() + chunk.len() <= 65536 => {
                    body.extend_from_slice(&chunk)
                }
                Ok(None) => return String::from_utf8_lossy(&body).contains("manifest.webmanifest"),
                _ => return false,
            }
        }
    }
    pub(super) async fn wait(&self, budget: Duration) -> bool {
        tokio::time::timeout(budget, async {
            loop {
                if self.check().await {
                    return true;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        })
        .await
        .unwrap_or(false)
    }
}

pub(super) async fn ready(i: &Installation, s: &State) -> bool {
    match Readiness::new(i, s) {
        Ok(probe) => probe.check().await,
        Err(_) => false,
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
                        match mode.load(Ordering::Relaxed) {
                            1 => StatusCode::OK,
                            3 => {
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                StatusCode::NO_CONTENT
                            }
                            _ => StatusCode::NO_CONTENT,
                        }
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
        };
        assert!(probe.check().await);
        mode.store(1, Ordering::Relaxed);
        assert!(
            !probe.check().await,
            "a generic successful endpoint is not readiness"
        );
        mode.store(2, Ordering::Relaxed);
        assert!(
            !probe.check().await,
            "an oversized page is never buffered unboundedly"
        );
        mode.store(3, Ordering::Relaxed);
        let began = std::time::Instant::now();
        assert!(!probe.wait(Duration::from_millis(150)).await);
        assert!(began.elapsed() < Duration::from_secs(1));
        server.abort();
    }
}
