//! Device-local browser groundwork. No site adapter, secrets, DOM or server transport.
//!
//! The only observable page data is a bounded page count. Starting a browser is a
//! local human command, never a model capability or a credential grant.
use crate::{Engine, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    process::{Child, Command},
};
use uuid::Uuid;

const LIFETIME: i64 = 8 * 60 * 60;
const IDLE: i64 = 30 * 60;
const HANDOFF: i64 = 5 * 60;
const RESPONSE_LIMIT: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Starting,
    AwaitingHuman,
    Ready,
    RecoveryRequired,
    Closed,
    Expired,
}

/// Contains no page URL/title/text, cookie, browser endpoint, password or grant.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    id: Uuid,
    generation: u64,
    created_at: i64,
    expires_at: i64,
    idle_until: i64,
    handoff_until: i64,
    last_clock: i64,
    phase: Phase,
    process_group: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct Observation {
    pub session: Uuid,
    pub generation: u64,
    pub phase: Phase,
    pub expires_at: i64,
    pub idle_until: i64,
    pub handoff_until: i64,
    pub page_count: Option<usize>,
    /// Intentionally false until a separate reviewed site adapter is installed.
    pub site_adapter_available: bool,
}

impl Record {
    fn new(now: i64) -> Result<Self> {
        if now <= 0 {
            return Err(Error::Policy);
        }
        Ok(Self {
            id: Uuid::new_v4(),
            generation: 1,
            created_at: now,
            expires_at: now.checked_add(LIFETIME).ok_or(Error::Policy)?,
            idle_until: now.checked_add(IDLE).ok_or(Error::Policy)?,
            handoff_until: now.checked_add(HANDOFF).ok_or(Error::Policy)?,
            last_clock: now,
            phase: Phase::Starting,
            process_group: None,
        })
    }

    fn check(&self, now: i64) -> Result<()> {
        if now < self.last_clock
            || self.created_at <= 0
            || self.expires_at.checked_sub(self.created_at) != Some(LIFETIME)
            || self.idle_until > self.expires_at
            || self.handoff_until > self.expires_at
            || self.last_clock < self.created_at
            || now >= self.expires_at
            || now >= self.idle_until
            || matches!(self.phase, Phase::Closed | Phase::Expired)
        {
            return Err(Error::Policy);
        }
        Ok(())
    }

    fn observation(&self, pages: Option<usize>) -> Observation {
        Observation {
            session: self.id,
            generation: self.generation,
            phase: self.phase,
            expires_at: self.expires_at,
            idle_until: self.idle_until,
            handoff_until: self.handoff_until,
            page_count: pages,
            site_adapter_available: true,
        }
    }

    fn profile(&self, root: &Path) -> PathBuf {
        root.join(format!("local-browser-{}-{}", self.id, self.generation))
    }
}

pub(crate) struct Browser {
    process: Arc<Mutex<Option<Child>>>,
    watchdog: tokio::task::JoinHandle<()>,
    pipe: UnixStream,
    next: u64,
    deadline: Arc<Mutex<Instant>>,
}

fn stop(process: &Arc<Mutex<Option<Child>>>) {
    if let Ok(mut slot) = process.lock()
        && let Some(child) = slot.as_mut()
    {
        // We own this unreaped process group, never a PID read from old storage.
        if let Some(pid) = child.id() {
            unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
        }
        let _ = child.start_kill();
    }
}

