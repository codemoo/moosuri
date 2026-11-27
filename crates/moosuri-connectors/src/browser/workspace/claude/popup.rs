//! Same-browser SSO windows for the Claude sign-in (e.g. "Continue with Google").
//!
//! Pages are auto-attached with `waitForDebuggerOnStart`, so a new window is paused
//! before any script or navigation. It is admitted only when all of these hold:
//! the sign-in is still waiting; a proof-validated human input was just dispatched
//! to the page that is named as the window's opener (`openerId`, `canAccessOpener`);
//! the window arrived after that input; its depth/count is within `MAX_POPUPS`.
//! An admitted window gets the same page settings and the callback Fetch rule, and
//! is only then resumed. Anything else is closed without ever being resumed, so no
//! request, script or callback of it can run. A popup callback is never adopted: it
//! is aborted like any non-root callback. Only the root session may deliver a code.
//! There is no URL, tab or CDP surface here; frames still show only an origin.
use super::*;
use crate::browser::cdp::Attached;

/// Windows admitted at once (also the opener-chain depth below the root).
pub(super) const MAX_POPUPS: usize = 2;
/// Window within which a gesture-opened window may appear after the input returns.
const ARM_WINDOW: Duration = Duration::from_secs(3);
/// Upper bound of one pending input (`remaining` never exceeds this).
const INPUT_BOUND: Duration = Duration::from_secs(15);

pub(super) struct Popup {
    pub page: Uuid,
    pub target: String,
    pub session: String,
}

/// Authority granted by exactly one accepted input; consumed by one admission.
pub(super) struct Arm {
    opener: String,
    after: u64,
    until: Instant,
}

impl ClaudeWorkspace {
    pub(super) fn displayed_target(&self) -> Option<String> {
        let handle = self.inner.current.as_ref()?;
        self.inner
            .session
            .targets
            .get(&handle.page)
            .map(|target| target.target.clone())
    }

    fn known_target(&self, id: &str) -> bool {
        self.inner.session.targets.values().any(|t| t.target == id)
    }

    /// Armed after the proof is validated and before the input is sent; the pending
    /// input itself may take up to its own bound, so the window starts generously and
    /// is tightened to `ARM_WINDOW` once the input is acknowledged (`extend_arm`).
    pub(super) fn arm_popup(&mut self, opener: String, after: u64) {
        self.arm = Some(Arm {
            opener,
            after,
            until: Instant::now() + INPUT_BOUND + ARM_WINDOW,
        });
    }

    pub(super) fn extend_arm(&mut self) {
        if let Some(arm) = self.arm.as_mut()
            && Instant::now() <= arm.until
        {
            arm.until = Instant::now() + ARM_WINDOW;
        }
    }

    /// Optionally reads events for `wait`, then accounts for attach/detach metadata.
    pub(super) async fn settle(&mut self, wait: Duration) -> Result<(), BrowserError> {
        let cancellation = self.inner.session.cancellation.clone();
        if cancellation.is_cancelled() {
            return Err(BrowserError::Cancelled);
        }
        let duration = self.inner.session.remaining()?;
        let result = tokio::select! { biased;
            _ = cancellation.cancelled() => Err(BrowserError::Cancelled),
            result = tokio::time::timeout(duration, self.settle_inner(wait)) =>
                result.unwrap_or(Err(BrowserError::Timeout)),
        };
        self.inner.session.after(&result);
        result
    }

    async fn settle_inner(&mut self, wait: Duration) -> Result<(), BrowserError> {
        if !wait.is_zero()
            && let Err(error) = self.inner.session.cdp.pump(wait).await
        {
            self.phase = Phase::Failed;
            return Err(error);
        }
        self.reconcile().await
    }

    /// Applies queued attach/detach metadata. While waiting, any ambiguity, overflow
    /// or failure ends the attempt (fail closed); once a code is held or delivered,
    /// windows are only closed and nothing can change the outcome.
    pub(super) async fn reconcile(&mut self) -> Result<(), BrowserError> {
        let (attached, detached, overflow) = self.inner.session.cdp.take_attachments();
        let waiting = matches!(self.phase, Phase::Waiting);
        let mut failed = overflow;
        for session in &detached {
            if waiting && self.on_detached(session).await.is_err() {
                failed = true;
            }
        }
        for item in attached {
            // A window that appeared and vanished within one batch is simply gone.
            if detached.contains(&item.session) {
                continue;
            }
            let session = item.session.clone();
            if self.on_attached(item).await.is_err() {
                // A window can disappear while its admission/close command is in
                // flight. Only Chrome's exact detach receipt proves that absence.
                if self.inner.session.cdp.detached_session(&session) {
                    failed |= self.on_detached(&session).await.is_err();
                } else {
                    failed = true;
                }
            }
        }
        if failed && waiting {
            self.phase = Phase::Failed;
            return Err(BrowserError::Protocol);
        }
        Ok(())
    }

    /// The initial setup report: the root and Chrome's blank page already exist and
    /// were not paused. Duplicate sessions are detached; unexpected paused pages closed.
    pub(super) async fn adopt_baseline(&mut self) -> Result<(), BrowserError> {
        let (attached, _, overflow) = self.inner.session.cdp.take_attachments();
        if overflow {
            return Err(BrowserError::Protocol);
        }
        for item in attached {
            if !item.page {
                self.detach(&item.session).await?;
            } else if item.waiting || item.opener.is_some() {
                self.close_target(&item.target).await?;
            } else {
                self.baseline.insert(item.target.clone());
                self.detach(&item.session).await?;
            }
        }
        Ok(())
    }

