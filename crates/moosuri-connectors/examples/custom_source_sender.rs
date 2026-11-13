//! A synthetic, single-writer custom collector example. No provider credentials
//! or source files are read. See src/source/README.md for recovery assumptions.
#[cfg(unix)]
mod sender {
    use reqwest::{Client, Method, header};
    use serde::{Deserialize, Serialize};
    use serde_json::{Value, json};
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        os::{
            fd::AsRawFd,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
        path::{Path, PathBuf},
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    use url::Url;
    use uuid::Uuid;
    use zeroize::Zeroizing;

    type Result<T> = std::result::Result<T, &'static str>;
    const LIMIT: usize = 128 * 1024;
    #[derive(Deserialize)]
    struct Status {
        connection_id: String,
        config_revision: i64,
        local_sequence: i64,
        checkpoint: Option<String>,
        active_run: Option<ActiveRun>,
    }
    #[derive(Deserialize)]
    struct ActiveRun {
        id: String,
        generation: i64,
    }
    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Run {
        run_id: String,
        generation: i64,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct State {
        version: u32,
        origin: String,
        connection_id: String,
        config_revision: i64,
        expected_sequence: i64,
        checkpoint_before: Option<String>,
        request_id: String,
        observed_at: i64,
        run_start_attempted: bool,
        run: Option<Run>,
        /// Exact UTF-8 HTTP body, persisted before the first batch dispatch.
        envelope: Option<String>,
        committed: bool,
        finished: bool,
    }
    struct Http {
        origin: Url,
        client: Client,
        authorization: header::HeaderValue,
    }
    impl Http {
        async fn request(
            &self,
            method: Method,
            suffix: &str,
            body: Option<&str>,
        ) -> Result<(u16, Zeroizing<Vec<u8>>)> {
            let url = self
                .origin
                .join(&format!("api/v1/source-ingest/{suffix}"))
                .map_err(|_| "invalid_origin")?;
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
            // Includes DNS, headers and bounded body consumption. Provider/server
            // diagnostics never escape as reqwest/JSON display strings.
            tokio::time::timeout(Duration::from_secs(30), async {
                let mut response = request
                    .send()
                    .await
                    .map_err(|_| "request_outcome_unknown_keep_state")?;
                let status = response.status().as_u16();
                if response
                    .content_length()
                    .is_some_and(|size| size > LIMIT as u64)
                {
                    return Err("response_limit_keep_state");
                }
                let mut bytes = Zeroizing::new(Vec::new());
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|_| "response_outcome_unknown_keep_state")?
                {
                    if bytes.len().saturating_add(chunk.len()) > LIMIT {
                        return Err("response_limit_keep_state");
                    }
                    bytes.extend_from_slice(&chunk);
                }
                Ok((status, bytes))
            })
            .await
            .map_err(|_| "request_timeout_outcome_unknown_keep_state")?
        }
        async fn status(&self) -> Result<Status> {
            let (code, bytes) = self.request(Method::GET, "status", None).await?;
            if code != 200 {
                return Err("status_denied_or_unavailable");
            }
            serde_json::from_slice(&bytes).map_err(|_| "invalid_status")
        }
        async fn receipt(&self, state: &State) -> Result<bool> {
            let (code, bytes) = self
                .request(Method::GET, &format!("receipts/{}", state.request_id), None)
                .await?;
            if code == 404 {
                return Ok(false);
            }
            if code != 200 {
                return Err("receipt_denied_or_unavailable_keep_state");
            }
            #[derive(Deserialize)]
            struct Receipt {
                request_id: String,
                run_id: String,
                sequence: i64,
            }
            let receipt: Receipt =
                serde_json::from_slice(&bytes).map_err(|_| "invalid_receipt_keep_state")?;
            let run = state.run.as_ref().ok_or("missing_saved_run")?;
            if receipt.request_id != state.request_id
                || receipt.run_id != run.run_id
                || state.expected_sequence.checked_add(1) != Some(receipt.sequence)
            {
                return Err("receipt_identity_mismatch_keep_state");
            }
            Ok(true)
        }
    }
    fn private_metadata(file: &File) -> Result<()> {
        let metadata = file
            .metadata()
            .map_err(|_| "private_file_metadata_failed")?;
        // SAFETY: geteuid has no arguments or memory contract.
        let owner = unsafe { libc::geteuid() };
        if !metadata.is_file()
            || metadata.uid() != owner
            || metadata.mode() & 0o077 != 0
            || metadata.nlink() != 1
        {
            return Err("file_must_be_owner_only_regular_without_links");
        }
        Ok(())
    }
    fn read_private(path: &Path, limit: usize) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("private_file_open_failed"),
        };
        private_metadata(&file)?;
        if file
            .metadata()
            .map_err(|_| "private_file_metadata_failed")?
            .len()
            > limit as u64
        {
            return Err("private_file_too_large");
        }
        let mut bytes = Zeroizing::new(Vec::new());
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "private_file_read_failed")?;
        if bytes.len() > limit {
            return Err("private_file_too_large");
        }
        Ok(Some(bytes))
    }
    fn state_parent(path: &Path) -> Result<&Path> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let metadata = fs::metadata(parent).map_err(|_| "state_parent_missing")?;
        // SAFETY: geteuid has no arguments or memory contract.
        if !metadata.is_dir()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err("state_parent_must_be_owner_only_directory");
        }
        Ok(parent)
    }
    fn state_lock(path: &Path) -> Result<File> {
        state_parent(path)?;
        let name = path
            .file_name()
            .ok_or("invalid_state_path")?
            .to_string_lossy();
        let lock_path = path.with_file_name(format!(".{name}.lock"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(lock_path)
            .map_err(|_| "state_lock_open_failed")?;
        private_metadata(&file)?;
        // SAFETY: the file descriptor is live and flock does not access memory.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("another_sender_holds_state_lock");
        }
        Ok(file)
    }
    fn save(path: &Path, state: &State) -> Result<()> {
        let parent = state_parent(path)?;
        // Reject a replaced symlink/non-private state before atomic replacement.
        let _existing = read_private(path, LIMIT)?;
        let temporary = parent.join(format!(".synthetic-source-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&temporary)
                .map_err(|_| "state_create_failed")?;
            private_metadata(&file)?;
            let bytes =
                Zeroizing::new(serde_json::to_vec(state).map_err(|_| "state_encode_failed")?);
            if bytes.len() > LIMIT {
                return Err("state_limit");
            }
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|_| "state_fsync_failed")?;
            fs::rename(&temporary, path).map_err(|_| "state_atomic_replace_failed")?;
            File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| "state_directory_fsync_failed")
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
    fn origin(input: &str) -> Result<Url> {
        let url = Url::parse(input).map_err(|_| "invalid_origin")?;
        let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if !(url.scheme() == "https" || url.scheme() == "http" && loopback)
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("https_origin_required_loopback_http_only_for_tests");
        }
        Ok(url)
    }
    fn same_binding(state: &State, origin: &Url, status: &Status) -> Result<()> {
        if state.version != 1
            || state.origin != origin.as_str()
            || state.connection_id != status.connection_id
            || state.config_revision != status.config_revision
            || state.expected_sequence < 0
            || Uuid::parse_str(&state.request_id).is_err()
        {
            return Err("saved_connection_or_config_mismatch_keep_state");
        }
        Ok(())
    }
    fn same_position(state: &State, status: &Status) -> Result<()> {
        if state.expected_sequence != status.local_sequence
            || state.checkpoint_before != status.checkpoint
        {
            return Err("saved_position_changed_check_receipt_keep_state");
        }
        Ok(())
    }
    fn same_run(run: &Run, status: &Status) -> bool {
        status
            .active_run
            .as_ref()
            .is_some_and(|active| active.id == run.run_id && active.generation == run.generation)
    }
    fn prepare(state: &State) -> Result<String> {
        let run = state.run.as_ref().ok_or("missing_saved_run")?;
        serde_json::to_string(&json!({
            "protocol_version":1,"request_id":state.request_id,"config_revision":state.config_revision,
            "run_id":run.run_id,"generation":run.generation,"expected_sequence":state.expected_sequence,
            "checkpoint_before":state.checkpoint_before,"checkpoint_after":format!("synthetic-example:{}",state.request_id),
            "entries":[{"external_id":"synthetic-example-note","version":state.request_id,
                "expected_revision":null,"title":"Synthetic custom collector example",
                "text":"SYNTHETIC EXAMPLE ONLY: this note demonstrates custom HTTP ingestion. It is not collected from a real service.",
                "speaker":"synthetic example","observed_at":state.observed_at,"valid_from":null,"valid_until":null}],
            "complete_snapshot":false,"tombstones":[],"metadata":[]
        })).map_err(|_| "envelope_encode_failed")
    }
    pub async fn run() -> Result<()> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        if args.len() != 3 {
            return Err(
                "usage_custom_source_sender_server_origin_private_token_file_private_state_file",
            );
        }
        let origin = origin(args[0].to_str().ok_or("invalid_origin")?)?;
        let token_path = PathBuf::from(&args[1]);
        let state_path = PathBuf::from(&args[2]);
        if token_path == state_path {
            return Err("token_and_state_paths_must_differ");
        }
        let _lock = state_lock(&state_path)?;
        let token = read_private(&token_path, 128)?.ok_or("token_file_missing")?;
        let token = std::str::from_utf8(&token)
            .map_err(|_| "invalid_token_file")?
            .trim();
        if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("invalid_token_file");
        }
        let mut authorization = header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "invalid_token_file")?;
        authorization.set_sensitive(true);
        let http = Http {
            origin,
            authorization,
            client: Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .connect_timeout(Duration::from_secs(5))
                .read_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(25))
                .pool_max_idle_per_host(0)
                .build()
                .map_err(|_| "http_client_failed")?,
        };
        let status = http.status().await?;
        let mut state = if let Some(bytes) = read_private(&state_path, LIMIT)? {
            serde_json::from_slice::<State>(&bytes).map_err(|_| "invalid_saved_state_keep_file")?
        } else {
            if status.active_run.is_some() {
                return Err("existing_active_run_use_its_producer_do_not_hijack");
            }
            let state = State {
                version: 1,
                origin: http.origin.to_string(),
                connection_id: status.connection_id.clone(),
                config_revision: status.config_revision,
                expected_sequence: status.local_sequence,
                checkpoint_before: status.checkpoint.clone(),
                request_id: Uuid::new_v4().to_string(),
                observed_at: i64::try_from(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| "invalid_clock")?
                        .as_secs(),
                )
                .map_err(|_| "invalid_clock")?,
                run_start_attempted: false,
                run: None,
                envelope: None,
                committed: false,
                finished: false,
            };
            save(&state_path, &state)?;
            state
        };
        same_binding(&state, &http.origin, &status)?;
        if state.run.is_none() {
            same_position(&state, &status)?;
            if let Some(active) = status.active_run.as_ref() {
                if !state.run_start_attempted {
                    return Err("unrelated_active_run_keep_state");
                }
                // A persisted intent plus the documented single-producer contract
                // permits readback after a lost run-start acknowledgement.
                state.run = Some(Run {
                    run_id: active.id.clone(),
                    generation: active.generation,
                });
            } else {
                if state.run_start_attempted {
                    return Err("run_start_outcome_unknown_no_active_run_manual_review_keep_state");
                }
                state.run_start_attempted = true;
                save(&state_path, &state)?;
                let response = http.request(Method::POST, "runs", Some("{}")).await;
                state.run = match response {
                    Ok((201, bytes)) => Some(
                        serde_json::from_slice(&bytes).map_err(|_| "invalid_run_ack_keep_state")?,
                    ),
                    _ => {
                        let current = http.status().await?;
                        same_binding(&state, &http.origin, &current)?;
                        same_position(&state, &current)?;
                        Some(
                            current
                                .active_run
                                .map(|active| Run {
                                    run_id: active.id,
                                    generation: active.generation,
                                })
                                .ok_or("run_start_outcome_unknown_keep_state")?,
                        )
                    }
                };
            }
            save(&state_path, &state)?;
        }
        if state.envelope.is_none() {
            let current = http.status().await?;
            same_binding(&state, &http.origin, &current)?;
            same_position(&state, &current)?;
            if !same_run(state.run.as_ref().ok_or("missing_saved_run")?, &current) {
                return Err("saved_run_not_active_keep_state");
            }
            state.envelope = Some(prepare(&state)?);
            save(&state_path, &state)?;
        }
        // Validate the saved immutable bytes against their pinned intent. This
        // is verification only; dispatch always uses the original saved bytes.
        let body = state.envelope.as_deref().ok_or("missing_saved_envelope")?;
        let envelope: Value =
            serde_json::from_str(body).map_err(|_| "invalid_saved_envelope_keep_state")?;
        let expected: Value = serde_json::from_str(&prepare(&state)?)
            .map_err(|_| "invalid_saved_envelope_keep_state")?;
        if envelope != expected {
            return Err("saved_envelope_intent_mismatch_keep_state");
        }
        if !http.receipt(&state).await? {
            if state.committed || state.finished {
                return Err("saved_receipt_no_longer_available_keep_state");
            }
            let current = http.status().await?;
            same_binding(&state, &http.origin, &current)?;
            same_position(&state, &current)?;
            if !same_run(state.run.as_ref().ok_or("missing_saved_run")?, &current) {
                return Err("saved_run_not_active_no_new_request_keep_state");
            }
            // A timeout or lost acknowledgement never changes the UUID/body.
            let _response = http.request(Method::POST, "batches", Some(body)).await;
            if !http.receipt(&state).await? {
                return Err("batch_outcome_unknown_retry_same_state_file");
            }
        }
        state.committed = true;
        save(&state_path, &state)?;
        if !state.finished {
            let run = state.run.as_ref().ok_or("missing_saved_run")?;
            let finish=json!({"run_id":run.run_id,"generation":run.generation,"outcome":"completed","error":null,"retry_after_seconds":null}).to_string();
            let (code, _) = http.request(Method::POST, "finish", Some(&finish)).await?;
            if code != 204 {
                return Err("batch_committed_finish_unconfirmed_keep_state");
            }
            state.finished = true;
            save(&state_path, &state)?;
        }
        // Completion is a producer report; the receipt attests the local commit.
        println!("synthetic_example_committed {}", state.request_id);
        Ok(())
    }
}
#[cfg(unix)]
#[tokio::main]
async fn main() {
    if let Err(code) = sender::run().await {
        eprintln!("{code}");
        std::process::exit(1);
    }
}
#[cfg(not(unix))]
fn main() {
    eprintln!("example_requires_unix_private_file_permissions");
    std::process::exit(1);
}
