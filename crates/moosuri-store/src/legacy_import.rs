//! Explicit normalized private export import, never a legacy DB/runtime adapter.
//! All input is private/untrusted; diagnostics contain fixed codes, never payloads.
use crate::{Store, StoreError, backup, cutover};
use fs2::FileExt;
use moosuri_core::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
    time::Instant,
};
use uuid::Uuid;
use zeroize::Zeroizing;
type Result<T> = std::result::Result<T, Error>;
const LIMIT: u64 = 16 * 1024 * 1024;
const EVENTS: usize = 512;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("import_invalid_input")]
    Invalid,
    #[error("import_bundle_invalid")]
    Bundle,
    #[error("import_output_exists")]
    Exists,
    #[error("import_unavailable")]
    Unavailable,
    #[error("import_integrity_failed")]
    Integrity,
}
impl From<StoreError> for Error {
    fn from(_: StoreError) -> Self {
        Self::Integrity
    }
}
impl From<backup::Error> for Error {
    fn from(_: backup::Error) -> Self {
        Self::Invalid
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: u32,
    purpose: String,
    bundle_id: String,
    source_manifest: Manifest,
    events: Vec<Value>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    deployment_id: String,
    source_version: String,
    source_schema: String,
    exported_at: i64,
    coverage: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    sequence: u32,
    owner: String,
    id: String,
    revision: i64,
    operation: Operation,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    SourceUpsert {
        scope: String,
        external_id: String,
        version: String,
        title: String,
        text: String,
        speaker: Option<String>,
        observed_at: i64,
        valid_from: Option<i64>,
        valid_until: Option<i64>,
    },
    SourceDelete,
    MemoryUpsert {
        subject: String,
        text: String,
        speaker: Option<String>,
        valid_from: Option<i64>,
        valid_until: Option<i64>,
        evidence: Vec<Evidence>,
    },
    MemoryForget,
    DocumentUpsert {
        title: String,
        body: String,
        references: Vec<Reference>,
    },
    DocumentDelete,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    source: String,
    revision: i64,
    start_byte: u32,
    end_byte: u32,
    quote: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    kind: Kind,
    id: String,
    revision: i64,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Source,
    Memory,
    Document,
}
impl Operation {
    fn kind(&self) -> Kind {
        match self {
            Self::SourceUpsert { .. } | Self::SourceDelete => Kind::Source,
            Self::MemoryUpsert { .. } | Self::MemoryForget => Kind::Memory,
            _ => Kind::Document,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
struct Mapping {
    kind: Kind,
    legacy_id: String,
    target_id: String,
    revision: i64,
    scope: Option<String>,
    external_id: Option<String>,
    state: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Outcome {
    index: usize,
    sequence: Option<u32>,
    row_sha256: String,
    kind: String,
    state: String,
    reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Report {
    pub format: u32,
    pub purpose: String,
    pub bundle_id: String,
    pub bundle_sha256: String,
    source_manifest: Manifest,
    pub selected_owner: String,
    pub account_id: String,
    pub tenant_id: String,
    pub total_events: usize,
    pub imported_events: usize,
    pub quarantined_events: usize,
    pub authority_state: String,
    pub report_artifacts: Vec<String>,
    mappings: Vec<Mapping>,
    outcomes: Vec<Outcome>,
}
pub struct Enrollment {
    pub name: String,
    pub email: String,
    pub password: Zeroizing<String>,
}
fn hash(bytes: &[u8]) -> String {
    backup::hex(&Sha256::digest(bytes))
}
fn valid_label(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
fn json(v: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(v).map_err(|_| Error::Integrity)
}
fn write(path: &Path, v: &impl Serialize) -> Result<()> {
    let b = json(v)?;
    let mut f = backup::files::create_file(path)?;
    f.write_all(&b).map_err(|_| Error::Unavailable)?;
    f.sync_all().map_err(|_| Error::Unavailable)
}
fn output(path: &Path) -> Result<PathBuf> {
    backup::files::private_output_path(path).map_err(|e| {
        if e == backup::Error::OutputExists {
            Error::Exists
        } else {
            Error::Invalid
        }
    })
}
struct Stage {
    path: PathBuf,
    identity: (u64, u64),
    published: bool,
}
impl Stage {
    fn new(parent: &Path) -> Result<Self> {
        let path = parent.join(format!(".moosuri-restore-stage-import-{}", Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| Error::Unavailable)?;
        let m = fs::symlink_metadata(&path).map_err(|_| Error::Unavailable)?;
        Ok(Self {
            path,
            identity: (m.dev(), m.ino()),
            published: false,
        })
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if !self.published
            && fs::symlink_metadata(&self.path)
                .is_ok_and(|m| m.is_dir() && (m.dev(), m.ino()) == self.identity)
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
fn open_private_stage(path: &Path) -> Result<Store> {
    // Normal admission deliberately rejects this prefix. Only this fresh private
    // importer may construct the existing admission token; crashes leave no
    // partially imported stage that ordinary serve/recovery can open.
    if fs::symlink_metadata(path).is_err() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(path)
            .map_err(|_| Error::Unavailable)?;
    }
    backup::files::directory(path)?;
    if fs::read_dir(path)
        .map_err(|_| Error::Unavailable)?
        .next()
        .is_some()
    {
        return Err(Error::Exists);
    }
    let lock = backup::files::create_file(&path.join("writer.lock"))?;
    lock.try_lock_exclusive().map_err(|_| Error::Unavailable)?;
    Store::open_admitted(
        path,
        WebToolProfile::default(),
        Some(cutover::RuntimeAdmission {
            root: path.to_owned(),
            lock,
        }),
    )
    .map_err(Error::from)
}
fn raw_kind(v: &Value) -> String {
    match v.pointer("/operation/kind").and_then(Value::as_str) {
        Some("source_upsert" | "source_delete") => "source",
        Some("memory_upsert" | "memory_forget") => "memory",
        Some("document_upsert" | "document_delete") => "document",
        _ => "unsupported",
    }
    .into()
}
fn key(kind: Kind, id: &str) -> (Kind, String) {
    (kind, id.to_owned())
}
fn target<'a>(
    map: &'a BTreeMap<(Kind, String), Mapping>,
    kind: Kind,
    id: &str,
    revision: i64,
) -> std::result::Result<&'a Mapping, StoreError> {
    let m = map.get(&key(kind, id)).ok_or(StoreError::InvalidInput)?;
    if m.state != "active" || m.revision != revision {
        return Err(StoreError::InvalidInput);
    }
    Ok(m)
}

async fn retire(
    store: &Store,
    user: &crate::UserContext,
    m: &mut Mapping,
) -> std::result::Result<(), StoreError> {
    if m.target_id.is_empty() || m.state != "active" {
        m.state = "quarantined".into();
        return Ok(());
    }
    match m.kind {
        Kind::Source => {
            store
                .delete_source(user, m.target_id.clone(), m.revision)
                .await?
        }
        Kind::Memory => {
            store
                .forget_memory(user, m.target_id.clone(), m.revision)
                .await?
        }
        Kind::Document => {
            store
                .delete_artifact(
                    user,
                    m.target_id.clone(),
                    ArtifactDelete {
                        request_id: Uuid::new_v4().to_string(),
                        expected_revision: m.revision,
                    },
                )
                .await?;
        }
    }
    m.revision += 1;
    m.state = "quarantined".into();
    Ok(())
}
async fn apply(
    store: &Store,
    user: &crate::UserContext,
    event: Event,
    map: &mut BTreeMap<(Kind, String), Mapping>,
    collections: &mut BTreeMap<String, (String, i64)>,
) -> std::result::Result<(), StoreError> {
    let kind = event.operation.kind();
    let k = key(kind, &event.id);
    let old = map.get(&k).cloned();
    if !valid_label(&event.id, 128)
        || event.revision <= 0
        || event.revision > 129
        || old.as_ref().is_some_and(|m| m.state != "active")
        || event.revision != old.as_ref().map(|m| m.revision + 1).unwrap_or(1)
    {
        return Err(StoreError::InvalidInput);
    }
    let request = Uuid::new_v4().to_string();
    let mut m = old.clone().unwrap_or(Mapping {
        kind,
        legacy_id: event.id,
        target_id: String::new(),
        revision: 0,
        scope: None,
        external_id: None,
        state: "active".into(),
    });
    match event.operation {
        Operation::SourceUpsert {
            scope,
            external_id,
            version,
            title,
            text,
            speaker,
            observed_at,
            valid_from,
            valid_until,
        } => {
            if !valid_label(&scope, 128)
                || m.scope.as_ref().is_some_and(|s| s != &scope)
                || m.external_id.as_ref().is_some_and(|s| s != &external_id)
            {
                return Err(StoreError::InvalidInput);
            }
            // Legacy source identity is unique within its scope. Distinct IDs
            // cannot silently alias one existing modern source through external_id.
            if map.values().any(|other| {
                other.kind == Kind::Source
                    && other.legacy_id != m.legacy_id
                    && other.scope.as_deref() == Some(&scope)
                    && other.external_id.as_deref() == Some(&external_id)
            }) {
                return Err(StoreError::InvalidInput);
            }
            let (collection, cursor) = if let Some(c) = collections.get(&scope) {
                c.clone()
            } else {
                let c = store
                    .create_collection(
                        user,
                        "Imported offline source evidence".into(),
                        format!("legacy:{}", hash(scope.as_bytes())),
                    )
                    .await?;
                collections.insert(scope.clone(), (c.id.clone(), 0));
                (c.id, 0)
            };
            let r = store
                .ingest_text(
                    user,
                    collection.clone(),
                    IngestBatch {
                        request_id: request,
                        expected_cursor: cursor,
                        next_cursor: cursor + 1,
                        complete_snapshot: false,
                        entries: vec![SourceInput {
                            external_id: external_id.clone(),
                            version,
                            expected_revision: old.as_ref().map(|m| m.revision),
                            title,
                            text,
                            speaker,
                            observed_at,
                            valid_from,
                            valid_until,
                        }],
                    },
                )
                .await?;
            collections.insert(scope.clone(), (collection, cursor + 1));
            m.target_id = r.source_ids[0].clone();
            if store
                .source(user, m.target_id.clone(), None)
                .await?
                .source
                .revision
                != event.revision
            {
                return Err(StoreError::InvalidInput);
            }
            m.scope = Some(scope);
            m.external_id = Some(external_id);
        }
        Operation::MemoryUpsert {
            subject,
            text,
            speaker,
            valid_from,
            valid_until,
            evidence,
        } => {
            let refs = evidence
                .into_iter()
                .map(|e| {
                    let t = target(map, Kind::Source, &e.source, e.revision)?;
                    Ok(EvidenceRef {
                        source_id: t.target_id.clone(),
                        source_revision: e.revision,
                        start_byte: e.start_byte,
                        end_byte: e.end_byte,
                        quote: e.quote,
                    })
                })
                .collect::<std::result::Result<Vec<_>, StoreError>>()?;
            let draft = MemoryDraft {
                subject,
                text,
                speaker,
                valid_from,
                valid_until,
                evidence: refs,
            };
            let r = if let Some(old) = &old {
                store
                    .correct_memory(user, old.target_id.clone(), old.revision, request, draft)
                    .await?
            } else {
                store.create_memory(user, request, draft).await?
            };
            m.target_id = r.id;
        }
        Operation::DocumentUpsert {
            title,
            body,
            references,
        } => {
            let refs = references
                .into_iter()
                .map(|r| {
                    let m = target(map, r.kind, &r.id, r.revision)?;
                    Ok(ArtifactDependency {
                        kind: match r.kind {
                            Kind::Source => ArtifactDependencyKind::Source,
                            Kind::Memory => ArtifactDependencyKind::Memory,
                            Kind::Document => ArtifactDependencyKind::Artifact,
                        },
                        id: m.target_id.clone(),
                        revision: r.revision,
                    })
                })
                .collect::<std::result::Result<Vec<_>, StoreError>>()?;
            let r = store
                .write_artifact(
                    user,
                    ArtifactWrite {
                        request_id: request,
                        id: old.as_ref().map(|m| m.target_id.clone()),
                        expected_revision: old.as_ref().map(|m| m.revision),
                        draft: ArtifactDraft {
                            title,
                            body,
                            payload: ArtifactPayload::Document,
                            references: refs,
                        },
                    },
                )
                .await?;
            m.target_id = r.id;
        }
        Operation::SourceDelete | Operation::MemoryForget | Operation::DocumentDelete => {
            // A first explicit revision1 tombstone may have no retained body.
            // Keep its barrier in mappings/reports without inventing source bytes.
            if old.is_some() {
                retire(store, user, &mut m).await?;
            }
            m.state = match kind {
                Kind::Source => "deleted",
                Kind::Memory => "forgotten",
                Kind::Document => "deleted",
            }
            .into();
        }
    }
    m.revision = event.revision;
    map.insert(k, m);
    Ok(())
}
fn known_key(v: &Value) -> Option<(Kind, String)> {
    let id = v.get("id")?.as_str()?;
    if !valid_label(id, 128) {
        return None;
    }
    let kind = match raw_kind(v).as_str() {
        "source" => Kind::Source,
        "memory" => Kind::Memory,
        "document" => Kind::Document,
        _ => return None,
    };
    Some(key(kind, id))
}
async fn reconcile(
    store: &Store,
    user: &crate::UserContext,
    map: BTreeMap<(Kind, String), Mapping>,
) -> Result<Vec<Mapping>> {
    let tenant = user.tenant.0.clone();
    Ok(store.call(move|db|{let mut result=vec![];for mut m in map.into_values(){if !m.target_id.is_empty(){let sql=match m.kind{Kind::Source=>"SELECT revision,CASE deleted WHEN 1 THEN 'deleted' ELSE 'active' END FROM knowledge_sources WHERE tenant_id=?1 AND id=?2",Kind::Memory=>"SELECT revision,status FROM knowledge_memories WHERE tenant_id=?1 AND id=?2",Kind::Document=>"SELECT revision,status FROM personal_artifacts WHERE tenant_id=?1 AND id=?2"};let (revision,state):(i64,String)=db.conn.query_row(sql,rusqlite::params![tenant,m.target_id],|r|Ok((r.get(0)?,r.get(1)?)))?;m.revision=revision;if m.state!="quarantined"{m.state=state;}}result.push(m);}Ok(result)}).await?)
}
async fn reports_in_sqlite(
    store: &Store,
    user: &crate::UserContext,
    report: &mut Report,
) -> Result<()> {
    // Ordinary private report artifacts intentionally remain editable owner data,
    // not an invented immutable provider receipt or new schema/audit authority.
    // Bounded chunks keep every mapping/outcome inside normal encrypted DB backups.
    let overview = serde_json::json!({"purpose":"imported-reconciliation-evidence-v1","bundle_id":report.bundle_id,"bundle_sha256":report.bundle_sha256,"source_manifest":report.source_manifest,"selected_owner":report.selected_owner,"account_id":report.account_id,"tenant_id":report.tenant_id,"total_events":report.total_events,"imported_events":report.imported_events,"quarantined_events":report.quarantined_events,"authority_state":report.authority_state,"limits":"ordinary editable imported report; not external receipt/latestness proof; retain original bundle and sibling reports"});
    let mut pages = vec![overview];
    for chunk in report.mappings.chunks(16) {
        pages.push(serde_json::json!({"purpose":"imported-identity-mapping-v1","bundle_id":report.bundle_id,"mappings":chunk}));
    }
    for chunk in report.outcomes.chunks(16) {
        pages.push(serde_json::json!({"purpose":"imported-event-reconciliation-v1","bundle_id":report.bundle_id,"outcomes":chunk}));
    }
    for (i, page) in pages.into_iter().enumerate() {
        let body = String::from_utf8(json(&page)?).map_err(|_| Error::Integrity)?;
        let r = store
            .write_artifact(
                user,
                ArtifactWrite {
                    request_id: Uuid::new_v4().to_string(),
                    id: None,
                    expected_revision: None,
                    draft: ArtifactDraft {
                        title: format!(
                            "Imported reconciliation report {} part {}",
                            report.bundle_id,
                            i + 1
                        ),
                        body,
                        payload: ArtifactPayload::Document,
                        references: vec![],
                    },
                },
            )
            .await?;
        report.report_artifacts.push(r.id);
    }
    Ok(())
}
async fn finalize(
    store: &Store,
    user: &crate::UserContext,
    collections: BTreeMap<String, (String, i64)>,
) -> Result<()> {
    for (_, (id, cursor)) in collections {
        store
            .disconnect_collection(user, id, cursor + 1, DisconnectMode::Keep)
            .await?;
    }
    let account = user.account.id.clone();
    store
        .call(move |db| {
            // New root imports no old authority whatsoever. Revoke the temporary
            // internal import login and assert no execution/credential table gained rows.
            for table in [
                "providers",
                "chat_runs",
                "missions",
                "reminder_schedules",
                "mail_connections",
                "calendar_connections",
                "source_connections",
                "chatgpt_bindings",
                "push_subscriptions",
            ] {
                if db
                    .conn
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                        r.get::<_, i64>(0)
                    })?
                    != 0
                {
                    return Err(StoreError::Corrupt);
                }
            }
            db.conn
                .execute("DELETE FROM sessions WHERE account_id=?1", [account])?;
            if db
                .conn
                .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))?
                != "ok"
                || db.conn.prepare("PRAGMA foreign_key_check")?.exists([])?
            {
                return Err(StoreError::Corrupt);
            }
            Ok(())
        })
        .await?;
    Ok(())
}
/// Import an explicitly selected normalized bundle into a new data root. Reports
/// publish first in a separate new directory; data publication is last. Orphan
/// reports never imply completion. Inputs and any existing destinations stay intact.
pub async fn import_bundle(
    input: &Path,
    owner: &str,
    enrollment: Enrollment,
    dest: &Path,
    report_dir: &Path,
) -> Result<Report> {
    if !valid_label(owner, 128) {
        return Err(Error::Invalid);
    }
    let dest = output(dest)?;
    // A completed import must be admissible through the ordinary runtime path.
    // Reserved restore-stage names (including ancestors) remain blocked.
    cutover::restore::reject_stage(&dest).map_err(|_| Error::Invalid)?;
    let report_dir = output(report_dir)?;
    if dest.starts_with(&report_dir) || report_dir.starts_with(&dest) {
        return Err(Error::Invalid);
    }
    let bytes = Zeroizing::new({
        let mut b = vec![];
        backup::files::read_file(input, LIMIT)?
            .take(LIMIT + 1)
            .read_to_end(&mut b)
            .map_err(|_| Error::Unavailable)?;
        if b.len() as u64 > LIMIT {
            return Err(Error::Bundle);
        }
        b
    });
    let bundle: Bundle = serde_json::from_slice(&bytes).map_err(|_| Error::Bundle)?;
    if bundle.format != 1
        || bundle.purpose != "moosuri-private-transfer-v1"
        || Uuid::parse_str(&bundle.bundle_id).is_err()
        || bundle.events.len() > EVENTS
        || !valid_label(&bundle.source_manifest.deployment_id, 128)
        || !valid_label(&bundle.source_manifest.source_version, 128)
        || !valid_label(&bundle.source_manifest.source_schema, 128)
        || !valid_label(&bundle.source_manifest.coverage, 1024)
    {
        return Err(Error::Bundle);
    }
    let mut stage = Stage::new(dest.parent().ok_or(Error::Invalid)?)?;
    let staged_data = stage.path.clone();
    let store = open_private_stage(&staged_data)?;
    store
        .bootstrap(
            enrollment.name,
            enrollment.email.clone(),
            enrollment.password.to_string(),
            false,
        )
        .await?;
    let login = store
        .login(enrollment.email, enrollment.password.to_string())
        .await?;
    let user = login.user;
    let mut report = Report {
        format: 1,
        purpose: "moosuri-private-import-report-v1".into(),
        bundle_id: bundle.bundle_id,
        bundle_sha256: hash(&bytes),
        source_manifest: bundle.source_manifest,
        selected_owner: owner.into(),
        account_id: user.account.id.clone(),
        tenant_id: user.tenant.0.clone(),
        total_events: bundle.events.len(),
        imported_events: 0,
        quarantined_events: 0,
        authority_state: "new_account_only_no_legacy_authority".into(),
        report_artifacts: vec![],
        mappings: vec![],
        outcomes: vec![],
    };
    let mut mappings = BTreeMap::new();
    let mut collections = BTreeMap::new();
    let mut blocked = BTreeSet::new();
    let start = Instant::now();
    for (index, row) in bundle.events.into_iter().enumerate() {
        if start.elapsed().as_secs() > 120 {
            return Err(Error::Unavailable);
        }
        let selected = row.get("owner").and_then(Value::as_str) == Some(owner);
        let key = if selected { known_key(&row) } else { None };
        let seq = row
            .get("sequence")
            .and_then(Value::as_u64)
            .and_then(|s| u32::try_from(s).ok());
        let parsed = serde_json::from_value::<Event>(row.clone());
        let reason = if !selected {
            Some("owner_not_selected")
        } else if seq != Some(index as u32 + 1) {
            Some("sequence_invalid")
        } else if key.as_ref().is_some_and(|k| blocked.contains(k)) {
            Some("object_already_quarantined")
        } else if let Ok(event) = parsed {
            if event.owner != owner || event.sequence != index as u32 + 1 {
                Some("owner_or_sequence_invalid")
            } else {
                match apply(&store, &user, event, &mut mappings, &mut collections).await {
                    Ok(()) => None,
                    Err(StoreError::InvalidInput | StoreError::Conflict | StoreError::NotFound) => {
                        Some("invalid_revision_reference_or_content")
                    }
                    Err(_) => return Err(Error::Integrity),
                }
            }
        } else {
            Some("unsupported_or_invalid_event")
        };
        if let Some(reason) = reason {
            if let Some(key) = key {
                blocked.insert(key.clone());
                if let Some(m) = mappings.get_mut(&key) {
                    retire(&store, &user, m).await?;
                }
            }
            report.quarantined_events += 1;
            report.outcomes.push(Outcome {
                index,
                sequence: seq,
                row_sha256: hash(&json(&row)?),
                kind: raw_kind(&row),
                state: "quarantined".into(),
                reason: reason.into(),
            });
        } else {
            report.imported_events += 1;
            report.outcomes.push(Outcome {
                index,
                sequence: seq,
                row_sha256: hash(&json(&row)?),
                kind: raw_kind(&row),
                state: "applied".into(),
                reason: "mapped_by_explicit_ordered_contract".into(),
            });
        }
    }
    report.mappings = reconcile(&store, &user, mappings).await?;
    reports_in_sqlite(&store, &user, &mut report).await?;
    finalize(&store, &user, collections).await?;
    drop(store);
    // Close SQLite before syncing/publishing. Its WAL is never blindly discarded.
    for e in fs::read_dir(&staged_data).map_err(|_| Error::Unavailable)? {
        let e = e.map_err(|_| Error::Unavailable)?;
        backup::files::read_file(&e.path(), backup::DB_LIMIT)?
            .sync_all()
            .map_err(|_| Error::Unavailable)?;
    }
    backup::files::sync_dir(&staged_data)?;
    let mut reports = Stage::new(report_dir.parent().ok_or(Error::Invalid)?)?;
    write(&reports.path.join("report.json"), &report)?;
    write(
        &reports.path.join("quarantine.json"),
        &serde_json::json!({"format":1,"bundle_sha256":report.bundle_sha256,"payload_policy":"rejected raw payload remains only in unchanged selected bundle; retain it privately","events":report.outcomes.iter().filter(|o|o.state=="quarantined").collect::<Vec<_>>()}),
    )?;
    backup::files::sync_dir(&reports.path)?;
    cutover::publication::no_replace(&reports.path, &report_dir)?;
    reports.published = true;
    backup::files::sync_dir(report_dir.parent().ok_or(Error::Invalid)?)?;
    cutover::publication::no_replace(&staged_data, &dest)?;
    stage.published = true;
    backup::files::sync_dir(dest.parent().ok_or(Error::Invalid)?)?;
    Ok(report)
}
#[cfg(test)]
mod tests;
