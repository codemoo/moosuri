use super::{
    Binding, Error,
    display::Display,
    sdk::{Callback, Email, LoginRequest},
};
use crate::browser::{BrowserConfig, BrowserSession, private_proxy};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

const CDP_STEP: Duration = Duration::from_secs(5);
/// Fixed, nonsecret document served for the intercepted callback navigation.
const BLANK_PAGE: &str =
    "<!doctype html><meta charset=utf-8><title>Done</title><p>You can close this window.</p>";

/// Only `Chrome::console` drives this, so the sticky gate is already held.
impl super::console::Port for Chrome {
    async fn navigate(&mut self, url: &str) -> Result<(), Error> {
        self.console_navigate(url).await
    }
    async fn eval(
        &mut self,
        spec: &super::console::Spec,
        call: &super::console::Call,
    ) -> Result<String, Error> {
        let expression = super::console::expression(spec, call)?;
        self.console_evaluate(&expression).await
    }
    async fn pause(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

/// Only `Chrome::client` drives this, so the sticky gate is already held. The
/// fixed expression is the only code ever evaluated; no selector, URL or script
/// comes from a caller, and no network interception or download is involved.
impl super::client::Port for Chrome {
    async fn navigate(&mut self, url: &str) -> Result<(), Error> {
        if !url.starts_with("https://console.cloud.google.com/auth/clients") {
            return Err(Error::Invalid);
        }
        self.console_navigate(url).await
    }
    async fn eval(
        &mut self,
        spec: &super::client::Spec,
        call: &super::client::Call<'_>,
    ) -> Result<Zeroizing<String>, Error> {
        let expression = Zeroizing::new(super::client::expression(spec, call)?);
        self.client_evaluate(&expression).await
    }
    async fn pause(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

pub(super) struct Chrome {
    browser: BrowserSession,
    display: Display,
    target: String,
    session: String,
    gate: Gate,
}

/// Sticky use state. Only `authorize` leaves `Clean`; only a confirmed target replacement
/// success returns to it. A dropped authorize future stays `InUse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gate {
    Clean,
    InUse,
    Tainted,
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
            BrowserSession::launch_mode(config, cancel, super::bootstrap::ATTEMPT_LIFETIME, true)
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
        let target = target["targetId"]
            .as_str()
            .ok_or(Error::Unavailable)?
            .to_owned();
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
            target,
            session,
            gate: Gate::Clean,
        })
    }

    pub async fn open_login(&mut self) -> Result<(), Error> {
        self.navigate("https://accounts.google.com/ServiceLogin?continue=https%3A%2F%2Fmyaccount.google.com%2F&hl=en").await
    }

    async fn navigate(&mut self, url: &str) -> Result<(), Error> {
        if self.gate != Gate::Clean {
            return Err(Error::Unavailable);
        }
        self.navigate_unchecked(url).await
    }

    async fn navigate_unchecked(&mut self, url: &str) -> Result<(), Error> {
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
        if self.gate != Gate::Clean {
            return Err(Error::Unavailable);
        }
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
    /// SDK login in this same logged-in Chrome. Only the exact loopback callback of
    /// `request` is intercepted (never reaching the proxy or the network): a
    /// top-level GET `Document` request in this session's top frame whose URL passes
    /// `LoginRequest::accept_callback`. It is fulfilled with a fixed blank page and
    /// returned as an opaque `Callback` for `Sdk::deliver_callback`. Everything else
    /// paused is aborted. On success the intercepted tab is closed while Fetch
    /// remains enabled, and a new tab in the same browser/profile is opened. Fetch
    /// is never disabled, so no paused request can be resumed by cleanup. On error,
    /// timeout or cancellation Chrome is tainted (best-effort `Browser.close`). A
    /// dropped future leaves the sticky `InUse` gate. In both cases later calls fail
    /// and the caller must `shutdown()` it. Success of the login is
    /// proven only by the SDK, not here.
    /// Errors: `NeedsHuman` (device confirmation/unknown page/timeout), `Blocked`
    /// (access blocked, bad or error callback, wrong account), `Unavailable`.
    pub async fn authorize(
        &mut self,
        request: &LoginRequest,
        email: &Email,
        cancel: &CancellationToken,
        limit: Duration,
    ) -> Result<Callback, Error> {
        // Sticky gate: a dropped future leaves `InUse`, so tainted Chrome is never reused.
        if self.gate != Gate::Clean {
            return Err(Error::Unavailable);
        }
        let pattern = request.fetch_pattern()?; // Fetch is not enabled yet: still clean.
        self.gate = Gate::InUse;
        // Nothing can be paused while Fetch is disabled; discard any stale queue.
        drop(self.browser.cdp.take_paused());
        let outcome = {
            let inner = self.authorize_inner(request, email, &pattern);
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(Error::Unavailable),
                r = tokio::time::timeout(limit, inner) => r.unwrap_or(Err(Error::NeedsHuman)),
            }
        };
        match outcome {
            Ok(callback) => match self.release_clean().await {
                Ok(()) => {
                    self.gate = Gate::Clean;
                    Ok(callback)
                }
                Err(error) => {
                    self.taint().await;
                    Err(error)
                }
            },
            Err(error) => {
                // Never disable Fetch with possibly unaccounted paused requests:
                // destroy the browser instead. The caller must call `shutdown`.
                self.taint().await;
                Err(error)
            }
        }
    }

