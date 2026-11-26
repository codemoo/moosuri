use super::{Binding, Error, display::Display};
use crate::browser::{BrowserConfig, BrowserSession, private_proxy};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub(super) struct Chrome {
    browser: BrowserSession,
    display: Display,
    session: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Page {
    Loading,
    Identifier,
    Password,
    Verification,
    Ready,
    Rejected,
    Mismatch,
    Unknown,
}

impl Chrome {
    pub(super) fn config(tenant: uuid::Uuid) -> Result<BrowserConfig, Error> {
        Ok(BrowserConfig {
            tenant,
            chromium: PathBuf::from("/usr/bin/google-chrome-stable"),
            profile_root: PathBuf::from("/profiles"),
            proxy: private_proxy(
                &std::env::var("MOOSURI_BROWSER_PROXY_IP").map_err(|_| Error::Unavailable)?,
            )
            .map_err(|_| Error::Unavailable)?,
            javascript_enabled: true,
        })
    }
    pub async fn start(binding: &Binding, cancel: CancellationToken) -> Result<Self, Error> {
        let config = Self::config(binding.tenant)?;
        // Check kernel isolation before starting either X or Chrome.
        crate::browser::capsule_health_mode(&config, true).map_err(|_| Error::Unavailable)?;
        let display = Display::start().await?;
        let mut browser =
            BrowserSession::launch_mode(config, cancel, Duration::from_secs(15 * 60), true)
                .await
                .map_err(|_| Error::Unavailable)?;
        let version = browser
            .cdp
            .command("Browser.getVersion", json!({}), None)
            .await
            .map_err(|_| Error::Unavailable)?;
        if version["product"].as_str() != Some("Chrome/158.0.8127.88") {
            return Err(Error::Unavailable);
        }
        let target = browser
            .cdp
            .command("Target.createTarget", json!({"url":"about:blank"}), None)
            .await
            .map_err(|_| Error::Unavailable)?;
        let target = target["targetId"].as_str().ok_or(Error::Unavailable)?;
        let attached = browser
            .cdp
            .command(
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
                None,
            )
            .await
            .map_err(|_| Error::Unavailable)?;
        let session = attached["sessionId"]
            .as_str()
            .ok_or(Error::Unavailable)?
            .to_owned();
        browser
            .cdp
            .command("Page.enable", json!({}), Some(&session))
            .await
            .map_err(|_| Error::Unavailable)?;
        Ok(Self {
            browser,
            display,
            session,
        })
    }

    pub async fn open_login(&mut self) -> Result<(), Error> {
        self.navigate("https://accounts.google.com/ServiceLogin?continue=https%3A%2F%2Fmyaccount.google.com%2F&hl=en").await
    }

    async fn navigate(&mut self, url: &str) -> Result<(), Error> {
        let result = self
            .browser
            .cdp
            .command("Page.navigate", json!({"url":url}), Some(&self.session))
            .await
            .map_err(|_| Error::Unavailable)?;
        if result.get("errorText").is_some() {
            return Err(Error::Unavailable);
        }
        Ok(())
    }

    // Fixed code is evaluated only in the top-level document. It returns an enum,
    // never page text, field values, challenge contents, cookies or account labels.
    async fn evaluate(
        &mut self,
        operation: &str,
        email: &str,
        secret: Option<&str>,
    ) -> Result<String, Error> {
        let args = Zeroizing::new(
            serde_json::to_string(&(operation, email, secret)).map_err(|_| Error::Invalid)?,
        );
        let expression = Zeroizing::new(format!(
            "({})(...{})",
            include_str!("account.js"),
            args.as_str()
        ));
        let result = self
            .browser
            .cdp
            .command(
                "Runtime.evaluate",
                json!({"expression":expression.as_str(),"returnByValue":true,"timeout":2000}),
                Some(&self.session),
            )
            .await
            .map_err(|_| Error::Unavailable)?;
        // CDP's JSON/WebSocket implementations make short-lived copies. Those
        // cannot all be zeroized; the disposable process is the lifetime boundary.
        if result.get("exceptionDetails").is_some() {
            return Err(Error::Unavailable);
        }
        result
            .pointer("/result/value")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or(Error::Unavailable)
    }

    pub async fn page(&mut self, email: &str) -> Result<Page, Error> {
        let value = self.evaluate("observe", email, None).await?;
        serde_json::from_value(json!(value)).map_err(|_| Error::Unavailable)
    }
    pub async fn identifier(&mut self, email: &str) -> Result<(), Error> {
        if self.evaluate("identifier", email, None).await? == "submitted" {
            Ok(())
        } else {
            Err(Error::NeedsHuman)
        }
    }
    pub async fn password(&mut self, email: &str, secret: &str) -> Result<(), Error> {
        if self.evaluate("password", email, Some(secret)).await? == "submitted" {
            Ok(())
        } else {
            Err(Error::NeedsHuman)
        }
    }
    pub async fn consent(
        &mut self,
        request: super::consent::Consent,
        binding: &Binding,
        email: &str,
    ) -> Result<(), Error> {
        let (url, callback, state) = request.validate(binding, email)?;
        self.navigate(&url).await?;
        let args = serde_json::to_string(&(email, callback, state)).map_err(|_| Error::Invalid)?;
        let expression = format!("({})(...{})", include_str!("consent.js"), args);
        for _ in 0..150 {
            let result = self
                .browser
                .cdp
                .command(
                    "Runtime.evaluate",
                    json!({"expression":expression,"returnByValue":true,"timeout":2000}),
                    Some(&self.session),
                )
                .await
                .map_err(|_| Error::Unavailable)?;
            if result.get("exceptionDetails").is_some() {
                return Err(Error::Unavailable);
            }
            match result.pointer("/result/value").and_then(|v| v.as_str()) {
                Some("returned") => return Ok(()),
                Some("blocked") => return Err(Error::Blocked),
                Some("needs_human") => return Err(Error::NeedsHuman),
                Some("waiting") => {}
                _ => return Err(Error::Unavailable),
            }
            tokio::time::sleep(Duration::from_millis(600)).await;
        }
        Err(Error::NeedsHuman)
    }
    pub async fn shutdown(mut self) {
        self.browser.shutdown().await;
        self.display.stop().await;
    }
}
