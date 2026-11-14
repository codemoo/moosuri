use super::*;
use chrono::DateTime;
use serde::de::IgnoredAny;
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::Read,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path},
};
use zeroize::Zeroizing;
const MAX_FILE: u64 = 32 * 1024 * 1024;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 20;
const SETTLE_SECONDS: i64 = 60;
const PARTIAL_IDLE_SECONDS: i64 = 3600;
const MAX_TEXT: usize = 2048;
pub(super) struct Snapshot {
    pub bytes: Zeroizing<Vec<u8>>,
    pub fingerprint: Fingerprint,
}
pub(super) struct Root {
    path: PathBuf,
    file: File,
}
pub(super) struct Scan {
    pub entries: Vec<SourceInput>,
    pub metadata: Vec<SourceObjectMetadata>,
    pub files: BTreeMap<String, Fingerprint>,
    pub objects: BTreeMap<String, String>,
    pub scan_after: Option<String>,
    pub examined: u32,
    pub deferred: u32,
    pub unsupported: u32,
    pub call_report: Option<crate::calls::Counts>,
}
#[derive(Deserialize)]
struct Line {
    timestamp: String,
    #[serde(flatten)]
    record: Record,
}
#[derive(Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
enum Record {
    SessionMeta(Meta),
    EventMsg(Event),
    Compacted(IgnoredAny),
    ResponseItem(IgnoredAny),
    DynamicToolsUpdated(IgnoredAny),
    InterAgentCommunication(IgnoredAny),
    InterAgentCommunicationMetadata(IgnoredAny),
    TurnContext(IgnoredAny),
    #[serde(rename = "token_usage_record")]
    TokenUsage(IgnoredAny),
    WorldState(IgnoredAny),
    SecurityRiskScore(IgnoredAny),
    RetainedContext(IgnoredAny),
    RealtimeItem(IgnoredAny),
    #[serde(other)]
    Ignored,
}
#[derive(Deserialize)]
struct Meta {
    id: String,
    session_id: Option<String>,
    source: String,
    thread_source: Option<String>,
    parent_thread_id: Option<IgnoredAny>,
    forked_from_id: Option<IgnoredAny>,
    agent_path: Option<IgnoredAny>,
    agent_role: Option<IgnoredAny>,
    agent_type: Option<IgnoredAny>,
    agent_nickname: Option<IgnoredAny>,
    history_base: Option<IgnoredAny>,
    subagent_history_start_ordinal: Option<IgnoredAny>,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    TaskStarted {
        turn_id: String,
        turn_attribution: Option<Attribution>,
        root_turn_id: Option<String>,
    },
    UserMessage {
        message: String,
    },
    TaskComplete {
        turn_id: String,
        error: Option<IgnoredAny>,
    },
    #[serde(other)]
    Ignored,
}
#[derive(Deserialize)]
struct Attribution {
    turn_id: String,
    turn_trigger: Option<String>,
    parent_turn_id: Option<IgnoredAny>,
    initiating_agent_path: Option<IgnoredAny>,
    root_turn_id: Option<String>,
}
struct Bucket {
    turn: String,
    requests: Vec<(i64, String)>,
    finished: bool,
    ended: bool,
    last: i64,
    omitted: bool,
}
struct Parsed {
    session: String,
    buckets: Vec<Bucket>,
    partial_line: bool,
    unsupported: bool,
}
fn stamp(value: &str) -> Result<i64> {
    let n = DateTime::parse_from_rfc3339(value)
        .map_err(|_| "unsupported_session_timestamp")?
        .timestamp();
    if !(0..=4_102_444_800).contains(&n) {
        return Err("unsupported_session_timestamp");
    }
    Ok(n)
}
fn valid_id(v: &str) -> bool {
    !v.is_empty() && v.len() <= 128 && !v.chars().any(char::is_control)
}
fn meta_allowed(meta: &Meta) -> bool {
    Uuid::parse_str(&meta.id).is_ok()
        && meta.session_id.as_ref().is_none_or(|id| id == &meta.id)
        && meta.source == "cli"
        && meta.thread_source.as_deref() == Some("user")
        && meta.parent_thread_id.is_none()
        && meta.forked_from_id.is_none()
        && meta.agent_path.is_none()
        && meta.agent_role.is_none()
        && meta.agent_type.is_none()
        && meta.agent_nickname.is_none()
        && meta.history_base.is_none()
        && meta.subagent_history_start_ordinal.is_none()
}
/// Evidence is a literal small prose request only. Never try to summarize a
/// credential/config/code/transcript-looking message into apparently safe prose.
fn prose(text: &str) -> Option<String> {
    let s = text.trim();
    if s.is_empty()
        || s.len() > MAX_TEXT
        || s.lines().count() > 20
        || s.chars().any(|c| c.is_control() && c != '\n')
    {
        return None;
    }
    let lower = s.to_lowercase();
    let markers = [
        "```",
        "~~~",
        "`",
        "{",
        "}",
        "<",
        ">",
        "=",
        ";",
        "|",
        "://",
        "/users/",
        "/home/",
        "/private/",
        "/tmp/",
        "~/",
        "\\",
        "authorization",
        "bearer ",
        "password",
        "passwd",
        "credential",
        "secret",
        "token",
        "api_key",
        "api-key",
        "api key",
        "private key",
        "access_key",
        "begin managed",
        "sk-",
        "ghp_",
        "github_pat_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "akia",
        "asia",
        "aiza",
        "xoxb-",
        "xoxp-",
        "eyj",
        "agents.md",
        "environment_context",
        "system-reminder",
        "user_instructions",
        "global codex",
        "another language model started",
        "message type:",
        "tool_call",
        "function_call",
        "event_msg",
        "response_item",
        "traceback",
        "stack trace",
        "assistant output",
        "assistant:",
        "developer instructions",
        "system prompt",
    ];
    if markers.iter().any(|marker| lower.contains(marker))
        || s.split_whitespace().any(|w| w.len() >= 24 && w.is_ascii())
        || s.lines()
            .any(|line| line.trim_start().starts_with(['[', ']', '$', '#']))
    {
        return None;
    }
    Some(s.to_owned())
}
fn parse(bytes: &[u8]) -> Result<Parsed> {
    let complete = bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map(|i| i + 1)
        .ok_or("session_header_incomplete")?;
    let mut lines = bytes[..complete]
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty());
    let first = lines.next().ok_or("session_header_incomplete")?;
    if first.len() > 2 * 1024 * 1024 {
        return Err("session_line_limit");
    }
    let first: Line = serde_json::from_slice(first).map_err(|_| "unsupported_codex_header")?;
    let Record::SessionMeta(meta) = first.record else {
        return Err("unsupported_codex_header");
    };
    if !meta_allowed(&meta) {
        return Err("unsupported_nonhuman_or_inherited_session");
    }
    let mut result = Parsed {
        session: meta.id,
        buckets: Vec::new(),
        partial_line: complete != bytes.len(),
        unsupported: false,
    };
    let mut active: Option<usize> = None;
    for (i, line) in lines.enumerate() {
        if i >= 20_000 || line.len() > 2 * 1024 * 1024 {
            return Err("session_record_limit");
        }
        let row: Line =
            serde_json::from_slice(line).map_err(|_| "malformed_complete_session_line")?;
        match row.record {
            Record::SessionMeta(_) => return Err("multiple_session_headers_unsupported"),
            Record::Compacted(_) => active = None,
            Record::EventMsg(Event::TaskStarted {
                turn_id,
                turn_attribution,
                root_turn_id,
            }) => {
                active = None;
                let Some(a) = turn_attribution else {
                    result.unsupported = true;
                    continue;
                };
                if !valid_id(&turn_id)
                    || a.turn_id != turn_id
                    || a.turn_trigger.as_deref() != Some("user")
                    || a.parent_turn_id.is_some()
                    || a.initiating_agent_path.is_some()
                    || a.root_turn_id.as_ref().is_some_and(|id| id != &turn_id)
                    || root_turn_id.as_ref().is_some_and(|id| id != &turn_id)
                {
                    result.unsupported = true;
                    continue;
                }
                if let Some(index) = result.buckets.iter().position(|b| b.turn == turn_id) {
                    active = Some(index);
                    result.buckets[index].ended = false;
                    result.buckets[index].finished = false;
                } else {
                    if result.buckets.len() >= 1024 {
                        return Err("session_bucket_limit");
                    }
                    result.buckets.push(Bucket {
                        turn: turn_id,
                        requests: vec![],
                        finished: false,
                        ended: false,
                        last: stamp(&row.timestamp)?,
                        omitted: false,
                    });
                    active = Some(result.buckets.len() - 1);
                }
            }
            Record::EventMsg(Event::UserMessage { message }) => {
                let Some(index) = active else {
                    result.unsupported = true;
                    continue;
                };
                let b = &mut result.buckets[index];
                let at = stamp(&row.timestamp)?;
                b.last = b.last.max(at);
                let Some(text) = prose(&message) else {
                    b.omitted = true;
                    continue;
                };
                if b.requests.last().is_some_and(|r| r == &(at, text.clone())) {
                    continue;
                }
                if b.requests.len() >= 4 {
                    b.omitted = true;
                    continue;
                }
                b.requests.push((at, text));
            }
            Record::EventMsg(Event::TaskComplete { turn_id, error }) => {
                if let Some(index) = active
                    && result.buckets[index].turn == turn_id
                {
                    let b = &mut result.buckets[index];
                    b.finished = error.is_none();
                    b.ended = true;
                    b.last = b.last.max(stamp(&row.timestamp)?);
                    active = None;
                }
            }
            _ => {}
        }
    }
    Ok(result)
}
pub(super) fn same_stat(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
}
pub(super) fn source_file(file: &File) -> Result<fs::Metadata> {
    let m = file.metadata().map_err(|_| "source_metadata_failed")?;
    // SAFETY: geteuid has no arguments or memory contract.
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o022 != 0
        || m.nlink() != 1
    {
        return Err("unsafe_source_file");
    }
    Ok(m)
}
impl Root {
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_absolute()
            || fs::canonicalize(path).map_err(|_| "source_directory_missing")? != path
        {
            return Err("explicit_canonical_source_directory_required");
        }
        super::trusted_ancestors(path)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_DIRECTORY)
            .open(path)
            .map_err(|_| "source_directory_open_failed")?;
        let m = file.metadata().map_err(|_| "source_metadata_failed")?;
        // SAFETY: geteuid has no arguments or memory contract.
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
            return Err("source_directory_must_be_owner_only");
        }
        Ok(Self {
            path: path.to_owned(),
            file,
        })
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub(super) fn open_relative(&self, path: &Path) -> Result<File> {
        let mut directory = self
            .file
            .try_clone()
            .map_err(|_| "source_directory_open_failed")?;
        let parts = path.components().collect::<Vec<_>>();
        for (i, part) in parts.iter().enumerate() {
            let Component::Normal(name) = part else {
                return Err("invalid_relative_source_path");
            };
            let name = CString::new(name.as_encoded_bytes())
                .map_err(|_| "invalid_relative_source_path")?;
            let flags = libc::O_RDONLY
                | libc::O_NOFOLLOW
                | libc::O_CLOEXEC
                | libc::O_NONBLOCK
                | if i + 1 < parts.len() {
                    libc::O_DIRECTORY
                } else {
                    0
                };
            // SAFETY: name is NUL-terminated and directory is a live owned fd;
            // O_NOFOLLOW applies at every component under the pinned root fd.
            let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
            if fd < 0 {
                return Err("source_open_failed");
            }
            // SAFETY: a successful openat returned one new owned descriptor.
            directory = unsafe { File::from_raw_fd(fd) };
        }
        Ok(directory)
    }
    pub(super) fn discover(&self, extension: &str) -> Result<BTreeMap<String, PathBuf>> {
        let mut todo = vec![(self.path.clone(), 0usize)];
        let mut found = BTreeMap::new();
        let mut examined = 0usize;
        while let Some((directory, depth)) = todo.pop() {
            if depth > 8 {
                return Err("source_directory_depth_limit");
            }
            let m =
                fs::symlink_metadata(&directory).map_err(|_| "source_discovery_changed_retry")?;
            if !m.is_dir() || m.file_type().is_symlink() || m.mode() & 0o022 != 0 {
                return Err("unsafe_source_subdirectory");
            }
            for item in fs::read_dir(&directory).map_err(|_| "source_discovery_changed_retry")? {
                examined += 1;
                if examined > 4096 {
                    return Err("source_directory_entry_limit_choose_smaller_scope");
                }
                let item = item.map_err(|_| "source_discovery_changed_retry")?;
                let kind = item
                    .file_type()
                    .map_err(|_| "source_discovery_changed_retry")?;
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    todo.push((item.path(), depth + 1));
                    continue;
                }
                if !kind.is_file() || item.path().extension().is_none_or(|v| v != extension) {
                    continue;
                }
                let relative = item
                    .path()
                    .strip_prefix(&self.path)
                    .map_err(|_| "source_path_outside_scope")?
                    .to_owned();
                found.insert(hash(relative.as_os_str().as_encoded_bytes()), relative);
                if found.len() > 1024 {
                    return Err("source_file_count_limit_choose_smaller_scope");
                }
            }
        }
        Ok(found)
    }
    pub(super) fn identity(&self) -> Result<(u64, u64)> {
        let pinned = self
            .file
            .metadata()
            .map_err(|_| "source_root_changed_retry")?;
        let named = fs::symlink_metadata(&self.path).map_err(|_| "source_root_changed_retry")?;
        if pinned.dev() != named.dev()
            || pinned.ino() != named.ino()
            || named.file_type().is_symlink()
        {
            return Err("source_root_changed_retry");
        }
        Ok((pinned.dev(), pinned.ino()))
    }
    pub(super) fn matches(&self, path: &Path, expected: &Fingerprint) -> bool {
        self.open_relative(path)
            .and_then(|f| source_file(&f))
            .is_ok_and(|m| {
                m.dev() == expected.device
                    && m.ino() == expected.inode
                    && m.len() == expected.bytes
                    && m.mtime() == expected.modified_sec
                    && m.mtime_nsec() == expected.modified_nsec
            })
    }
    pub(super) fn snapshot(
        &self,
        path: &Path,
        time: i64,
        max_file: u64,
        remaining: u64,
    ) -> Result<Snapshot> {
        let file = self.open_relative(path)?;
        let before = source_file(&file)?;
        if before.len() > max_file
            || before.len() > remaining
            || time.saturating_sub(before.mtime()) < SETTLE_SECONDS
        {
            return Err("source_not_settled_or_bounded");
        }
        let mut bytes = Zeroizing::new(Vec::new());
        (&file)
            .take(max_file + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "source_read_changed_retry")?;
        if bytes.len() as u64 != before.len()
            || !same_stat(&before, &source_file(&file)?)
            || !same_stat(&before, &source_file(&self.open_relative(path)?)?)
        {
            return Err("source_read_changed_retry");
        }
        Ok(Snapshot {
            fingerprint: Fingerprint {
                device: before.dev(),
                inode: before.ino(),
                bytes: before.len(),
                modified_sec: before.mtime(),
                modified_nsec: before.mtime_nsec(),
                sha256: hash(&bytes),
            },
            bytes,
        })
    }
    pub fn scan(&self, state: &State, time: i64) -> Result<Scan> {
        let pinned = self
            .file
            .metadata()
            .map_err(|_| "source_root_changed_retry")?;
        let named = fs::symlink_metadata(&self.path).map_err(|_| "source_root_changed_retry")?;
        if pinned.dev() != named.dev()
            || pinned.ino() != named.ino()
            || named.file_type().is_symlink()
        {
            return Err("source_root_changed_retry");
        }
        let paths = self.discover("jsonl")?;
        let mut ordered = paths.into_iter().collect::<Vec<_>>();
        if let Some(after) = &state.scan_after {
            let offset = ordered.partition_point(|(id, _)| id <= after);
            ordered.rotate_left(offset);
        }
        let mut out = Scan {
            entries: vec![],
            metadata: vec![],
            files: BTreeMap::new(),
            objects: BTreeMap::new(),
            scan_after: state.scan_after.clone(),
            examined: 0,
            deferred: 0,
            unsupported: 0,
            call_report: None,
        };
        let mut total = 0u64;
        for (key, path) in ordered.into_iter().take(MAX_FILES) {
            out.examined += 1;
            out.scan_after = Some(key.clone());
            let snapshot =
                match self.snapshot(&path, time, MAX_FILE, MAX_TOTAL.saturating_sub(total)) {
                    Ok(value) => value,
                    Err(_) => {
                        out.deferred += 1;
                        continue;
                    }
                };
            total += snapshot.bytes.len() as u64;
            let Snapshot { bytes, fingerprint } = snapshot;
            if state.files.get(&key).is_some_and(|old| {
                old.device == fingerprint.device
                    && old.inode == fingerprint.inode
                    && old.bytes == fingerprint.bytes
                    && old.modified_sec == fingerprint.modified_sec
                    && old.modified_nsec == fingerprint.modified_nsec
                    && old.sha256 == fingerprint.sha256
            }) {
                continue;
            }
            let parsed = match parse(&bytes) {
                Ok(p) => p,
                Err(_) => {
                    out.unsupported += 1;
                    continue;
                }
            };
            let mut deferred = parsed.partial_line || parsed.unsupported;
            if parsed.unsupported {
                out.unsupported += 1;
            }
            for b in parsed.buckets {
                if b.requests.is_empty() {
                    continue;
                }
                if !b.ended && time.saturating_sub(b.last) < PARTIAL_IDLE_SECONDS {
                    deferred = true;
                    continue;
                }
                let id = format!(
                    "local-codex:{}",
                    hash(format!("{}:{}", parsed.session, b.turn))
                );
                let status = if b.finished {
                    "finished_marker"
                } else {
                    "partial"
                };
                let mut text = format!(
                    "Local Codex user-request evidence\nTurn state: {status} (not proof of work success)\nEvidence coverage: {}\n",
                    if b.omitted {
                        "some requests omitted"
                    } else {
                        "selected plain user requests"
                    }
                );
                for (at, request) in b.requests {
                    text.push_str(&format!("\nUser request at UTC:{at}\n{request}\n"));
                }
                let version = hash(text.as_bytes());
                if state.objects.get(&id) == Some(&version)
                    || out.objects.get(&id) == Some(&version)
                {
                    continue;
                }
                if out.entries.len() >= MAX_ENTRIES {
                    deferred = true;
                    continue;
                }
                out.objects.insert(id.clone(), version.clone());
                out.metadata.push(SourceObjectMetadata {
                    external_id: id.clone(),
                    thread_id: Some(format!(
                        "local-codex-thread:{}",
                        hash(parsed.session.as_bytes())
                    )),
                    parent_id: None,
                });
                out.entries.push(SourceInput {
                    external_id: id,
                    version,
                    expected_revision: None,
                    title: "Codex 사용자 요청 기록".into(),
                    text,
                    speaker: Some("user request (local Codex)".into()),
                    observed_at: b.last,
                    valid_from: None,
                    valid_until: None,
                });
            }
            if deferred {
                out.deferred += 1;
            } else {
                out.files.insert(key, fingerprint);
            }
            if out.entries.len() >= MAX_ENTRIES {
                break;
            }
        }
        let named = fs::symlink_metadata(&self.path).map_err(|_| "source_root_changed_retry")?;
        if pinned.dev() != named.dev()
            || pinned.ino() != named.ino()
            || named.file_type().is_symlink()
        {
            return Err("source_root_changed_retry");
        }
        Ok(out)
    }
}
