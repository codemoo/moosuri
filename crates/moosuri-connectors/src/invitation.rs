//! Optional operator-owned native gws adapter. No ambient auth or arbitrary command.
use crate::gmail::FrozenMail;
use sha2::{Digest, Sha256};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::AsyncReadExt;

#[derive(Clone)]
pub struct GwsInvitationSender {
    executable: PathBuf,
    credentials: PathBuf,
    config_dir: PathBuf,
    from: String,
    pin: String,
    credentials_fingerprint: [u8; 32],
}
pub enum InvitationSendResult {
    Sent(String),
    DefiniteFailure,
    Unknown,
}
fn owner_file(path: &Path, executable: bool) -> Result<(), &'static str> {
    let m = fs::symlink_metadata(path).map_err(|_| "invitation_sender_path_unavailable")?;
    if !path.is_absolute()
        || !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || (!executable && m.permissions().mode() & 0o077 != 0)
        || (executable && m.permissions().mode() & 0o022 != 0)
    {
        return Err("invitation_sender_requires_owned_regular_paths");
    }
    Ok(())
}
impl GwsInvitationSender {
    pub fn new(
        executable: PathBuf,
        credentials: PathBuf,
        config_dir: PathBuf,
        from: String,
    ) -> Result<Self, &'static str> {
        owner_file(&executable, true)?;
        owner_file(&credentials, false)?;
        let mut magic = [0; 4];
        std::fs::File::open(&executable)
            .and_then(|mut f| f.read_exact(&mut magic))
            .map_err(|_| "invitation_native_gws_required")?;
        if magic != [0x7f, b'E', b'L', b'F']
            && magic != [0xcf, 0xfa, 0xed, 0xfe]
            && magic != [0xfe, 0xed, 0xfa, 0xcf]
            && magic != [0xca, 0xfe, 0xba, 0xbe]
            && magic != [0xca, 0xfe, 0xba, 0xbf]
        {
            return Err("invitation_native_gws_required");
        }