/// Owns an unreaped child across cancellation points during explicit shutdown.
/// `Child` alone kills only its parent on drop; descendants require our group.
/// Tokio clears `id()` only after successful reap, so a reaped PID is never used.
struct ShutdownChild(Child);
impl Drop for ShutdownChild {
    fn drop(&mut self) {
        if let Some(pid) = self.0.id() {
            unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
            let _ = self.0.start_kill();
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.watchdog.abort();
        stop(&self.process);
        // Do not claim cleanup on Drop. A subsequent local action checks that
        // the process group has exited before deleting its private profile.
    }
}

fn alive(group: u32) -> bool {
    if group <= 1 || group > i32::MAX as u32 {
        return true;
    }
    (unsafe { libc::kill(-(group as i32), 0) == 0 })
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Read-only quiescence check after our owned parent has exited. Renderer
/// children may still be exiting/reaped by init. Never signal a reaped PID.
async fn wait_group_absent(group: u32) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(3), async {
        while alive(group) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| Error::Unknown)
}

fn cleanup(record: &Record, root: &Path) -> Result<()> {
    if record.process_group.is_some_and(alive) || record.phase == Phase::Starting {
        synthetic_site_stage(SiteStage::CleanupGroupAlive);
        return Err(Error::Unknown);
    }
    let profile = record.profile(root);
    match fs::symlink_metadata(&profile) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {
            crate::journal::private_directory(&profile)?;
            fs::remove_dir_all(profile).map_err(|_| Error::PrivateState)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(Error::PrivateState),
    }
}

impl Browser {
    async fn launch(executable: &Path, profile: PathBuf, seconds: i64) -> Result<(Self, u32)> {
        use std::os::{
            fd::AsRawFd,
            unix::fs::{DirBuilderExt, MetadataExt},
        };
        let meta = fs::symlink_metadata(executable).map_err(|_| Error::PrivateState)?;
        if !executable.is_absolute()
            || !meta.is_file()
            || meta.file_type().is_symlink()
            // The explicit executable and local OS administrators are trusted.
            // macOS applications commonly allow the local admin group to update.
            || meta.mode() & 0o002 != 0
            || (meta.uid() != 0 && meta.uid() != unsafe { libc::geteuid() })
            || seconds <= 0
        {
            return Err(Error::Policy);
        }
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&profile)
            .map_err(|_| Error::PrivateState)?;
        crate::journal::private_directory(&profile)?;
        let default = profile.join("Default");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&default)
            .map_err(|_| Error::PrivateState)?;
        let preferences = json!({
            "session":{"restore_on_startup":5},
            "credentials_enable_service":false,
            "profile":{"password_manager_enabled":false,"default_content_setting_values":{
                "automatic_downloads":2,"notifications":2,"geolocation":2,
                "media_stream_mic":2,"media_stream_camera":2,"popups":2}},
            "download":{"prompt_for_download":true}
        });
        crate::journal::private_write(&default, "Preferences", preferences.to_string().as_bytes())?;
        let (parent, child_pipe) =
            std::os::unix::net::UnixStream::pair().map_err(|_| Error::PrivateState)?;
        parent
            .set_nonblocking(true)
            .map_err(|_| Error::PrivateState)?;
        // A socketpair is inherited only as Chromium's read/write pipe FDs3/4.
        // No debugging TCP listener or attach-to-existing-profile option exists.
        let mut command = Command::new(executable);
        command.env_clear();
        for name in [
            "HOME",
            "TMPDIR",
            "DISPLAY",
            "XAUTHORITY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .args([
                "--remote-debugging-pipe",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-sync",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-crash-reporter",
                "--disable-breakpad",
                "--disable-default-apps",
                "--incognito",
                "--new-window",
                "about:blank",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        let fd = child_pipe.as_raw_fd();
        unsafe {
            command.pre_exec(move || {
                // Duplicate above4 before replacing either destination.
                let source = libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 5);
                if source < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let failed =
                    libc::setsid() < 0 || libc::dup2(source, 3) < 0 || libc::dup2(source, 4) < 0;
                libc::close(source);
                if failed {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let pipe = UnixStream::from_std(parent).map_err(|_| Error::PrivateState)?;
        let child = command.spawn().map_err(|_| Error::PrivateState)?;
        let pid = child.id().ok_or(Error::PrivateState)?;
        drop(child_pipe);
        let process = Arc::new(Mutex::new(Some(child)));
        let deadline = Arc::new(Mutex::new(
            Instant::now() + Duration::from_secs(seconds as u64),
        ));
        let (watched, until) = (process.clone(), deadline.clone());
        let watchdog = tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if until.lock().map_or(true, |d| Instant::now() >= *d) {
                    stop(&watched);
                    break;
                }
            }
        });
        Ok((
            Self {
                process,
                watchdog,
                pipe,
                next: 0,
                deadline,
            },
            pid,
        ))
    }