    /// Fixed Console branding/scope operation in this same authenticated Chrome.
    /// Sticky `InUse` while running (a dropped future leaves it unusable); any error,
    /// timeout or cancellation taints Chrome so no mutating click can be repeated.
    /// The caller must still `shutdown()`.
    pub(super) async fn console(
        &mut self,
        op: super::console::Op,
        spec: &super::console::Spec,
        cancel: &CancellationToken,
    ) -> Result<super::bootstrap::Reply, Error> {
        if self.gate != Gate::Clean {
            return Err(Error::Unavailable);
        }
        self.gate = Gate::InUse;
        match super::console::run(self, op, spec, cancel).await {
            Ok(reply) => {
                self.gate = Gate::Clean;
                Ok(reply)
            }
            Err(error) => {
                self.taint().await;
                Err(error)
            }
        }
    }

    /// Fixed OAuth client reconcile/creation in this same authenticated Chrome.
    /// Same gate discipline as `console`: any error, timeout or cancellation taints
    /// Chrome, so a possibly-performed `Create` click can never be repeated. The
    /// outcome may contain the one-time client secret (private, redacted Debug).
    pub(super) async fn client(
        &mut self,
        op: super::client::Op,
        spec: &super::client::Spec,
        cancel: &CancellationToken,
    ) -> Result<super::client::Outcome, Error> {
        if self.gate != Gate::Clean {
            return Err(Error::Unavailable);
        }
        self.gate = Gate::InUse;
        match super::client::run(self, op, spec, cancel).await {
            Ok(outcome) => {
                self.gate = Gate::Clean;
                Ok(outcome)
            }
            Err(error) => {
                self.taint().await;
                Err(error)
            }
        }
    }

    /// True while this Chrome may be used (not tainted, no authorize in flight).
    #[cfg(test)]
    pub(super) fn is_clean(&self) -> bool {
        self.gate == Gate::Clean
    }

    /// Marks Chrome unusable and asks it to exit (best effort). Interception stays
    /// enabled so nothing paused can ever be released to the proxy.
    async fn taint(&mut self) {
        self.gate = Gate::Tainted;
        let _ = self
            .browser
            .cdp
            .command_timeout("Browser.close", json!({}), None, CDP_STEP)
            .await;
    }