    async fn detach(&mut self, session: &str) -> Result<(), BrowserError> {
        self.inner
            .session
            .cdp
            .command_timeout(
                "Target.detachFromTarget",
                json!({"sessionId":session}),
                None,
                STEP,
            )
            .await
            .map(drop)
    }

    async fn close_target(&mut self, target: &str) -> Result<(), BrowserError> {
        self.inner
            .session
            .cdp
            .command_timeout("Target.closeTarget", json!({"targetId":target}), None, STEP)
            .await
            .map(drop)
    }

    async fn on_detached(&mut self, session: &str) -> Result<(), BrowserError> {
        if let Some(index) = self.popups.iter().position(|p| p.session == session) {
            let popup = self.popups.remove(index);
            self.inner.session.targets.remove(&popup.page);
            if self
                .inner
                .current
                .as_ref()
                .is_some_and(|handle| handle.page == popup.page)
            {
                // Return to the most recent surviving window, else the root.
                self.inner.current = None;
                self.show_top().await?;
            }
            return Ok(());
        }
        let root_lost = self
            .root_page
            .and_then(|root| self.inner.session.targets.get(&root))
            .is_some_and(|target| target.session == session);
        if root_lost {
            return Err(BrowserError::Protocol);
        }
        Ok(())
    }

    pub(super) async fn close_callback_window(
        &mut self,
        session: &str,
    ) -> Result<(), BrowserError> {
        let target = self
            .popups
            .iter()
            .find(|p| p.session == session)
            .ok_or(BrowserError::InvalidHandle)?
            .target
            .clone();
        if let Err(error) = self.close_target(&target).await
            && !self.inner.session.cdp.detached_session(session)
        {
            return Err(error);
        }
        self.on_detached(session).await
    }

    async fn on_attached(&mut self, item: Attached) -> Result<(), BrowserError> {
        if !item.page {
            return self.detach(&item.session).await;
        }
        if !item.waiting {
            // Not paused, so its start cannot be intercepted: keep only known pages.
            if self.known_target(&item.target)
                || self.baseline.contains(&item.target)
                || item.opener.is_none()
            {
                return self.detach(&item.session).await;
            }
            return self.close_target(&item.target).await;
        }
        let waiting = matches!(self.phase, Phase::Waiting);
        let armed = self.arm.as_ref().is_some_and(|arm| {
            Instant::now() <= arm.until
                && item.seq > arm.after
                && item.can_access_opener
                && item.opener.as_deref() == Some(arm.opener.as_str())
                && self.known_target(&arm.opener)
        });
        if !waiting || !armed {
            // Unrelated, noopener, stale or unprompted: never resumed, never shown.
            return self.close_target(&item.target).await;
        }
        if self.popups.len() >= MAX_POPUPS {
            let _ = self.close_target(&item.target).await;
            return Err(BrowserError::Capacity);
        }
        self.arm = None;
        let admitted = tokio::time::timeout(Duration::from_secs(10), self.admit(&item))
            .await
            .unwrap_or(Err(BrowserError::Timeout));
        if admitted.is_err() {
            let _ = self.close_target(&item.target).await;
        }
        admitted
    }

    /// Configure, install the callback rule, resume, then display. The Fetch rule is
    /// in place before the window runs anything.
    async fn admit(&mut self, item: &Attached) -> Result<(), BrowserError> {
        let session = item.session.as_str();
        self.inner.session.configure_page(session).await?;
        self.inner
            .session
            .cdp
            .command_timeout("Fetch.enable", fetch_patterns(), Some(session), STEP)
            .await?;
        let tree = self
            .inner
            .session
            .cdp
            .command_timeout("Page.getFrameTree", json!({}), Some(session), STEP)
            .await?;
        let frame = tree
            .pointer("/frameTree/frame")
            .ok_or(BrowserError::Protocol)?;
        let record = Target {
            target: item.target.clone(),
            session: item.session.clone(),
            frame: string(frame, "id")?,
            loader: string(frame, "loaderId")?,
        };
        let page = Uuid::new_v4();
        self.inner.session.targets.insert(page, record);
        self.popups.push(Popup {
            page,
            target: item.target.clone(),
            session: item.session.clone(),
        });
        self.inner
            .session
            .cdp
            .command_timeout(
                "Runtime.runIfWaitingForDebugger",
                json!({}),
                Some(session),
                STEP,
            )
            .await?;
        self.show_top().await
    }

    /// Displays the most recent window (or the root). Changing the displayed page
    /// invalidates every frame proof, so no earlier token can be used on it.
    async fn show_top(&mut self) -> Result<(), BrowserError> {
        let page = self
            .popups
            .last()
            .map(|popup| popup.page)
            .or(self.root_page)
            .ok_or(BrowserError::InvalidHandle)?;
        let target = self
            .inner
            .session
            .targets
            .get(&page)
            .ok_or(BrowserError::InvalidHandle)?
            .target
            .clone();
        self.inner.clear_proofs();
        self.inner.last_links = None;
        self.inner
            .session
            .cdp
            .command_timeout(
                "Target.activateTarget",
                json!({"targetId":target}),
                None,
                STEP,
            )
            .await?;
        self.inner.current = Some(PageHandle {
            page,
            worker_generation: self.inner.session.generation,
            navigation_generation: 1,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