    async fn pages(&mut self) -> Result<usize> {
        self.next = self.next.checked_add(1).ok_or(Error::PrivateState)?;
        let id = self.next;
        tokio::time::timeout(Duration::from_secs(3), async {
            let bytes = format!(
                "{}\0",
                json!({"id":id,"method":"Target.getTargets","params":{}})
            );
            self.pipe
                .write_all(bytes.as_bytes())
                .await
                .map_err(|_| Error::Unavailable)?;
            let mut consumed = 0;
            for _ in 0..128 {
                let mut bytes = zeroize::Zeroizing::new(Vec::new());
                loop {
                    let byte = self.pipe.read_u8().await.map_err(|_| Error::Unavailable)?;
                    consumed += 1;
                    if consumed > RESPONSE_LIMIT {
                        return Err(Error::ProviderContent);
                    }
                    if byte == 0 {
                        break;
                    }
                    bytes.push(byte);
                }
                // Chromium target metadata can contain secret-bearing URLs/titles.
                // Never log/store/return it; only count literal page target kinds.
                let response: Value =
                    serde_json::from_slice(&bytes).map_err(|_| Error::ProviderContent)?;
                if response.get("id").and_then(Value::as_u64) != Some(id) {
                    continue;
                }
                if response.get("error").is_some() {
                    return Err(Error::Unavailable);
                }
                let targets = response
                    .pointer("/result/targetInfos")
                    .and_then(Value::as_array)
                    .ok_or(Error::ProviderContent)?;
                if targets.len() > 64 {
                    return Err(Error::ProviderContent);
                }
                let pages = targets
                    .iter()
                    .filter(|v| v.get("type").and_then(Value::as_str) == Some("page"))
                    .count();
                return if pages <= 8 {
                    Ok(pages)
                } else {
                    Err(Error::Policy)
                };
            }
            Err(Error::ProviderContent)
        })
        .await
        .map_err(|_| Error::Unavailable)?
    }

    async fn close(mut self) -> Result<()> {
        self.watchdog.abort();
        // Give Chromium's browser process the first opportunity to shut down
        // and reap its renderer/network children. Killing the entire group first
        // can orphan zombies under a non-reaping container PID1, so kill(0)
        // correctly continues to report a group even after our child is reaped.
        self.next = self.next.checked_add(1).ok_or(Error::PrivateState)?;
        let close = format!(
            "{}\0",
            json!({"id":self.next,"method":"Browser.close","params":{}})
        );
        let _ = tokio::time::timeout(
            Duration::from_secs(1),
            self.pipe.write_all(close.as_bytes()),
        )
        .await;
        let child = self.process.lock().map_err(|_| Error::PrivateState)?.take();
        if let Some(child) = child {
            // Keep the whole owned group under RAII while awaiting reaping.
            // Cancellation drops this guard before Browser's empty slot.
            let mut child = ShutdownChild(child);
            let graceful = tokio::time::timeout(Duration::from_secs(3), child.0.wait()).await;
            match graceful {
                Ok(result) => {
                    result.map_err(|_| Error::Unknown)?;
                    synthetic_site_stage(SiteStage::CloseGracefulReaped);
                }
                Err(_) => {
                    // Still own this unreaped child, never a retained journal PID.
                    if let Some(pid) = child.0.id() {
                        unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
                    }
                    let _ = child.0.start_kill();
                    tokio::time::timeout(Duration::from_secs(3), child.0.wait())
                        .await
                        .map_err(|_| Error::Unknown)?
                        .map_err(|_| Error::Unknown)?;
                    synthetic_site_stage(SiteStage::CloseForcedReaped);
                }
            }
        }
        self.pipe.shutdown().await.ok();
        Ok(())
    }
}

impl Engine {
    pub fn browser_suspend(&mut self) -> Result<()> {
        self.local_browser.take();
        self.retain_browser();
        self.save()
    }

