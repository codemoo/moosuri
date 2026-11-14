//! Explicit, once-only local Codex user-request ingestion. No session discovery defaults.
use moosuri_core::{
    SourceBatch, SourceBatchReceipt, SourceInput, SourceObjectMetadata, SourceRunStartIntent,
    SourceRunStartReceipt,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
#[path = "calls/scan.rs"]
mod call_files;
mod files;
mod start_recovery;
#[cfg(test)]
mod tests;
mod transport;
pub type Result<T> = std::result::Result<T, &'static str>;
const STATE_LIMIT: usize = 2 * 1024 * 1024;
const MAX_OBJECTS: usize = 8192;
const MAX_ENTRIES: usize = 16;
pub struct Options {
    pub directory: PathBuf,
    pub origin: String,
    pub connection_id: String,
    pub token_file: PathBuf,
    pub state_file: PathBuf,
}
#[derive(Serialize)]
pub struct Receipt {
    pub status: &'static str,
    pub request_id: Option<String>,
    pub accepted: u32,
    pub unchanged: u32,
    pub examined_files: u32,
    pub deferred_files: u32,
    pub unsupported_files: u32,
    /// A bounded scan is never proof of complete external history.
    pub producer_report: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transcription: Option<crate::calls::Counts>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Run {
    run_id: String,
    generation: i64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fingerprint {
    device: u64,
    inode: u64,
    bytes: u64,
    modified_sec: i64,
    modified_nsec: i64,
    sha256: String,
}
fn codex_producer() -> String {
    "local-codex-v1".into()
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    version: u32,
    #[serde(default = "codex_producer")]
    producer: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    transcriptions: BTreeMap<String, crate::calls::Transcription>,
    origin: String,
    connection_id: String,
    config_revision: i64,
    root_digest: String,
    sequence: i64,
    checkpoint: Option<String>,
    scan_after: Option<String>,
    files: BTreeMap<String, Fingerprint>,
    objects: BTreeMap<String, String>,
    pending: Option<Pending>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    request_id: String,
    run_start_attempted: bool,
    #[serde(default)]
    start_intent: Option<SourceRunStartIntent>,
    run: Option<Run>,
    envelope: Option<String>,
    entries: Vec<SourceInput>,
    metadata: Vec<SourceObjectMetadata>,
    files_after: BTreeMap<String, Fingerprint>,
    objects_after: BTreeMap<String, String>,
    scan_after: Option<String>,
    checkpoint_after: String,
    examined_files: u32,
    deferred_files: u32,
    unsupported_files: u32,
    committed: bool,
    #[serde(default)]
    call_report: Option<crate::calls::Counts>,
}
fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
fn now() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "clock_unavailable")?
            .as_secs(),
    )
    .map_err(|_| "clock_unavailable")
}
fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| "state_encode_failed")
}
fn binding(
    state: &State,
    options: &Options,
    origin: &str,
    status: &transport::Status,
    root: &str,
) -> Result<()> {
    if state.version != 1
        || state.origin != origin
        || state.connection_id != options.connection_id
        || state.connection_id != status.connection_id
        || state.config_revision != status.config_revision
        || state.root_digest != root
        || state.sequence < 0
        || state.files.len() > 1024
        || state.objects.len() > MAX_OBJECTS
        || state.transcriptions.len() > 64
    {
        return Err("saved_scope_or_destination_changed_keep_state");
    }
    Ok(())
}
fn position(state: &State, status: &transport::Status) -> Result<()> {
    if state.sequence != status.local_sequence || state.checkpoint != status.checkpoint {
        return Err("destination_position_changed_keep_state");
    }
    Ok(())
}
fn envelope(state: &State, p: &Pending) -> Result<String> {
    let run = p.run.as_ref().ok_or("missing_saved_run")?;
    encode(&SourceBatch {
        protocol_version: 1,
        request_id: p.request_id.clone(),
        config_revision: state.config_revision,
        run_id: run.run_id.clone(),
        generation: run.generation,
        expected_sequence: state.sequence,
        checkpoint_before: state.checkpoint.clone(),
        checkpoint_after: Some(p.checkpoint_after.clone()),
        entries: p.entries.clone(),
        metadata: p.metadata.clone(),
        tombstones: vec![],
        complete_snapshot: false,
    })
}
fn check_receipt(state: &State, p: &Pending, r: &SourceBatchReceipt) -> Result<()> {
    let run = p.run.as_ref().ok_or("missing_saved_run")?;
    if r.request_id != p.request_id
        || r.run_id != run.run_id
        || Some(r.sequence) != state.sequence.checked_add(1)
        || r.accepted.saturating_add(r.unchanged) != p.entries.len() as u32
    {
        return Err("receipt_identity_or_count_mismatch_keep_state");
    }
    Ok(())
}
fn committed_position(
    state: &State,
    pending: &Pending,
    receipt: &SourceBatchReceipt,
    current: &transport::Status,
    http: &transport::Http,
) -> Result<()> {
    if current.local_sequence != receipt.sequence
        || current.checkpoint.as_deref() != Some(pending.checkpoint_after.as_str())
    {
        return Err("committed_position_changed_keep_state");
    }
    if current.active_run.is_some()
        && !http.same_run(pending.run.as_ref().ok_or("missing_saved_run")?, current)
    {
        return Err("conflicting_active_run_keep_state");
    }
    // The receipt remains tied to precisely the old position, even if its run
    // is no longer active. It cannot authorize skipping somebody else's batch.
    if state.sequence.checked_add(1) != Some(receipt.sequence) {
        return Err("committed_position_changed_keep_state");
    }
    Ok(())
}
fn stage(state: &mut State, scan: files::Scan, status: &transport::Status) -> Result<()> {
    let added = scan
        .objects
        .keys()
        .filter(|id| !state.objects.contains_key(*id))
        .count();
    let files = scan
        .files
        .keys()
        .filter(|id| !state.files.contains_key(*id))
        .count();
    if state.objects.len().saturating_add(added) > MAX_OBJECTS
        || state.files.len().saturating_add(files) > 1024
    {
        return Err("local_cursor_limit_choose_new_connection");
    }
    let request_id = Uuid::new_v4().to_string();
    // This is an opaque commit cursor, not an assertion that all history was scanned.
    let checkpoint_after = format!(
        "{}:{}",
        state.producer,
        hash(encode(&(
            &scan.files,
            &scan.objects,
            &scan.scan_after,
            &request_id
        ))?)
    );
    state.pending = Some(Pending {
        request_id,
        run_start_attempted: false,
        start_intent: Some(SourceRunStartIntent {
            start_binding: status
                .start_binding
                .clone()
                .ok_or("start_recovery_server_upgrade_required")?,
            request_id: Uuid::new_v4().to_string(),
            config_revision: state.config_revision,
            expected_run_generation: status
                .run_generation
                .ok_or("start_recovery_server_upgrade_required")?,
            expected_sequence: state.sequence,
            checkpoint_before: state.checkpoint.clone(),
        }),
        run: None,
        envelope: None,
        entries: scan.entries,
        metadata: scan.metadata,
        files_after: scan.files,
        objects_after: scan.objects,
        scan_after: scan.scan_after,
        checkpoint_after,
        examined_files: scan.examined,
        deferred_files: scan.deferred,
        unsupported_files: scan.unsupported,
        committed: false,
        call_report: scan.call_report,
    });
    Ok(())
}
/// Execute exactly one bounded scan/batch, or recover the previous immutable
/// pending batch. The caller supplies every source/destination/private state path.
pub async fn run_once(options: &Options) -> Result<Receipt> {
    run_at(options, now()?).await
}
async fn run_at(options: &Options, time: i64) -> Result<Receipt> {
    run_with_transport(options, time, cfg!(test)).await
}
/// Synthetic loopback seam for composed Store/router tests; never a CLI option.
#[cfg(feature = "test-fixtures")]
#[doc(hidden)]
pub async fn run_loopback_fixture(options: &Options) -> Result<Receipt> {
    run_with_transport(options, now()?, true).await
}
async fn run_with_transport(
    options: &Options,
    time: i64,
    loopback_fixture: bool,
) -> Result<Receipt> {
    run_feed(options, time, loopback_fixture, None).await
}
pub(crate) async fn run_calls(
    options: &Options,
    key: Option<&std::path::Path>,
    audio: crate::audio::AudioClient,
    loopback: bool,
) -> Result<Receipt> {
    run_feed(options, now()?, loopback, Some((key, audio))).await
}
async fn run_feed(
    options: &Options,
    time: i64,
    loopback_fixture: bool,
    calls: Option<(Option<&std::path::Path>, crate::audio::AudioClient)>,
) -> Result<Receipt> {
    let producer = if calls.is_some() {
        "normalized-call-v1"
    } else {
        "local-codex-v1"
    };
    Uuid::parse_str(&options.connection_id).map_err(|_| "invalid_connection_id")?;
    let source = files::Root::open(&options.directory)?;
    let root_digest = hash(source.path().as_os_str().as_encoded_bytes());
    let state_path = transport::state_path(&options.state_file)?;
    let token_path = transport::token_path(&options.token_file)?;
    if state_path.starts_with(source.path())
        || token_path.starts_with(source.path())
        || state_path == token_path
    {
        return Err("token_and_state_must_be_separate_outside_source_directory");
    }
    if let Some((Some(key), _)) = &calls
        && (*key == token_path || *key == state_path || key.starts_with(source.path()))
    {
        return Err("transcription_key_must_be_separate_outside_source");
    }
    let _lock = transport::lock(&state_path)?;
    let http = transport::Http::new(&options.origin, &token_path, loopback_fixture)?;
    let status = http.status().await?;
    if status.connection_id != options.connection_id {
        return Err("destination_connection_mismatch");
    }
    let mut state = if let Some(bytes) = transport::read_private(&state_path, STATE_LIMIT)? {
        serde_json::from_slice::<State>(&bytes).map_err(|_| "invalid_saved_state_keep_file")?
    } else {
        if status.active_run.is_some() || status.local_sequence != 0 || status.checkpoint.is_some()
        {
            return Err("new_state_requires_unused_dedicated_connection");
        }
        State {
            version: 1,
            producer: producer.into(),
            transcriptions: BTreeMap::new(),
            origin: http.origin().into(),
            connection_id: status.connection_id.clone(),
            config_revision: status.config_revision,
            root_digest: root_digest.clone(),
            sequence: 0,
            checkpoint: None,
            scan_after: None,
            files: BTreeMap::new(),
            objects: BTreeMap::new(),
            pending: None,
        }
    };
    if state.producer != producer {
        return Err("saved_producer_changed_keep_state");
    }
    binding(&state, options, http.origin(), &status, &root_digest)?;
    transport::save(&state_path, &state)?;
    let resuming = state.pending.is_some();
    if state.pending.is_none() {
        position(&state, &status)?;
        if status.active_run.is_some() {
            return Err("unrelated_active_run_keep_state");
        }
        let scan = if let Some((key, audio)) = &calls {
            call_files::scan(&source, &mut state, time, &state_path, *key, audio).await?
        } else {
            source.scan(&state, time)?
        };
        if scan.entries.is_empty() && scan.files.is_empty() && scan.scan_after == state.scan_after {
            return Ok(Receipt {
                status: if calls.is_some() && scan.deferred > 0 {
                    "calls_deferred_no_new_data"
                } else {
                    "no_change"
                },
                request_id: None,
                accepted: 0,
                unchanged: 0,
                examined_files: scan.examined,
                deferred_files: scan.deferred,
                unsupported_files: scan.unsupported,
                producer_report: true,
                transcription: scan.call_report,
            });
        }
        stage(&mut state, scan, &status)?;
        transport::save(&state_path, &state)?;
    }
    if state
        .pending
        .as_ref()
        .ok_or("missing_pending")?
        .envelope
        .is_none()
    {
        start_recovery::ensure(
            &mut state,
            options,
            &http,
            &state_path,
            &root_digest,
            resuming,
        )
        .await?;
    }
    if state
        .pending
        .as_ref()
        .ok_or("missing_pending")?
        .envelope
        .is_none()
    {
        let body = envelope(&state, state.pending.as_ref().ok_or("missing_pending")?)?;
        if body.len() > 512 * 1024 {
            return Err("envelope_limit_keep_state");
        }
        state.pending.as_mut().ok_or("missing_pending")?.envelope = Some(body);
        transport::save(&state_path, &state)?;
    }
    let pending = state.pending.as_ref().ok_or("missing_pending")?;
    if Uuid::parse_str(&pending.request_id).is_err()
        || pending.entries.len() > MAX_ENTRIES
        || pending.envelope.as_deref() != Some(envelope(&state, pending)?.as_str())
    {
        return Err("saved_envelope_intent_mismatch_keep_state");
    }
    let mut receipt = http.receipt(&pending.request_id).await?;
    if receipt.is_none() {
        if pending.committed {
            return Err("committed_receipt_unavailable_keep_state");
        }
        let current = http.status().await?;
        binding(&state, options, http.origin(), &current, &root_digest)?;
        position(&state, &current)?;
        if !http.same_run(pending.run.as_ref().ok_or("missing_saved_run")?, &current) {
            return Err("saved_run_not_active_no_new_request_keep_state");
        }
        // Exactly one send. Any transport/server uncertainty keeps the saved UUID,
        // bytes, file/object cursor and prior committed position unchanged.
        let _outcome = http
            .batch(
                pending
                    .envelope
                    .as_deref()
                    .ok_or("missing_saved_envelope")?,
            )
            .await;
        receipt = http.receipt(&pending.request_id).await?;
    }
    let receipt = receipt.ok_or("batch_outcome_unknown_retry_same_command_and_state")?;
    check_receipt(
        &state,
        state.pending.as_ref().ok_or("missing_pending")?,
        &receipt,
    )?;
    state.pending.as_mut().ok_or("missing_pending")?.committed = true;
    transport::save(&state_path, &state)?;
    let pending = state.pending.as_ref().ok_or("missing_pending")?;
    let run = pending.run.as_ref().ok_or("missing_saved_run")?;
    let current = http.status().await?;
    binding(&state, options, http.origin(), &current, &root_digest)?;
    committed_position(&state, pending, &receipt, &current, &http)?;
    if current.active_run.is_some() {
        // A confirmed batch survives startup/lease fencing. Finish only while
        // the original run remains active; never rewrite an unknown old run.
        let finish = http.finish(run).await;
        let after = http.status().await?;
        binding(&state, options, http.origin(), &after, &root_digest)?;
        committed_position(&state, pending, &receipt, &after, &http)?;
        if after.active_run.is_some() {
            return Err(finish
                .err()
                .unwrap_or("committed_run_finish_unconfirmed_keep_state"));
        }
    }
    let pending = state.pending.take().ok_or("missing_pending")?;
    let result = Receipt {
        status: "batch_committed_partial_scan",
        request_id: Some(pending.request_id),
        accepted: receipt.accepted,
        unchanged: receipt.unchanged,
        examined_files: pending.examined_files,
        deferred_files: pending.deferred_files,
        unsupported_files: pending.unsupported_files,
        producer_report: true,
        transcription: pending.call_report,
    };
    state.files.extend(pending.files_after);
    state.objects.extend(pending.objects_after);
    state.scan_after = pending.scan_after;
    state.sequence = receipt.sequence;
    state.checkpoint = Some(pending.checkpoint_after);
    if state.files.len() > 1024 || state.objects.len() > MAX_OBJECTS {
        return Err("local_cursor_limit_keep_state");
    }
    transport::save(&state_path, &state)?;
    Ok(result)
}

fn trusted_ancestors(path: &std::path::Path) -> Result<()> {
    use std::{
        fs::OpenOptions,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        path::{Component, PathBuf},
    };
    let mut current = PathBuf::new();
    // SAFETY: geteuid has no arguments or memory contract.
    let owner = unsafe { libc::geteuid() };
    let mut sticky_parent = false;
    for part in path.components() {
        if matches!(part, Component::CurDir | Component::ParentDir) {
            return Err("untrusted_directory_ancestor");
        }
        current.push(part);
        let directory = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
            .open(&current)
            .map_err(|_| "untrusted_directory_ancestor")?;
        let info = directory
            .metadata()
            .map_err(|_| "untrusted_directory_ancestor")?;
        if !info.is_dir()
            || (info.uid() != 0 && info.uid() != owner)
            || (info.mode() & 0o022 != 0 && info.mode() & 0o1000 == 0)
            || (sticky_parent && info.uid() != owner)
        {
            return Err("untrusted_directory_ancestor");
        }
        sticky_parent = info.mode() & 0o022 != 0 && info.mode() & 0o1000 != 0;
    }
    if sticky_parent {
        return Err("sticky_directory_requires_owned_child");
    }
    Ok(())
}