    async fn authorize_inner(
        &mut self,
        request: &LoginRequest,
        email: &Email,
        pattern: &str,
    ) -> Result<Callback, Error> {
        let session = self.session.clone();
        let tree = self
            .browser
            .cdp
            .command_timeout("Page.getFrameTree", json!({}), Some(&session), CDP_STEP)
            .await
            .map_err(|_| Error::Unavailable)?;
        let frame = tree
            .pointer("/frameTree/frame/id")
            .and_then(|v| v.as_str())
            .ok_or(Error::Unavailable)?
            .to_owned();
        self.browser
            .cdp
            .command_timeout(
                "Fetch.enable",
                json!({"patterns":[{"urlPattern":pattern,"resourceType":"Document","requestStage":"Request"}]}),
                Some(&session),
                CDP_STEP,
            )
            .await
            .map_err(|_| Error::Unavailable)?;
        self.navigate_unchecked(request.url()).await?;
        let args = Zeroizing::new(
            serde_json::to_string(&(email.as_str(), request.callback(), request.state()))
                .map_err(|_| Error::Invalid)?,
        );
        let expression = Zeroizing::new(format!("({})(...{})", include_str!("consent.js"), *args));
        loop {
            self.browser
                .cdp
                .pump(Duration::from_millis(600))
                .await
                .map_err(|_| Error::Unavailable)?;
            if let Some(callback) = self.drain_paused(request, &frame).await? {
                return Ok(callback);
            }
            let result = self
                .browser
                .cdp
                .command_timeout(
                    "Runtime.evaluate",
                    json!({"expression":expression.as_str(),"returnByValue":true,"timeout":2000}),
                    Some(&session),
                    CDP_STEP,
                )
                .await
                .map_err(|_| Error::Unavailable)?;
            // The event for the callback may have arrived while evaluating.
            if let Some(callback) = self.drain_paused(request, &frame).await? {
                return Ok(callback);
            }
            if result.get("exceptionDetails").is_some() {
                return Err(Error::Unavailable);
            }
            match result.pointer("/result/value").and_then(|v| v.as_str()) {
                Some("waiting") => {}
                Some("needs_human") => return Err(Error::NeedsHuman),
                // "returned" without our own captured callback is not proof of anything.
                Some("returned" | "blocked") => return Err(Error::Blocked),
                _ => return Err(Error::Unavailable),
            }
        }
    }

    async fn abort(&mut self, request_id: &str, session: &str) -> Result<(), Error> {
        self.browser
            .cdp
            .command_timeout(
                "Fetch.failRequest",
                json!({"requestId":request_id,"errorReason":"BlockedByClient"}),
                Some(session),
                CDP_STEP,
            )
            .await
            .map(drop)
            .map_err(|_| Error::Unavailable)
    }

    /// Consumes queued paused requests: one exact callback is fulfilled and
    /// returned; every other request is aborted.
    async fn drain_paused(
        &mut self,
        request: &LoginRequest,
        frame: &str,
    ) -> Result<Option<Callback>, Error> {
        let (events, overflow) = self.browser.cdp.take_paused();
        let (mut captured, mut rejected, mut failed) = (None, false, overflow);
        for event in events {
            let text = |p: &str| event.pointer(p).and_then(|v| v.as_str());
            let (Some(id), Some(sid)) = (text("/params/requestId"), text("/sessionId")) else {
                failed = true;
                continue;
            };
            let own = sid == self.session
                && text("/params/resourceType") == Some("Document")
                && text("/params/frameId") == Some(frame)
                && text("/params/request/method") == Some("GET")
                && event.pointer("/params/responseStatusCode").is_none()
                && event.pointer("/params/responseErrorReason").is_none();
            let accepted = if own && captured.is_none() {
                text("/params/request/url").and_then(|url| request.accept_callback(url).ok())
            } else {
                None
            };
            if let Some(callback) = accepted {
                let fulfilled = self
                    .browser
                    .cdp
                    .command_timeout(
                        "Fetch.fulfillRequest",
                        json!({
                            "requestId": id,
                            "responseCode": 200,
                            "responseHeaders": [
                                {"name":"Content-Type","value":"text/html; charset=utf-8"},
                                {"name":"Cache-Control","value":"no-store"},
                                {"name":"Referrer-Policy","value":"no-referrer"},
                            ],
                            "body": STANDARD.encode(BLANK_PAGE),
                        }),
                        Some(sid),
                        CDP_STEP,
                    )
                    .await;
                if fulfilled.is_err() {
                    let _ = self.abort(id, sid).await;
                    failed = true;
                } else {
                    captured = Some(callback);
                }
            } else {
                rejected |= own;
                failed |= self.abort(id, sid).await.is_err();
            }
        }
        if failed {
            Err(Error::Unavailable)
        } else if captured.is_none() && rejected {
            Err(Error::Blocked)
        } else {
            Ok(captured)
        }
    }

