use super::*;
use reqwest::{Client, Method, header};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    time::Duration,
};
use url::Url;
use zeroize::Zeroizing;
#[derive(Deserialize)]
pub(super) struct Status {
    #[serde(default)]
    pub start_binding: Option<String>,
    #[serde(default)]
    pub run_generation: Option<i64>,
    pub connection_id: String,
    pub config_revision: i64,
    pub local_sequence: i64,
    pub checkpoint: Option<String>,
    pub active_run: Option<ActiveRun>,
}
#[derive(Deserialize)]
pub(super) struct ActiveRun {
    id: String,
    generation: i64,
}
pub(super) struct Http {
    origin: Url,
    client: Client,
    authorization: header::HeaderValue,
}
impl Http {
    pub fn origin(&self) -> &str {
        self.origin.as_str()
    }
    pub fn new(input: &str, token_path: &Path, loopback_fixture: bool) -> Result<Self> {
        let origin = Url::parse(input).map_err(|_| "invalid_https_origin")?;
        let permitted = origin.scheme() == "https"
            || loopback_fixture
                && origin.scheme() == "http"
                && origin.host_str() == Some("127.0.0.1");
        if !permitted
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.path() != "/"
            || origin.query().is_some()
            || origin.fragment().is_some()
        {
            return Err("exact_https_origin_required");
        }
        let token = read_private(token_path, 128)?.ok_or("token_file_missing")?;
        let token = std::str::from_utf8(&token)
            .map_err(|_| "invalid_token_file")?
            .trim();
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("invalid_token_file");
        }
        let mut authorization = header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "invalid_token_file")?;
        authorization.set_sensitive(true);
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(25))
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| "http_client_unavailable")?;
        Ok(Self {
            origin,
            client,
            authorization,
        })
    }
    async fn request(
        &self,
        method: Method,
        suffix: &str,
        body: Option<&str>,
    ) -> Result<(u16, Zeroizing<Vec<u8>>)> {
        let url = self
            .origin
            .join(&format!("api/v1/source-ingest/{suffix}"))
            .map_err(|_| "invalid_ingest_route")?;
        if url.origin() != self.origin.origin() {
            return Err("ingest_origin_mismatch");
        }
        let mut request = self
            .client
            .request(method, url)
            .header(header::AUTHORIZATION, self.authorization.clone())
            .header(header::ACCEPT, "application/json");
        if let Some(body) = body {
            request = request
                .header(header::CONTENT_TYPE, "application/json")
                .body(body.to_owned());
        }
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut response = request
                .send()
                .await
                .map_err(|_| "request_outcome_unknown_keep_state")?;
            let status = response.status().as_u16();
            if response.content_length().is_some_and(|n| n > 128 * 1024) {
                return Err("response_limit_keep_state");
            }
            let mut bytes = Zeroizing::new(Vec::new());
            while let Some(chunk) = response
                .chunk()
                .await
                .map_err(|_| "response_outcome_unknown_keep_state")?
            {
                if bytes.len().saturating_add(chunk.len()) > 128 * 1024 {
                    return Err("response_limit_keep_state");
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok((status, bytes))
        })
        .await
        .map_err(|_| "request_timeout_outcome_unknown_keep_state")?
    }
    pub async fn status(&self) -> Result<Status> {
        let (code, body) = self.request(Method::GET, "status", None).await?;
        if code != 200 {
            return Err("status_denied_or_unavailable");
        }
        serde_json::from_slice(&body).map_err(|_| "invalid_ingest_status")
    }
    pub async fn start(&self, intent: &SourceRunStartIntent) -> Result<SourceRunStartReceipt> {
        let body = serde_json::to_string(&moosuri_core::SourceRunStart {
            intent: Some(intent.clone()),
        })
        .map_err(|_| "start_intent_encode_failed")?;
        let (code, body) = self.request(Method::POST, "runs", Some(&body)).await?;
        if code != 201 {
            return Err("run_start_unconfirmed_retry_same_intent");
        }
        let ack: SourceRunStartReceipt =
            serde_json::from_slice(&body).map_err(|_| "invalid_run_ack_keep_state")?;
        if ack.request_id != intent.request_id
            || ack.expected_run_generation != intent.expected_run_generation
            || Some(ack.generation) != intent.expected_run_generation.checked_add(1)
            || Uuid::parse_str(&ack.run_id).is_err()
        {
            return Err("run_start_identity_mismatch_keep_state");
        }
        Ok(ack)
    }
    pub async fn receipt(&self, id: &str) -> Result<Option<SourceBatchReceipt>> {
        Uuid::parse_str(id).map_err(|_| "invalid_saved_request_id")?;
        let (code, body) = self
            .request(Method::GET, &format!("receipts/{id}"), None)
            .await?;
        match code {
            404 => Ok(None),
            200 => serde_json::from_slice(&body)
                .map(Some)
                .map_err(|_| "invalid_receipt_keep_state"),
            _ => Err("receipt_denied_or_unavailable_keep_state"),
        }
    }
    pub fn same_run(&self, run: &Run, status: &Status) -> bool {
        status
            .active_run
            .as_ref()
            .is_some_and(|r| r.id == run.run_id && r.generation == run.generation)
    }
    pub async fn batch(&self, body: &str) -> Result<()> {
        let (code, _) = self.request(Method::POST, "batches", Some(body)).await?;
        if code != 200 && code != 201 {
            return Err("batch_unconfirmed_keep_state");
        }
        Ok(())
    }
    pub async fn finish(&self, run: &Run) -> Result<()> {
        let body=serde_json::json!({"run_id":run.run_id,"generation":run.generation,"outcome":"partial","error":null,"retry_after_seconds":null}).to_string();
        let (code, _) = self.request(Method::POST, "finish", Some(&body)).await?;
        if code != 204 {
            return Err("batch_committed_finish_unconfirmed_keep_state");
        }
        Ok(())
    }
}
fn private(file: &File) -> Result<()> {
    let m = file
        .metadata()
        .map_err(|_| "private_file_metadata_failed")?;
    // SAFETY: geteuid has no arguments or memory contract.
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
    {
        return Err("file_must_be_owner_only_regular_without_links");
    }
    Ok(())
}
pub(super) fn read_private(path: &Path, limit: usize) -> Result<Option<Zeroizing<Vec<u8>>>> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("private_file_open_failed"),
    };
    private(&file)?;
    if file
        .metadata()
        .map_err(|_| "private_file_metadata_failed")?
        .len()
        > limit as u64
    {
        return Err("private_file_limit");
    }
    let mut data = Zeroizing::new(Vec::new());
    file.take(limit as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|_| "private_file_read_failed")?;
    if data.len() > limit {
        return Err("private_file_limit");
    }
    Ok(Some(data))
}
fn canonical_parent(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err("explicit_absolute_private_path_required");
    }
    let parent = path.parent().ok_or("invalid_private_path")?;
    if fs::canonicalize(parent).map_err(|_| "private_parent_missing")? != parent {
        return Err("private_parent_must_not_contain_symlinks");
    }
    super::trusted_ancestors(parent)?;
    let m = fs::symlink_metadata(parent).map_err(|_| "private_parent_missing")?;
    // SAFETY: geteuid has no arguments or memory contract.
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
        return Err("private_parent_must_be_owner_only_directory");
    }
    Ok(parent.to_owned())
}
pub(super) fn state_path(path: &Path) -> Result<PathBuf> {
    canonical_parent(path)?;
    if path.file_name().is_none() {
        return Err("invalid_state_path");
    }
    for ancestor in path.parent().ok_or("invalid_state_path")?.ancestors() {
        if ancestor.join(".git").exists() {
            return Err("state_must_be_outside_git_checkout");
        }
    }
    let _ = read_private(path, STATE_LIMIT)?;
    Ok(path.to_owned())
}
pub(super) fn credential_path(path: &Path, limit: usize) -> Result<PathBuf> {
    canonical_parent(path)?;
    let _ = read_private(path, limit)?.ok_or("credential_file_missing")?;
    Ok(path.to_owned())
}
pub(super) fn token_path(path: &Path) -> Result<PathBuf> {
    canonical_parent(path)?;
    let _ = read_private(path, 128)?.ok_or("token_file_missing")?;
    Ok(path.to_owned())
}
pub(super) fn lock(path: &Path) -> Result<File> {
    let name = path
        .file_name()
        .ok_or("invalid_state_path")?
        .to_str()
        .ok_or("invalid_state_path")?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path.with_file_name(format!(".{name}.lock")))
        .map_err(|_| "state_lock_open_failed")?;
    private(&file)?;
    // SAFETY: the descriptor is live; flock does not access memory.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("another_producer_holds_state_lock");
    }
    Ok(file)
}
pub(super) fn save(path: &Path, state: &State) -> Result<()> {
    let parent = canonical_parent(path)?;
    let _ = read_private(path, STATE_LIMIT)?;
    let bytes = Zeroizing::new(serde_json::to_vec(state).map_err(|_| "state_encode_failed")?);
    if bytes.len() > STATE_LIMIT {
        return Err("state_limit_keep_existing_file");
    }
    let temp = parent.join(format!(".local-work-session-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&temp)
            .map_err(|_| "state_create_failed")?;
        private(&file)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "state_fsync_failed")?;
        fs::rename(&temp, path).map_err(|_| "state_atomic_replace_failed")?;
        File::open(parent)
            .and_then(|f| f.sync_all())
            .map_err(|_| "state_directory_fsync_failed")
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
