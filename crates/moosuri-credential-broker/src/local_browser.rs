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
            site_adapter_available: false,
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

fn cleanup(record: &Record, root: &Path) -> Result<()> {
    if record.process_group.is_some_and(alive) || record.phase == Phase::Starting {
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
        stop(&self.process);
        let child = self.process.lock().map_err(|_| Error::PrivateState)?.take();
        if let Some(mut child) = child {
            tokio::time::timeout(Duration::from_secs(3), child.wait())
                .await
                .map_err(|_| Error::Unknown)?
                .map_err(|_| Error::Unknown)?;
        }
        self.pipe
            .shutdown()
            .await
            .map_err(|_| Error::Unavailable)
            .ok();
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
