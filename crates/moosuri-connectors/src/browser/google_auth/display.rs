//! One private X server inside the disposable authentication capsule.
use super::Error;
use ring::rand::{SecureRandom, SystemRandom};
use std::{os::unix::fs::DirBuilderExt, process::Stdio, time::Duration};
use tokio::{
    io::AsyncWriteExt,
    process::{Child, Command},
};

/// Shared with the Claude sign-in worker (same image, same private X contract).
pub(in crate::browser) struct Display(Child);
impl Display {
    pub async fn start() -> Result<Self, Error> {
        let root = "/tmp/moosuri-google-display";
        // A fresh container has no old display. Never adopt or remove another one.
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(root)
            .map_err(|_| Error::Unavailable)?;
        let mut cookie = [0; 16];
        SystemRandom::new()
            .fill(&mut cookie)
            .map_err(|_| Error::Unavailable)?;
        let hex: String = cookie.iter().map(|b| format!("{b:02x}")).collect();
        let mut auth = Command::new("/usr/bin/xauth")
            .env_clear()
            .args([
                "-f",
                "/tmp/moosuri-google-display/Xauthority",
                "source",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Error::Unavailable)?;
        let mut input = auth.stdin.take().ok_or(Error::Unavailable)?;
        input
            .write_all(format!("add :99 . {hex}\n").as_bytes())
            .await
            .map_err(|_| Error::Unavailable)?;
        drop(input);
        if !tokio::time::timeout(Duration::from_secs(3), auth.wait())
            .await
            .map_err(|_| Error::Unavailable)?
            .map_err(|_| Error::Unavailable)?
            .success()
        {
            return Err(Error::Unavailable);
        }
        let mut command = Command::new("/usr/bin/Xvfb");
        command
            .env_clear()
            .args([
                ":99",
                "-screen",
                "0",
                "1280x900x24",
                "-nolisten",
                "tcp",
                "-auth",
                "/tmp/moosuri-google-display/Xauthority",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .process_group(0);
        let mut display = Self(command.spawn().map_err(|_| Error::Unavailable)?);
        for _ in 0..30 {
            if display
                .0
                .try_wait()
                .map_err(|_| Error::Unavailable)?
                .is_some()
            {
                return Err(Error::Unavailable);
            }
            let status = Command::new("/usr/bin/xdpyinfo")
                .env_clear()
                .env("DISPLAY", ":99")
                .env("XAUTHORITY", "/tmp/moosuri-google-display/Xauthority")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(|_| Error::Unavailable)?;
            if tokio::time::timeout(Duration::from_millis(500), status.wait_with_output())
                .await
                .is_ok_and(|s| s.is_ok_and(|s| s.status.success()))
            {
                return Ok(display);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(Error::Unavailable)
    }
    pub async fn stop(&mut self) {
        let _ = self.0.kill().await;
    }
}
impl Drop for Display {
    fn drop(&mut self) {
        if let Some(pid) = self.0.id() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = self.0.start_kill();
    }
}