    /// Close the intercepted target without disabling Fetch. Closing the target
    /// cancels its pending requests; the new target shares only the same profile,
    /// preserving the authenticated account without carrying interception state.
    async fn release_clean(&mut self) -> Result<(), Error> {
        let closed = self
            .browser
            .cdp
            .command_timeout(
                "Target.closeTarget",
                json!({"targetId":self.target}),
                None,
                CDP_STEP,
            )
            .await
            .map_err(|_| Error::Unavailable)?;
        if closed["success"] != true {
            return Err(Error::Unavailable);
        }
        drop(self.browser.cdp.take_paused());
        let target = self
            .browser
            .cdp
            .command_timeout(
                "Target.createTarget",
                json!({"url":"about:blank"}),
                None,
                CDP_STEP,
            )
            .await
            .map_err(|_| Error::Unavailable)?;
        let target = target["targetId"]
            .as_str()
            .ok_or(Error::Unavailable)?
            .to_owned();
        let attached = self
            .browser
            .cdp
            .command_timeout(
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
                None,
                CDP_STEP,
            )
            .await
            .map_err(|_| Error::Unavailable)?;
        let session = attached["sessionId"]
            .as_str()
            .ok_or(Error::Unavailable)?
            .to_owned();
        self.browser
            .cdp
            .command_timeout("Page.enable", json!({}), Some(&session), CDP_STEP)
            .await
            .map_err(|_| Error::Unavailable)?;
        // Any late events belong to the already-closed target, never this tab.
        drop(self.browser.cdp.take_paused());
        self.target = target;
        self.session = session;
        Ok(())
    }

    async fn console_navigate(&mut self, url: &str) -> Result<(), Error> {
        if !url.starts_with("https://console.cloud.google.com/auth/") {
            return Err(Error::Invalid);
        }
        tokio::time::timeout(Duration::from_secs(20), async {
            let result = self
                .browser
                .cdp
                .command("Page.navigate", json!({"url":url}), Some(&self.session))
                .await
                .map_err(|_| Error::Unavailable)?;
            if result.get("errorText").is_some() {
                return Err(Error::Unavailable);
            }
            let loader = result
                .get("loaderId")
                .and_then(|v| v.as_str())
                .ok_or(Error::Unavailable)?;
            // Do not classify the previous document while Page.navigate is
            // still committing the new one. A cross-origin redirect is checked
            // by the fixed script after this loader has become authoritative.
            loop {
                let frame = self
                    .browser
                    .cdp
                    .command("Page.getFrameTree", json!({}), Some(&self.session))
                    .await
                    .map_err(|_| Error::Unavailable)?;
                if frame
                    .pointer("/frameTree/frame/loaderId")
                    .and_then(|v| v.as_str())
                    == Some(loader)
                {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .map_err(|_| Error::Unavailable)?
    }

    /// Like `console_evaluate`, but the string result (which may hold the one-time
    /// client secret) is moved out of the CDP value into a zeroizing buffer. The
    /// transport's own short-lived copies cannot be zeroized; the disposable
    /// worker process is the lifetime boundary.
    async fn client_evaluate(&mut self, expression: &str) -> Result<Zeroizing<String>, Error> {
        let mut result = self
            .browser
            .cdp
            .command_timeout(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"timeout":2000}),
                Some(&self.session),
                CDP_STEP,
            )
            .await
            .map_err(|_| Error::Unavailable)?;
        if result.get("exceptionDetails").is_some() {
            return Err(Error::Unavailable);
        }
        match result
            .pointer_mut("/result/value")
            .map(serde_json::Value::take)
        {
            Some(serde_json::Value::String(value)) => Ok(Zeroizing::new(value)),
            _ => Err(Error::Unavailable),
        }
    }

    async fn console_evaluate(&mut self, expression: &str) -> Result<String, Error> {
        let result = self
            .browser
            .cdp
            .command_timeout(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true,"timeout":2000}),
                Some(&self.session),
                CDP_STEP,
            )
            .await
            .map_err(|_| Error::Unavailable)?;
        if result.get("exceptionDetails").is_some() {
            return Err(Error::Unavailable);
        }
        result
            .pointer("/result/value")
            .and_then(|v| v.as_str())
            .map(str::to_owned)
            .ok_or(Error::Unavailable)
    }

    pub async fn shutdown(mut self) {
        self.browser.shutdown().await;
        self.display.stop().await;
    }
}