    pub(crate) fn retain_browser(&mut self) {
        if let Some(record) = self.state.local_browser.as_mut()
            && !matches!(
                record.phase,
                Phase::Closed | Phase::Expired | Phase::Starting
            )
        {
            record.phase = Phase::RecoveryRequired;
        }
    }

    /// Fixed local state only; no browser connection or page content is read.
    pub fn browser_status(&self) -> Result<Observation> {
        if self.poisoned {
            return Err(Error::PrivateState);
        }
        self.state
            .local_browser
            .as_ref()
            .map(|r| r.observation(None))
            .ok_or(Error::Policy)
    }

    /// Local human command. Never called by the server/model transport loop.
    pub async fn browser_start(
        &mut self,
        executable: &Path,
        resume: bool,
        now: i64,
    ) -> Result<Observation> {
        if self.poisoned || self.state.revoked || self.local_browser.is_some() {
            return Err(Error::Policy);
        }
        let mut record = if let Some(old) = &self.state.local_browser {
            cleanup(old, &self.local_root)?;
            if resume {
                old.check(now)?;
                if old.phase != Phase::RecoveryRequired {
                    return Err(Error::Policy);
                }
                let mut next = old.clone();
                next.generation = next.generation.checked_add(1).ok_or(Error::Policy)?;
                next.handoff_until = (now + HANDOFF).min(next.expires_at).min(next.idle_until);
                next.last_clock = now;
                next.phase = Phase::Starting;
                next.process_group = None;
                next
            } else {
                if !matches!(old.phase, Phase::Closed | Phase::Expired) {
                    return Err(Error::Policy);
                }
                Record::new(now)?
            }
        } else {
            if resume {
                return Err(Error::Policy);
            }
            Record::new(now)?
        };
        self.state.local_browser = Some(record.clone());
        self.save()?; // A crash before PID persistence is unknown; never auto-relaunch.
        let launched = Browser::launch(
            executable,
            record.profile(&self.local_root),
            record.handoff_until - now,
        )
        .await;
        let (browser, pid) = match launched {
            Ok(value) => value,
            Err(error) => {
                record.phase = Phase::Closed;
                self.state.local_browser = Some(record);
                self.save()?;
                return Err(error);
            }
        };
        record.process_group = Some(pid);
        record.phase = Phase::AwaitingHuman;
        self.state.local_browser = Some(record.clone());
        self.local_browser = Some(browser);
        if let Err(error) = self.save() {
            self.local_browser.take();
            return Err(error);
        }
        Ok(record.observation(None))
    }

    /// Explicit local confirmation after viewing the fresh blank window. This
    /// records human presence only; it proves neither site login nor task success.
    pub async fn browser_observe(&mut self, confirm: bool, now: i64) -> Result<Observation> {
        if self.poisoned || self.state.revoked {
            return Err(Error::Policy);
        }
        let record = self.state.local_browser.as_mut().ok_or(Error::Policy)?;
        if record.check(now).is_err()
            || (record.phase == Phase::AwaitingHuman && now >= record.handoff_until)
        {
            self.browser_close_at(now, true).await?;
            return Err(Error::Policy);
        }
        if !matches!(record.phase, Phase::AwaitingHuman | Phase::Ready) {
            return Err(Error::Policy);
        }
        if confirm && record.phase != Phase::AwaitingHuman {
            return Err(Error::Policy);
        }
        let operation_started = Instant::now();
        let browser = self.local_browser.as_mut().ok_or(Error::Unavailable)?;
        if browser
            .deadline
            .lock()
            .map_or(true, |d| operation_started >= *d)
        {
            self.browser_close_at(now, true).await?;
            return Err(Error::Policy);
        }
        let pages = match browser.pages().await {
            Ok(pages) => pages,
            Err(error) => {
                self.browser_close_at(now, false).await?;
                return Err(error);
            }
        };
        if browser
            .deadline
            .lock()
            .map_or(true, |d| Instant::now() >= *d)
        {
            self.browser_close_at(now, true).await?;
            return Err(Error::Policy);
        }
        let record = self.state.local_browser.as_mut().unwrap();
        record.last_clock = now;
        if confirm {
            record.phase = Phase::Ready;
            record.idle_until = (now + IDLE).min(record.expires_at);
            *browser.deadline.lock().map_err(|_| Error::PrivateState)? =
                operation_started + Duration::from_secs((record.idle_until - now) as u64);
        }
        // Observation/polling does not renew idle time. No page data is journaled.
        let result = record.observation(Some(pages));
        self.save()?;
        Ok(result)
    }