        let m = fs::symlink_metadata(&config_dir)
            .map_err(|_| "invitation_sender_config_unavailable")?;
        if !config_dir.is_absolute()
            || !m.is_dir()
            || m.uid() != unsafe { libc::geteuid() }
            || m.permissions().mode() & 0o077 != 0
        {
            return Err("invitation_sender_requires_private_config_dir");
        }
        // Validate the exact displayed sender using the shared outbound validator.
        crate::gmail::FrozenMail::prepare(
            &crate::gmail::MailContent {
                from: from.clone(),
                to: vec!["validation@example.invalid".into()],
                cc: vec![],
                bcc: vec![],
                subject: String::new(),
                text: String::new(),
                reply: None,
            },
            "validation@moosuri.local",
        )
        .map_err(|_| "invitation_sender_invalid_address")?;
        let credential_bytes = zeroize::Zeroizing::new(
            fs::read(&credentials).map_err(|_| "invitation_credentials_unavailable")?,
        );
        if credential_bytes.is_empty() || credential_bytes.len() > 65536 {
            return Err("invitation_credentials_invalid_size");
        }
        let credentials_fingerprint: [u8; 32] = Sha256::digest(&*credential_bytes).into();
        let pin = Sha256::digest(
            serde_json::to_vec(&(
                &executable,
                &credentials,
                &config_dir,
                &from,
                credentials_fingerprint,
            ))
            .map_err(|_| "invitation_sender_invalid")?,
        )
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
        // Only remove our own bounded exact staging shape, never arbitrary config.
        for entry in fs::read_dir(&config_dir).map_err(|_| "invitation_staging_unavailable")? {
            let e = entry.map_err(|_| "invitation_staging_unavailable")?;
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(suffix) = name.strip_prefix("moosuri-invitation-")
                && uuid::Uuid::parse_str(suffix).is_ok()
            {
                let m =
                    fs::symlink_metadata(e.path()).map_err(|_| "invitation_staging_unavailable")?;
                if m.is_dir()
                    && m.uid() == unsafe { libc::geteuid() }
                    && m.permissions().mode() & 0o077 == 0
                {
                    drop(Stage(e.path()));
                }
            }
        }
        Ok(Self {
            executable,
            credentials,
            config_dir,
            from,
            pin,
            credentials_fingerprint,
        })
    }
    pub fn from(&self) -> &str {
        &self.from
    }
    pub fn pin(&self) -> &str {
        &self.pin
    }
    fn private_command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.executable);
        cmd.env_clear()
            .env("HOME", &self.config_dir)
            .env("GOOGLE_WORKSPACE_CLI_CONFIG_DIR", &self.config_dir)
            .env("GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE", &self.credentials)
            .env("GOOGLE_WORKSPACE_CLI_LOG", "off")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        cmd
    }
    fn credentials_current(&self) -> bool {
        if owner_file(&self.credentials, false).is_err() {
            return false;
        }
        let Ok(bytes) = fs::read(&self.credentials).map(zeroize::Zeroizing::new) else {
            return false;
        };
        if bytes.len() > 65536
            || <[u8; 32]>::from(Sha256::digest(&*bytes)) != self.credentials_fingerprint
        {
            return false;
        }
        true
    }
    pub async fn ready(&self) -> bool {
        if !self.credentials_current() {
            return false;
        }
        let mut cmd = self.private_command();
        cmd.arg("--version");
        let Ok(mut child) = cmd.spawn() else {
            return false;
        };
        matches!(
            tokio::time::timeout(std::time::Duration::from_secs(3), async {
                let mut bytes = Vec::new();
                child
                    .stdout
                    .take()
                    .ok_or(())?
                    .take(4097)
                    .read_to_end(&mut bytes)
                    .await
                    .map_err(|_| ())?;
                if bytes.len() > 4096 || !child.wait().await.map_err(|_| ())?.success() {
                    return Err(());
                }
                Ok(String::from_utf8(bytes).map_err(|_| ())?.lines().next() == Some("gws 0.22.5"))
            })
            .await,
            Ok(Ok(true))
        )
    }
    /// Trusted caller: await ready(), then commit/revalidate current authority,
    /// then call this method. No awaited preflight may separate that final
    /// authority check from the native send-process spawn.
    pub async fn send_ready(&self, mail: &FrozenMail) -> InvitationSendResult {
        if !self.credentials_current() {
            return InvitationSendResult::DefiniteFailure;
        }
        // --upload/--upload-content-type are documented gws native media-input flags.
        // The CLI receives only a private path; MIME/password never enters argv/env.
        // Credential input is explicitly configured; every ambient variable is removed.
        if owner_file(&self.executable, true).is_err()
            || owner_file(&self.credentials, false).is_err()
        {
            return InvitationSendResult::DefiniteFailure;
        }
        let Ok(metadata) = fs::symlink_metadata(&self.config_dir) else {
            return InvitationSendResult::DefiniteFailure;
        };
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.permissions().mode() & 0o077 != 0
        {
            return InvitationSendResult::DefiniteFailure;
        }
        let directory = self
            .config_dir
            .join(format!("moosuri-invitation-{}", uuid::Uuid::new_v4()));
        if std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .is_err()
        {
            return InvitationSendResult::DefiniteFailure;
        }
        let cleanup = Stage(directory.clone());
        let path = directory.join("payload.mime");
        let mut file = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(f) => f,
            Err(_) => return InvitationSendResult::DefiniteFailure,
        };
        if file.write_all(mail.raw_mime()).is_err() {
            return InvitationSendResult::DefiniteFailure;
        }
        drop(file);
        let mut cmd = self.private_command();
        cmd.args([
            "gmail",
            "users",
            "messages",
            "send",
            "--params",
            r#"{"userId":"me"}"#,
            "--upload",
        ])
        .arg("payload.mime")
        .args([
            "--upload-content-type",
            "message/rfc822",
            "--format",
            "json",
        ])
        .current_dir(&directory)
        .env("GOOGLE_WORKSPACE_CLI_LOG", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(_) => return InvitationSendResult::DefiniteFailure,
        };
        let result = tokio::time::timeout(std::time::Duration::from_secs(35), async {
            let mut bytes = zeroize::Zeroizing::new(Vec::new());
            child
                .stdout
                .take()
                .ok_or(())?
                .take(16385)
                .read_to_end(&mut bytes)
                .await
                .map_err(|_| ())?;
            if bytes.len() > 16384 {
                return Err(());
            }
            let status = child.wait().await.map_err(|_| ())?;
            if !status.success() {
                return Err(());
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
            let id = value.get("id").and_then(|s| s.as_str()).ok_or(())?;
            if id.is_empty()
                || id.len() > 256
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            {
                return Err(());
            }
            Ok(id.to_owned())
        })
        .await;
        drop(child);
        drop(cleanup);
        match result {
            Ok(Ok(id)) => InvitationSendResult::Sent(id),
            _ => InvitationSendResult::Unknown,
        }
    }
}
struct Stage(PathBuf);
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("payload.mime"));
        let _ = fs::remove_dir(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PathBuf, GwsInvitationSender) {
        let dir =
            std::env::temp_dir().join(format!("moosuri-gws-fixture-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let executable = dir.join("native-fixture");
        fs::copy("/usr/bin/false", &executable).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let credentials = dir.join("synthetic-credentials.json");
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&credentials)
            .unwrap();
        f.write_all(br#"{"synthetic_fixture_only":true}"#).unwrap();
        let sender = GwsInvitationSender::new(
            executable,
            credentials,
            dir.clone(),
            "system@example.test".into(),
        )
        .unwrap();
        (dir, sender)
    }
    #[tokio::test]
    async fn invitation_native_process_never_gets_body_in_arguments_and_failed_process_cleans_stage()
     {
        let (dir, sender) = fixture();
        let command = sender.private_command();
        let environments = command
            .as_std()
            .get_envs()
            .filter_map(|(key, value)| value.map(|_| key.to_string_lossy().into_owned()))
            .collect::<Vec<_>>();
        assert!(environments.iter().all(|k| {
            [
                "HOME",
                "GOOGLE_WORKSPACE_CLI_CONFIG_DIR",
                "GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE",
                "GOOGLE_WORKSPACE_CLI_LOG",
            ]
            .contains(&k.as_str())
        }));
        let mail = FrozenMail::prepare(
            &crate::gmail::MailContent {
                from: "system@example.test".into(),
                to: vec!["invitee@example.test".into()],
                cc: vec![],
                bcc: vec![],
                subject: "Synthetic invitation".into(),
                text: "Synthetic-one-time-password".into(),
                reply: None,
            },
            "synthetic@example.test",
        )
        .unwrap();
        assert!(
            !sender.ready().await,
            "unsupported native fixture version must fail readiness"
        );
        assert!(
            matches!(
                sender.send_ready(&mail).await,
                InvitationSendResult::Unknown
            ),
            "every failure after spawn stays unknown"
        );
        assert!(!fs::read_dir(&dir).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("moosuri-invitation-")
        }));
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn invitation_startup_only_cleans_owned_exact_private_stages() {
        let (dir, sender) = fixture();
        let stage = dir.join(format!("moosuri-invitation-{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&stage).unwrap();
        fs::write(stage.join("payload.mime"), b"synthetic-only").unwrap();
        let unrelated = dir.join("keep-this.txt");
        fs::write(&unrelated, b"synthetic unrelated config").unwrap();
        let _sender = GwsInvitationSender::new(
            sender.executable,
            sender.credentials,
            dir.clone(),
            sender.from,
        )
        .unwrap();
        assert!(!stage.exists());
        assert!(unrelated.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
