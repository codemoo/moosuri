//! Explicit per-owner, metadata-only GWS helper. No ambient Google credentials.
use crate::model::CancellationToken;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct GwsInboxRegistry {
    path: PathBuf,
    client: reqwest::Client,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::post};
    fn private(path: &Path, value: &str) {
        std::fs::write(path, value).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn registry(dir: &Path, endpoint: &str) -> (GwsInboxRegistry, PathBuf, PathBuf) {
        let key = dir.join("key");
        private(&key, &"a".repeat(64));
        let config = dir.join("registry.json");
        private(&config,&serde_json::json!([{"account_id":"00000000-0000-0000-0000-000000000001","mailbox":"owner@example.test","endpoint":endpoint,"key_file":key}]).to_string());
        (
            GwsInboxRegistry::from_file(config.clone()).unwrap(),
            config,
            key,
        )
    }
    #[test]
    fn gws_registry_rejects_public_files_symlinks_rotation_and_non_loopback() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, path, key) = registry(dir.path(), "http://127.0.0.1:3221/v1/inbox/latest");
        assert!(registry.mailbox("foreign-account").unwrap().is_none());
        let binding = registry
            .binding("00000000-0000-0000-0000-000000000001")
            .unwrap()
            .unwrap();
        private(&key, &"b".repeat(64));
        assert!(registry.check(&binding).is_err());
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            registry
                .binding("00000000-0000-0000-0000-000000000001")
                .is_err()
        );
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&path, &link).unwrap();
        assert!(GwsInboxRegistry::from_file(link).is_err());
        let bad = std::fs::read_to_string(&path)
            .unwrap()
            .replace("127.0.0.1", "192.0.2.1");
        private(&path, &bad);
        assert!(registry.entries().is_err());
    }
    #[tokio::test]
    async fn gws_http_accepts_escaped_bounded_metadata_and_rejects_oversized_envelopes() {
        let mode = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let shared = mode.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1/inbox/latest", listener.local_addr().unwrap());
        let app=Router::new().route("/v1/inbox/latest",post(move|| {let shared=shared.clone();async move {
            if shared.load(std::sync::atomic::Ordering::SeqCst) {return " ".repeat(131073);}
            serde_json::json!({"mailbox":"owner@example.test","messages":(0..20).map(|i|serde_json::json!({"id":format!("{i:x}"),"from":"\"".repeat(1024),"subject":"\\".repeat(1024),"date":"\"".repeat(512),"internal_date":i})).collect::<Vec<_>>()}).to_string()
        }}));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let dir = tempfile::tempdir().unwrap();
        let (registry, _, _) = registry(dir.path(), &endpoint);
        let binding = registry
            .binding("00000000-0000-0000-0000-000000000001")
            .unwrap()
            .unwrap();
        assert_eq!(
            registry
                .latest(&binding, &CancellationToken::new())
                .await
                .unwrap()
                .len(),
            20
        );
        mode.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            registry
                .latest(&binding, &CancellationToken::new())
                .await
                .is_err()
        );
        server.abort();
    }
}
#[derive(Clone, Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    account_id: String,
    mailbox: String,
    endpoint: String,
    key_file: PathBuf,
}
pub struct GwsInboxBinding {
    entry: Entry,
    key: Zeroizing<String>,
    pin: [u8; 32],
}
impl GwsInboxBinding {
    pub fn mailbox(&self) -> &str {
        &self.entry.mailbox
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InboxMetadata {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub internal_date: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    mailbox: String,
    messages: Vec<InboxMetadata>,
}

fn private_file(path: &Path, limit: u64) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if !path.is_absolute() {
        return Err("gws_inbox_private_file_required");
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "gws_inbox_file_unavailable")?;
    let m = file.metadata().map_err(|_| "gws_inbox_file_unavailable")?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.permissions().mode() & 0o077 != 0
        || m.len() > limit
    {
        return Err("gws_inbox_private_file_required");
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.by_ref()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "gws_inbox_file_unavailable")?;
    if bytes.len() as u64 > limit {
        return Err("gws_inbox_file_too_large");
    }
    Ok(bytes)
}
impl GwsInboxRegistry {
    pub fn from_file(path: PathBuf) -> Result<Self, &'static str> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(50))
            .build()
            .map_err(|_| "gws_inbox_client_unavailable")?;
        let registry = Self { path, client };
        let entries = registry.entries()?;
        for entry in entries {
            registry
                .binding(&entry.account_id)?
                .ok_or("gws_inbox_invalid_configuration")?;
        }
        Ok(registry)
    }
    fn entries(&self) -> Result<Vec<Entry>, &'static str> {
        let bytes = private_file(&self.path, 65536)?;
        let entries: Vec<Entry> =
            serde_json::from_slice(&bytes).map_err(|_| "gws_inbox_invalid_configuration")?;
        let mut owners = std::collections::HashSet::new();
        if entries.len() > 32 {
            return Err("gws_inbox_invalid_configuration");
        }
        for e in &entries {
            let url =
                url::Url::parse(&e.endpoint).map_err(|_| "gws_inbox_invalid_configuration")?;
            if uuid::Uuid::parse_str(&e.account_id)
                .map(|u| u.to_string() != e.account_id)
                .unwrap_or(true)
                || !owners.insert(&e.account_id)
                || (e.mailbox.is_empty()
                    || e.mailbox.len() > 254
                    || !e.mailbox.contains('@')
                    || e.mailbox
                        .chars()
                        .any(|c| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>')))
                || url.scheme() != "http"
                || url.host_str() != Some("127.0.0.1")
                || url.port().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/v1/inbox/latest"
            {
                return Err("gws_inbox_invalid_configuration");
            }
        }
        Ok(entries)
    }
    /// Current owner's public UI descriptor only; never checks or exposes a key.
    pub fn mailbox(&self, owner: &str) -> Result<Option<String>, &'static str> {
        Ok(self
            .entries()?
            .into_iter()
            .find(|e| e.account_id == owner)
            .map(|e| e.mailbox))
    }
    pub fn binding(&self, owner: &str) -> Result<Option<GwsInboxBinding>, &'static str> {
        let Some(entry) = self.entries()?.into_iter().find(|e| e.account_id == owner) else {
            return Ok(None);
        };
        let bytes = private_file(&entry.key_file, 128)?;
        let key = std::str::from_utf8(&bytes)
            .map_err(|_| "gws_inbox_invalid_key")?
            .strip_suffix('\n')
            .unwrap_or(std::str::from_utf8(&bytes).map_err(|_| "gws_inbox_invalid_key")?);
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("gws_inbox_invalid_key");
        }
        let mut digest = Sha256::new();
        digest.update(serde_json::to_vec(&entry).map_err(|_| "gws_inbox_invalid_configuration")?);
        digest.update(key.as_bytes());
        Ok(Some(GwsInboxBinding {
            entry,
            key: Zeroizing::new(key.into()),
            pin: digest.finalize().into(),
        }))
    }
    pub fn check(&self, binding: &GwsInboxBinding) -> Result<(), &'static str> {
        if self
            .binding(&binding.entry.account_id)?
            .is_none_or(|current| current.pin != binding.pin)
        {
            return Err("gws_inbox_binding_changed");
        }
        Ok(())
    }
    pub async fn latest(
        &self,
        binding: &GwsInboxBinding,
        cancel: &CancellationToken,
    ) -> Result<Vec<InboxMetadata>, &'static str> {
        self.check(binding)?;
        let operation = async {
            let mut response = self
                .client
                .post(&binding.entry.endpoint)
                .bearer_auth(&*binding.key)
                .json(&serde_json::json!({}))
                .send()
                .await
                .map_err(|_| "gws_inbox_unavailable")?;
            if response.status() != reqwest::StatusCode::OK
                || response.content_length().is_some_and(|n| n > 131072)
            {
                return Err("gws_inbox_invalid_response");
            }
            let mut bytes = Zeroizing::new(Vec::new());
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "gws_inbox_unavailable")?
            {
                if bytes.len() + chunk.len() > 131072 {
                    return Err("gws_inbox_invalid_response");
                }
                bytes.extend_from_slice(&chunk);
            }
            let reply: Reply =
                serde_json::from_slice(&bytes).map_err(|_| "gws_inbox_invalid_response")?;
            if reply.mailbox != binding.entry.mailbox || reply.messages.len() > 20 {
                return Err("gws_inbox_invalid_response");
            }
            let mut ids = std::collections::HashSet::new();
            for m in &reply.messages {
                if m.id.is_empty()
                    || m.id.len() > 128
                    || m.from.len() > 1024
                    || m.subject.len() > 1024
                    || m.date.len() > 512
                    || m.internal_date < 0
                    || !ids.insert(&m.id)
                    || [&m.id, &m.from, &m.subject, &m.date]
                        .iter()
                        .any(|s| s.chars().any(char::is_control))
                {
                    return Err("gws_inbox_invalid_response");
                }
            }
            self.check(binding)?;
            Ok(reply.messages)
        };
        tokio::select! { biased; _=cancel.cancelled()=>Err("gws_inbox_cancelled"), result=operation=>result }
    }
}