    pub async fn browser_close(&mut self, now: i64) -> Result<()> {
        self.browser_close_at(now, false).await
    }

    async fn browser_close_at(&mut self, now: i64, expired: bool) -> Result<()> {
        if let Some(browser) = self.local_browser.take() {
            browser.close().await?;
            // Parent reaping and complete group disappearance are distinct.
            // Give init a bounded opportunity to reap already exiting children.
            if let Some(group) = self
                .state
                .local_browser
                .as_ref()
                .and_then(|r| r.process_group)
            {
                wait_group_absent(group).await?;
            }
        }
        let record = self.state.local_browser.as_mut().ok_or(Error::Policy)?;
        cleanup(record, &self.local_root)?;
        record.phase = if expired {
            Phase::Expired
        } else {
            Phase::Closed
        };
        record.last_clock = record.last_clock.max(now);
        record.process_group = None;
        self.save()
    }
}

#[cfg(test)]
mod tests;

// Compile-time test fixture capability plus two explicit synthetic-test opt-ins.
// Every emitted value is a fixed enum variant. Never format browser input/output.
#[derive(Clone, Copy, Debug)]
enum SiteStage {
    Targets,
    Attach,
    Interception,
    PageEnabled,
    LoginNavigation,
    LoginPage,
    VersionChecked,
    SessionChecked,
    PasswordSubmitted,
    TotpChallenge,
    TotpSubmitted,
    Authenticated,
    RenderedVersionChecked,
    RepositoryNavigation,
    RepositoryPage,
    AccountChecked,
    CountProjected,
    InterceptionDisabled,
    Detached,
    OriginDenied,
    CdpRejected,
    JavascriptRejected,
    CloseGracefulReaped,
    CloseForcedReaped,
    CleanupGroupAlive,
}
fn synthetic_site_stage(stage: SiteStage) {
    #[cfg(any(test, feature = "test-fixtures"))]
    if std::env::var_os("MOOSURI_GITEA_SYNTHETIC_STAGE_TRACE").is_some_and(|v| v == "1")
        && std::env::var_os("MOOSURI_GITEA_TEST_FIXTURE").is_some()
    {
        eprintln!("moosuri synthetic Gitea stage: {stage:?}");
    }
    #[cfg(not(any(test, feature = "test-fixtures")))]
    let _ = stage;
}

impl Browser {
    /// Internal only: the model/server never supplies CDP methods or script text.
    async fn site_call(
        &mut self,
        method: &str,
        params: Value,
        session: Option<&str>,
        origin: &str,
    ) -> Result<Value> {
        self.next = self.next.checked_add(1).ok_or(Error::Policy)?;
        let id = self.next;
        let mut request = json!({"id":id,"method":method,"params":params});
        if let Some(s) = session {
            request["sessionId"] = json!(s)
        }
        let encoded = zeroize::Zeroizing::new(format!("{}\0", request));
        tokio::time::timeout(Duration::from_secs(15),async {
            self.pipe.write_all(encoded.as_bytes()).await.map_err(|_|Error::Unknown)?;
            let mut consumed=0usize;
            loop {
                let mut bytes=zeroize::Zeroizing::new(Vec::new());loop{let b=self.pipe.read_u8().await.map_err(|_|Error::Unknown)?;consumed+=1;if consumed>1024*1024{return Err(Error::ProviderContent)}if b==0{break}bytes.push(b)}
                let v:Value=serde_json::from_slice(&bytes).map_err(|_|Error::ProviderContent)?;
                if v["method"]=="Fetch.requestPaused" {
                    let target=v.pointer("/params/request/url").and_then(Value::as_str).and_then(|s|url::Url::parse(s).ok());
                    let allowed=target.is_some_and(|u|u.origin().ascii_serialization()==origin && u.username().is_empty() && u.password().is_none());
                    self.next=self.next.checked_add(1).ok_or(Error::Policy)?;
                    let mut reply=json!({"id":self.next,"method":if allowed{"Fetch.continueRequest"}else{"Fetch.failRequest"},"params":{"requestId":v["params"]["requestId"]},"sessionId":v["sessionId"]});
                    if !allowed {reply["params"]["errorReason"]=json!("BlockedByClient");}
                    let data=zeroize::Zeroizing::new(format!("{}\0",reply));self.pipe.write_all(data.as_bytes()).await.map_err(|_|Error::Unknown)?;
                    if !allowed{synthetic_site_stage(SiteStage::OriginDenied);return Err(Error::Policy)}continue;
                }
                if v["id"].as_u64()!=Some(id){continue}
                if v.get("error").is_some(){
                    synthetic_site_stage(SiteStage::CdpRejected);
                    // Chromium reports a destroyed/missing execution context
                    // with -32000 during a document navigation. Only the explicit
                    // read-only wait may retry this; no action is replayed.
                    return Err(if method=="Runtime.evaluate" && v.pointer("/error/code").and_then(Value::as_i64)==Some(-32000){Error::Unavailable}else{Error::Unknown})
                }
                return v.get("result").cloned().ok_or(Error::Unknown)
            }
        }).await.map_err(|_|Error::Unknown)?
    }
    async fn site_eval(&mut self, session: &str, origin: &str, script: String) -> Result<Value> {
        let value = self
            .site_call(
                "Runtime.evaluate",
                json!({"expression":script,"awaitPromise":true,"returnByValue":true}),
                Some(session),
                origin,
            )
            .await?;
        if value.get("exceptionDetails").is_some() {
            synthetic_site_stage(SiteStage::JavascriptRejected);
            return Err(Error::Policy);
        }
        value
            .pointer("/result/value")
            .cloned()
            .ok_or(Error::ProviderContent)
    }
    /// Poll on the native side so an asynchronous JavaScript promise does not
    /// span a document swap. This accepts only a bounded numeric observation.
    /// The caller supplies an adapter-owned read expression, never an action.
    async fn site_wait(&mut self, session: &str, origin: &str, condition: &str) -> Result<()> {
        let origin_json = serde_json::to_string(origin).map_err(|_| Error::Policy)?;
        let expression = format!(
            "(()=>{{if(location.origin!=={origin_json} && location.href!=='about:blank')return -1;return ({condition})?1:0;}})()"
        );
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                match self.site_eval(session, origin, expression.clone()).await {
                    Ok(value) => match value.as_i64() {
                        Some(1) => return Ok(()),
                        Some(0) => {}
                        Some(-1) => return Err(Error::Policy),
                        _ => return Err(Error::ProviderContent),
                    },
                    Err(Error::Unavailable) => {} // Read-only context observation only.
                    Err(error) => return Err(error),
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| Error::Unknown)?
    }
    async fn gitea_read(
        &mut self,
        origin: &str,
        repo: &moosuri_vault_protocol::broker::GitHubRepository,
        fields: &moosuri_vault_core::ItemFields,
    ) -> Result<u64> {
        let targets = self
            .site_call("Target.getTargets", json!({}), None, origin)
            .await?;
        synthetic_site_stage(SiteStage::Targets);
        let pages = targets["targetInfos"]
            .as_array()
            .ok_or(Error::Policy)?
            .iter()
            .filter(|v| v["type"] == "page")
            .collect::<Vec<_>>();
        if pages.len() != 1 {
            return Err(Error::Policy);
        }
        let attached = self
            .site_call(
                "Target.attachToTarget",
                json!({"targetId":pages[0]["targetId"],"flatten":true}),
                None,
                origin,
            )
            .await?;
        synthetic_site_stage(SiteStage::Attach);
        let session = attached["sessionId"]
            .as_str()
            .ok_or(Error::Policy)?
            .to_owned();
        self.site_call(
            "Fetch.enable",
            json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}]}),
            Some(&session),
            origin,
        )
        .await?;
        synthetic_site_stage(SiteStage::Interception);
        self.site_call("Page.enable", json!({}), Some(&session), origin)
            .await?;
        synthetic_site_stage(SiteStage::PageEnabled);
        let origin_json = serde_json::to_string(origin).map_err(|_| Error::Policy)?;
        let user_json = zeroize::Zeroizing::new(
            serde_json::to_string(fields.username.expose()).map_err(|_| Error::Policy)?,
        );
        let user = user_json.as_str();
        self.site_call(
            "Page.navigate",
            json!({"url":format!("{origin}/user/login")}),
            Some(&session),
            origin,
        )
        .await?;
        synthetic_site_stage(SiteStage::LoginNavigation);
        self.site_wait(&session,origin,"document.readyState==='complete' && ((location.pathname==='/user/login' && document.querySelector('#user_name')) || (!location.pathname.startsWith('/user/') && document.querySelector('.user-menu')))").await?;
        synthetic_site_stage(SiteStage::LoginPage);
        // Gitea's strict REQUIRE_SIGNIN_VIEW gates even /api/v1/version.
        // Its public head_script supplies AssetVersion; server.go derives it
        // directly from AppVer (only '+' becomes '~'). Pin it before any input.
        self.site_eval(
            &session,
            origin,
            "(()=>{if(window.config?.assetVersionEncoded!=='1.24.6')throw 0;return true;})()"
                .into(),
        )
        .await?;
        synthetic_site_stage(SiteStage::VersionChecked);
        let logged_in=self.site_eval(&session,origin,format!("location.origin==={origin_json} && !location.pathname.startsWith('/user/') && !!document.querySelector('.user-menu')")).await?.as_bool().ok_or(Error::Policy)?;
        synthetic_site_stage(SiteStage::SessionChecked);
        if !logged_in {
            let password =
                serde_json::to_string(fields.secret.expose()).map_err(|_| Error::Policy)?;
            // Fixed form, action, fields, top frame and challenge. Unknown MFA/CAPTCHA stops.
            let script = zeroize::Zeroizing::new(format!(
                "(()=>{{if(location.origin!=={origin_json}||location.pathname!='/user/login'||window.top!==window||document.querySelector('iframe,.g-recaptcha,.h-captcha,input[name=captcha]'))throw 0;let u=document.querySelectorAll('input#user_name[name=user_name]'),p=document.querySelectorAll('input#password[name=password][type=password]');if(u.length!==1||p.length!==1||u[0].form!==p[0].form)throw 0;let f=u[0].form;if(f.method!=='post'||f.action!=={origin_json}+'/user/login')throw 0;u[0].value={user};p[0].value={password};f.requestSubmit();return true;}})()"
            ));
            self.site_eval(&session, origin, script.to_string()).await?;
            synthetic_site_stage(SiteStage::PasswordSubmitted);
            self.site_wait(&session,origin,"location.pathname==='/user/two_factor' && document.readyState==='complete' && document.querySelector('#passcode')").await?;
            synthetic_site_stage(SiteStage::TotpChallenge);
            let totp = crate::gitea::totp(fields)?;
            let code = totp
                .code_at(crate::current_time()? as u64)
                .map_err(|_| Error::Policy)?;
            let passcode = serde_json::to_string(code.expose()).map_err(|_| Error::Policy)?;
            let script = zeroize::Zeroizing::new(format!(
                "(()=>{{if(location.origin!=={origin_json}||location.pathname!='/user/two_factor'||window.top!==window||document.querySelector('iframe'))throw 0;let p=document.querySelectorAll('input#passcode[name=passcode]');if(p.length!==1)throw 0;let f=p[0].form;if(f.method!=='post'||f.action!=={origin_json}+'/user/two_factor')throw 0;p[0].value={passcode};f.requestSubmit();return true;}})()"
            ));
            self.site_eval(&session, origin, script.to_string()).await?;
            synthetic_site_stage(SiteStage::TotpSubmitted);
            self.site_wait(
                &session,
                origin,
                "!location.pathname.startsWith('/user/') && document.readyState==='complete'",
            )
            .await?;
        }
        synthetic_site_stage(SiteStage::Authenticated);
        // Gitea's API auth group deliberately omits web Session auth. A strict
        // instance rejects browser cookies at /api/v1/version even after login.
        // Remain on the authenticated rendered-page surface for this adapter.
        let path = format!("/{}/{}", repo.owner, repo.repository);
        let path_json = serde_json::to_string(&path).map_err(|_| Error::Policy)?;
        self.site_call(
            "Page.navigate",
            json!({"url":format!("{origin}{path}")}),
            Some(&session),
            origin,
        )
        .await?;
        synthetic_site_stage(SiteStage::RepositoryNavigation);
        self.site_wait(&session,origin,&format!("location.pathname==={path_json} && document.readyState==='complete' && document.querySelector('.repository')")).await?;
        synthetic_site_stage(SiteStage::RepositoryPage);
        self.site_eval(
            &session,
            origin,
            "(()=>{if(window.config?.assetVersionEncoded!=='1.24.6')throw 0;return true;})()"
                .into(),
        )
        .await?;
        synthetic_site_stage(SiteStage::RenderedVersionChecked);
        self.site_eval(&session,origin,format!("(()=>{{let u=document.querySelector('.user-menu strong');if(!u||u.textContent.trim()!=={user})throw 0;return true;}})()")).await?;
        synthetic_site_stage(SiteStage::AccountChecked);
        // Only a decimal count may cross this projection. No text/URLs/HTML returned.
        let value=self.site_eval(&session,origin,format!("(()=>{{if(location.origin!=={origin_json}||location.pathname!=={path_json}||document.querySelector('iframe'))throw 0;let a=[...document.querySelectorAll('a.item')].filter(a=>a.getAttribute('href')==={path_json}+'/issues');if(a.length!==1)throw 0;let label=a[0].querySelector('span.label');if(!label)return 0;let n=label.textContent.trim();if(!/^[0-9]+$/.test(n))throw 0;let value=Number(n);if(!Number.isSafeInteger(value)||value>1000000000)throw 0;return value;}})()")).await?;
        let count = value
            .as_u64()
            .filter(|v| *v <= 1_000_000_000)
            .ok_or(Error::ProviderContent)?;
        synthetic_site_stage(SiteStage::CountProjected);
        self.site_call("Fetch.disable", json!({}), Some(&session), origin)
            .await?;
        synthetic_site_stage(SiteStage::InterceptionDisabled);
        self.site_call(
            "Target.detachFromTarget",
            json!({"sessionId":session}),
            None,
            origin,
        )
        .await?;
        synthetic_site_stage(SiteStage::Detached);
        Ok(count)
    }
}
impl Engine {
    pub(crate) fn browser_adapter_ready(&self, now: i64) -> Result<()> {
        let r = self
            .state
            .local_browser
            .as_ref()
            .ok_or(Error::Unavailable)?;
        r.check(now)?;
        if r.phase != Phase::Ready
            || self
                .local_browser
                .as_ref()
                .is_none_or(|b| b.deadline.lock().map_or(true, |d| Instant::now() >= *d))
        {
            return Err(Error::Unavailable);
        }
        Ok(())
    }
    pub(crate) async fn browser_gitea(
        &mut self,
        origin: &str,
        repo: &moosuri_vault_protocol::broker::GitHubRepository,
        fields: &moosuri_vault_core::ItemFields,
    ) -> Result<u64> {
        self.browser_adapter_ready(crate::current_time()?)?;
        let result = self
            .local_browser
            .as_mut()
            .ok_or(Error::Unavailable)?
            .gitea_read(origin, repo, fields)
            .await;
        if result.is_err() {
            let _ = self.browser_close_at(crate::current_time()?, false).await;
        }
        result
    }
}
