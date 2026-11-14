//! Source-grounded schema23 relation projection adapter. No SQL/backup/key access.
use crate::{
    backup,
    legacy_import::{self, Enrollment, Report},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::Path,
};
use uuid::Uuid;
use zeroize::Zeroizing;
const LIMIT: u64 = 16 * 1024 * 1024;
const ROW_LIMIT: usize = 4096;
const CONVERTED_LIMIT: usize = 128;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("legacy_relation_input_invalid")]
    Invalid,
    #[error("legacy_relation_schema_mismatch")]
    Schema,
    #[error("legacy_relation_limit_exceeded")]
    Limit,
    #[error("legacy_relation_output_unavailable")]
    Output,
    #[error("legacy_relation_import_failed")]
    Import,
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    #[serde(rename = "type")]
    kind: String,
    format: u32,
    purpose: String,
    source_schema: u32,
    owner: String,
    deployment_id: String,
    source_version: String,
    snapshot: String,
    exported_at: String,
    inventory: Vec<Identity>,
}
#[derive(Deserialize, Serialize, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct Identity {
    schema: String,
    name: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Relation {
    #[serde(rename = "type")]
    kind: String,
    schema: String,
    name: String,
    count: usize,
    rows: Vec<Value>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Complete {
    #[serde(rename = "type")]
    kind: String,
    snapshot: String,
    relations: usize,
}
#[derive(Deserialize)]
struct Checksum {
    name: String,
    sha256: String,
}
#[derive(Clone, Serialize)]
struct RowResult {
    relation: String,
    index: usize,
    row_sha256: String,
    decision: String,
    reason: String,
    target_legacy_id: Option<String>,
    legacy_revision: Option<i64>,
    new_revision: Option<i64>,
    metadata: Value,
}
#[derive(Serialize)]
pub struct AdapterReport {
    pub purpose: String,
    pub snapshot_sha256: String,
    pub source_schema: u32,
    pub total_rows: usize,
    pub converted_objects: usize,
    pub quarantined_rows: usize,
    pub supporting_rows: usize,
    pub source_admission: String,
    pub snapshot_current_projection_only: bool,
    relations: BTreeMap<String, usize>,
    rows: Vec<RowResult>,
}
pub struct Imported {
    pub report: Report,
    pub adapter: AdapterReport,
}
struct Snapshot {
    header: Header,
    tables: BTreeMap<String, Vec<Value>>,
    outcomes: Vec<RowResult>,
    sha: String,
}
fn sha(b: &[u8]) -> String {
    backup::hex(&Sha256::digest(b))
}
fn bytes(v: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(v).map_err(|_| Error::Invalid)
}
fn text<'a>(v: &'a Value, k: &str) -> Result<&'a str> {
    v.get(k).and_then(Value::as_str).ok_or(Error::Invalid)
}
fn number(v: &Value, k: &str) -> Result<i64> {
    v.get(k).and_then(Value::as_i64).ok_or(Error::Invalid)
}
fn boolean(v: &Value, k: &str) -> Result<bool> {
    v.get(k).and_then(Value::as_bool).ok_or(Error::Invalid)
}
fn label(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && s.len() <= max && !s.chars().any(char::is_control)
}
fn stamp(s: &str) -> Result<i64> {
    chrono::DateTime::parse_from_rfc3339(&s.replace(' ', "T"))
        .map(|s| s.timestamp())
        .map_err(|_| Error::Invalid)
}
fn optional_stamp(v: &Value, k: &str) -> Result<Value> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(Value::Null),
        Some(Value::String(s)) => Ok(json!(stamp(s)?)),
        _ => Err(Error::Invalid),
    }
}
fn sid(id: &str) -> String {
    format!("source:{}", sha(id.as_bytes()))
}
fn mid(id: &str) -> String {
    format!("memory:{id}")
}
fn did(id: &str) -> String {
    format!("document:{id}")
}
fn row_metadata(table: &str, r: &Value) -> Result<Value> {
    let fields: &[&str] = match table {
        "public.ma_source_bindings" => &[
            "id",
            "dataset_id",
            "scope_digest",
            "generation",
            "revision",
            "purged",
            "source_kind",
            "coverage",
        ],
        "public.ma_source_documents" => &[
            "evidence_id",
            "binding_id",
            "key_hash",
            "version",
            "source_at",
            "observed_at",
        ],
        "public.ma_items" => &["id", "kind", "revision", "deleted"],
        "public.ma_memory_claims" => &[
            "item_id",
            "identity",
            "origin",
            "state",
            "observed_at",
            "topic",
            "valid_from",
            "valid_until",
        ],
        "public.ma_memory_identities" => &["identity", "canonical_id", "item_id", "suppressed"],
        "public.ma_memory_supports" => &[
            "item_id",
            "binding_id",
            "key_hash",
            "version",
            "evidence_id",
            "active",
            "unit_id",
            "observed_at",
            "source_at",
        ],
        _ => &[],
    };
    let mut m = serde_json::Map::new();
    for key in fields {
        if let Some(v) = r.get(*key) {
            m.insert((*key).into(), v.clone());
        }
    }
    if table == "public.ma_items"
        && r.get("kind").and_then(Value::as_str) == Some("artifacts")
        && let Some(data) = r.get("data")
    {
        for key in ["subtitle", "type", "visibility_epoch", "evidence"] {
            if let Some(v) = data.get(key) {
                m.insert(key.into(), v.clone());
            }
        }
    }
    let value = Value::Object(m);
    if bytes(&value)?.len() > 2048 {
        return Err(Error::Limit);
    }
    Ok(value)
}
fn parse(raw: &[u8], owner: &str) -> Result<Snapshot> {
    if raw.len() as u64 > LIMIT {
        return Err(Error::Limit);
    }
    let mut lines = raw
        .split(|b| *b == b'\n')
        .filter(|l| !l.iter().all(u8::is_ascii_whitespace));
    let header: Header =
        serde_json::from_slice(lines.next().ok_or(Error::Invalid)?).map_err(|_| Error::Invalid)?;
    if header.kind != "header"
        || header.format != 1
        || header.purpose != "musuri-schema23-relations-v1"
        || header.source_schema != 23
        || header.owner != owner
        || !label(owner, 128)
        || !label(&header.deployment_id, 128)
        || !label(&header.source_version, 128)
        || !label(&header.snapshot, 256)
        || header.inventory.len() > 256
    {
        return Err(Error::Schema);
    }
    stamp(&header.exported_at)?;
    let inventory = header.inventory.iter().cloned().collect::<BTreeSet<_>>();
    if inventory.len() != header.inventory.len() {
        return Err(Error::Invalid);
    }
    let all = lines.collect::<Vec<_>>();
    let complete: Complete =
        serde_json::from_slice(all.last().ok_or(Error::Invalid)?).map_err(|_| Error::Invalid)?;
    if complete.kind != "complete"
        || complete.snapshot != header.snapshot
        || complete.relations != inventory.len()
        || all.len() != inventory.len() + 1
    {
        return Err(Error::Invalid);
    }
    let mut tables = BTreeMap::new();
    let mut outcomes = vec![];
    let mut total = 0usize;
    for line in &all[..all.len() - 1] {
        let r: Relation = serde_json::from_slice(line).map_err(|_| Error::Invalid)?;
        if r.kind != "relation"
            || r.count != r.rows.len()
            || !inventory.contains(&Identity {
                schema: r.schema.clone(),
                name: r.name.clone(),
            })
            || !label(&r.schema, 128)
            || !label(&r.name, 128)
        {
            return Err(Error::Invalid);
        }
        total = total.checked_add(r.rows.len()).ok_or(Error::Limit)?;
        if total > ROW_LIMIT {
            return Err(Error::Limit);
        }
        let name = format!("{}.{}", r.schema, r.name);
        if tables.contains_key(&name) {
            return Err(Error::Invalid);
        }
        for (index, row) in r.rows.iter().enumerate() {
            if !row.is_object() {
                return Err(Error::Invalid);
            }
            outcomes.push(RowResult {
                relation: name.clone(),
                index,
                row_sha256: sha(&bytes(row)?),
                decision: "quarantined".into(),
                reason: "unsupported_relation_or_unproved_row".into(),
                target_legacy_id: None,
                legacy_revision: None,
                new_revision: None,
                metadata: row_metadata(&name, row)?,
            });
        }
        tables.insert(name, r.rows);
    }
    let required = [
        "ma_meta",
        "ma_schema_migrations",
        "ma_items",
        "ma_source_bindings",
        "ma_source_documents",
        "ma_source_forgets",
        "ma_memory_claims",
        "ma_memory_identities",
        "ma_memory_supports",
    ];
    if required
        .iter()
        .any(|n| !tables.contains_key(&format!("public.{n}")))
    {
        return Err(Error::Schema);
    }
    let known: Vec<String> = serde_json::from_str(include_str!(
        "../../../tools/legacy/schema23-table-names.json"
    ))
    .map_err(|_| Error::Schema)?;
    if known
        .iter()
        .any(|n| !tables.contains_key(&format!("public.{n}")))
    {
        return Err(Error::Schema);
    }
    let expected: Vec<Checksum> = serde_json::from_str(include_str!(
        "../../../tools/legacy/schema23-checksums.json"
    ))
    .map_err(|_| Error::Schema)?;
    let actual = tables
        .get("public.ma_schema_migrations")
        .ok_or(Error::Schema)?;
    if actual.len() != 23 || expected.len() != 23 {
        return Err(Error::Schema);
    }
    let mut seen = BTreeSet::new();
    for row in actual {
        let n = text(row, "name")?;
        let checksum = text(row, "checksum")?;
        if !seen.insert(n) || !expected.iter().any(|e| e.name == n && e.sha256 == checksum) {
            return Err(Error::Schema);
        }
    }
    Ok(Snapshot {
        header,
        tables,
        outcomes,
        sha: sha(raw),
    })
}
impl Snapshot {
    fn rows(&self, table: &str) -> &[Value] {
        self.tables
            .get(&format!("public.{table}"))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
    fn mark(
        &mut self,
        table: &str,
        index: usize,
        decision: &str,
        reason: &str,
        target: Option<String>,
        revision: Option<i64>,
    ) {
        if let Some(r) = self
            .outcomes
            .iter_mut()
            .find(|r| r.relation == format!("public.{table}") && r.index == index)
        {
            r.decision = decision.into();
            r.reason = reason.into();
            r.target_legacy_id = target;
            r.legacy_revision = revision;
            r.new_revision = if decision == "converted" {
                Some(1)
            } else {
                None
            };
        }
    }
}
fn unique(rows: &[Value], field: &str) -> Result<BTreeMap<String, (usize, Value)>> {
    let mut result = BTreeMap::new();
    for (i, r) in rows.iter().enumerate() {
        let id = text(r, field)?;
        if result.insert(id.to_owned(), (i, r.clone())).is_some() {
            return Err(Error::Invalid);
        }
    }
    Ok(result)
}
fn event(owner: &str, id: String, op: Value) -> Value {
    json!({"owner":owner,"id":id,"revision":1,"operation":op})
}

fn convert(mut s: Snapshot) -> Result<(Value, AdapterReport)> {
    let owner = s.header.owner.clone();
    let meta = s.rows("ma_meta");
    if meta.len() != 1 || number(&meta[0], "id")? != 1 {
        return Err(Error::Schema);
    }
    let epoch = number(&meta[0], "memory_epoch")?;
    let watermark = json!({"memory_epoch":epoch,"journal_sequence":meta[0].get("journal_sequence"),"journal_digest":meta[0].get("journal_digest"),"items_revision":meta[0].get("items_revision")});
    let bindings = unique(s.rows("ma_source_bindings"), "id")?;
    let documents = unique(s.rows("ma_source_documents"), "evidence_id")?;
    let claims = unique(s.rows("ma_memory_claims"), "item_id")?;
    let identities = unique(s.rows("ma_memory_identities"), "identity")?;
    let forgets = s
        .rows("ma_source_forgets")
        .iter()
        .map(|r| text(r, "binding_id").map(str::to_owned))
        .collect::<Result<BTreeSet<_>>>()?;
    let items = s.rows("ma_items").to_vec();
    let supports = s.rows("ma_memory_supports").to_vec();
    let mut seen_items = BTreeSet::new();
    for r in &items {
        let k = (text(r, "kind")?, text(r, "id")?);
        if !seen_items.insert(k) {
            return Err(Error::Invalid);
        }
    }
    // Hash-only forgotten identities cannot be translated into new extraction
    // suppressions. Retain current owner corrections, but withhold implicated raw
    // sources. Missing/ambiguous support scope requires withholding every source.
    let mut suppressed = BTreeSet::new();
    for (_, r) in identities.values() {
        if boolean(r, "suppressed")? {
            suppressed.insert(text(r, "item_id")?.to_owned());
        }
    }
    for r in &items {
        if text(r, "kind")? == "memories" && boolean(r, "deleted")? {
            suppressed.insert(text(r, "id")?.to_owned());
        }
    }
    for (_, r) in claims.values() {
        if text(r, "state")? != "active" {
            suppressed.insert(text(r, "item_id")?.to_owned());
        }
    }
    let mut blocked_sources = BTreeSet::new();
    let mut block_all = false;
    for id in &suppressed {
        let rows = supports
            .iter()
            .filter(|r| r.get("item_id").and_then(Value::as_str) == Some(id.as_str()))
            .collect::<Vec<_>>();
        if rows.is_empty() {
            block_all = true;
            continue;
        }
        for r in rows {
            let pair = text(r, "binding_id")
                .and_then(|b| text(r, "key_hash").map(|k| (b.to_owned(), k.to_owned())));
            match pair {
                Ok(p) if label(&p.0, 1024) && label(&p.1, 128) && bindings.contains_key(&p.0) => {
                    blocked_sources.insert(p);
                }
                _ => block_all = true,
            }
        }
    }
    let mut events = vec![];
    let mut admitted_sources = BTreeMap::<String, (String, Value)>::new();
    let mut converted = 0;
    for (evidence, (i, d)) in &documents {
        let candidate = (|| -> Result<Value> {
            let b = text(d, "binding_id")?;
            let (_, binding) = bindings.get(b).ok_or(Error::Invalid)?;
            if boolean(binding, "purged")?
                || text(binding, "generation")?.is_empty()
                || forgets.contains(b)
            {
                return Err(Error::Invalid);
            }
            let key = text(d, "key_hash")?;
            if block_all || blocked_sources.contains(&(b.to_owned(), key.to_owned())) {
                return Err(Error::Invalid);
            }
            let body = text(d, "body")?;
            let title = text(d, "title")?;
            let version = text(d, "version")?;
            if !label(evidence, 1024)
                || !label(key, 128)
                || !label(version, 128)
                || body.trim().is_empty()
                || body.len() > 65536
                || !label(title, 200)
                || body.chars().any(|c| c.is_control() && !c.is_whitespace())
            {
                return Err(Error::Invalid);
            }
            Ok(
                json!({"kind":"source_upsert","scope":format!("legacy-binding:{}",sha(b.as_bytes())),"external_id":key,"version":version,"title":title,"text":body,"speaker":null,"observed_at":stamp(text(d,"observed_at")?)?,"valid_from":null,"valid_until":null}),
            )
        })();
        if let Ok(op) = candidate {
            let id = sid(evidence);
            events.push(event(&owner, id.clone(), op));
            admitted_sources.insert(evidence.clone(), (id.clone(), d.clone()));
            converted += 1;
            s.mark(
                "ma_source_documents",
                *i,
                "converted",
                "current_source_projection_no_provider_authority",
                Some(id),
                None,
            );
        } else {
            s.mark(
                "ma_source_documents",
                *i,
                "quarantined",
                if block_all
                    || blocked_sources.contains(&(
                        text(d, "binding_id").unwrap_or("").into(),
                        text(d, "key_hash").unwrap_or("").into(),
                    ))
                {
                    "source_withheld_for_untranslatable_legacy_suppression"
                } else {
                    "source_binding_version_content_or_reference_invalid"
                },
                None,
                None,
            );
        }
    }
    let mut admitted_memories = BTreeMap::<String, i64>::new();
    for (i, item) in items
        .iter()
        .enumerate()
        .filter(|(_, r)| r.get("kind").and_then(Value::as_str) == Some("memories"))
    {
        let id = text(item, "id")?;
        let rev = number(item, "revision")?;
        if Uuid::parse_str(id).is_err() || rev < 1 {
            return Err(Error::Invalid);
        }
        let candidate = (|| -> Result<(Value, String)> {
            let c = claims.get(id).map(|(_, r)| r);
            let gone = boolean(item, "deleted")?
                || c.is_some_and(|r| r.get("state").and_then(Value::as_str) != Some("active"));
            if gone {
                return Ok((
                    json!({"kind":"memory_forget"}),
                    "withdrawn_snapshot_no_native_identity_suppression".into(),
                ));
            }
            let c = c.ok_or(Error::Invalid)?;
            let identity = text(c, "identity")?;
            let (_, identity_row) = identities.get(identity).ok_or(Error::Invalid)?;
            if text(identity_row, "item_id")? != id || boolean(identity_row, "suppressed")? {
                return Err(Error::Invalid);
            }
            let data = item
                .get("data")
                .and_then(Value::as_object)
                .ok_or(Error::Invalid)?;
            for key in ["value", "subject", "predicate", "speaker", "origin"] {
                if data.get(key) != c.get(key) {
                    return Err(Error::Invalid);
                }
            }
            let value = text(c, "value")?;
            let subject = format!("{}: {}", text(c, "subject")?, text(c, "predicate")?);
            if value.trim().is_empty()
                || value.len() > 4096
                || !label(&subject, 200)
                || value.chars().any(|c| c.is_control() && !c.is_whitespace())
            {
                return Err(Error::Invalid);
            }
            let from = optional_stamp(c, "valid_from")?;
            let until = optional_stamp(c, "valid_until")?;
            if from
                .as_i64()
                .zip(until.as_i64())
                .is_some_and(|(a, b)| a >= b)
                || !label(text(c, "speaker")?, 200)
            {
                return Err(Error::Invalid);
            }
            for key in ["valid_from", "valid_until"] {
                if optional_stamp(&item["data"], key)? != optional_stamp(c, key)? {
                    return Err(Error::Invalid);
                }
            }
            let mut refs = vec![];
            let mut seen_refs = BTreeSet::new();
            let origin = text(c, "origin")?;
            if !matches!(origin, "owner" | "source") {
                return Err(Error::Invalid);
            }
            for r in supports.iter().filter(|r| {
                r.get("item_id").and_then(Value::as_str) == Some(id)
                    && r.get("active").and_then(Value::as_bool) == Some(true)
            }) {
                let evidence = text(r, "evidence_id")?;
                let (source, d) = admitted_sources.get(evidence).ok_or(Error::Invalid)?;
                for key in ["binding_id", "key_hash", "version"] {
                    if r.get(key) != d.get(key) {
                        return Err(Error::Invalid);
                    }
                }
                let quote = text(r, "quote")?;
                let body = text(d, "body")?;
                if quote.is_empty() {
                    return Err(Error::Invalid);
                }
                let matches = body.match_indices(quote).collect::<Vec<_>>();
                if matches.len() != 1 {
                    return Err(Error::Invalid);
                }
                let start = matches[0].0;
                if !seen_refs.insert((source.clone(), start, start + quote.len())) {
                    return Err(Error::Invalid);
                }
                refs.push(json!({"source":source,"revision":1,"start_byte":start,"end_byte":start+quote.len(),"quote":quote}));
            }
            if refs.len() > 8 || (origin == "source" && refs.is_empty()) {
                return Err(Error::Invalid);
            }
            Ok((
                json!({"kind":"memory_upsert","subject":subject,"text":value,"speaker":text(c,"speaker")?,"valid_from":optional_stamp(c,"valid_from")?,"valid_until":optional_stamp(c,"valid_until")?,"evidence":refs}),
                "current_claim_only_legacy_revision_mapped_to_one".into(),
            ))
        })();
        match candidate {
            Ok((op, reason)) => {
                let active = op["kind"] == "memory_upsert";
                events.push(event(&owner, mid(id), op));
                converted += 1;
                if active {
                    admitted_memories.insert(id.into(), rev);
                }
                s.mark(
                    "ma_items",
                    i,
                    "converted",
                    &reason,
                    Some(mid(id)),
                    Some(rev),
                );
                if let Some((ci, _)) = claims.get(id) {
                    s.mark(
                        "ma_memory_claims",
                        *ci,
                        "support",
                        "joined_current_projection_or_withdrawal",
                        Some(mid(id)),
                        Some(rev),
                    );
                }
            }
            Err(_) => s.mark(
                "ma_items",
                i,
                "quarantined",
                "claim_projection_support_or_current_identity_invalid",
                None,
                Some(rev),
            ),
        }
    }
    let mut pending = items
        .iter()
        .enumerate()
        .filter(|(_, r)| r.get("kind").and_then(Value::as_str) == Some("artifacts"))
        .map(|(i, r)| (i, r.clone()))
        .collect::<Vec<_>>();
    let mut admitted_docs = BTreeMap::<String, i64>::new();
    let mut doc_bounds = BTreeMap::<String, (usize, usize)>::new();
    while !pending.is_empty() {
        let mut again = vec![];
        let mut progress = false;
        for (index, item) in pending {
            let id = text(&item, "id")?;
            let revision = number(&item, "revision")?;
            if Uuid::parse_str(id).is_err() || revision < 1 {
                return Err(Error::Invalid);
            }
            let candidate = (|| -> Result<Option<Value>> {
                if boolean(&item, "deleted")? {
                    return Ok(Some(json!({"kind":"document_delete"})));
                }
                let d = item.get("data").ok_or(Error::Invalid)?;
                if d.get("visibility_epoch")
                    .is_some_and(|v| v.as_i64() != Some(epoch))
                {
                    return Err(Error::Invalid);
                }
                if !matches!(text(d, "type")?, "plan" | "note" | "reading") {
                    return Err(Error::Invalid);
                }
                let title = text(d, "title")?;
                let body = text(d, "body")?;
                if !label(title, 200)
                    || body.len() > 30000
                    || body.chars().any(|c| c.is_control() && !c.is_whitespace())
                {
                    return Err(Error::Invalid);
                }
                let evidence = d.get("evidence").cloned().unwrap_or_else(|| json!([]));
                let evidence = evidence.as_array().ok_or(Error::Invalid)?;
                if evidence.len() > 12 {
                    return Err(Error::Invalid);
                }
                let mut refs = vec![];
                let mut depth = 1usize;
                let mut cost = 0usize;
                for r in evidence {
                    cost += 1;
                    let rid = text(r, "id")?;
                    match text(r, "kind")? {
                        "source" => {
                            let (sid, source) = admitted_sources.get(rid).ok_or(Error::Invalid)?;
                            if r.get("version") != source.get("version") {
                                return Err(Error::Invalid);
                            }
                            refs.push(json!({"kind":"source","id":sid,"revision":1}));
                        }
                        "memory" => {
                            if admitted_memories.get(rid).copied() != Some(number(r, "revision")?) {
                                return Err(Error::Invalid);
                            }
                            refs.push(json!({"kind":"memory","id":mid(rid),"revision":1}));
                        }
                        "artifact" => {
                            if let Some(rev) = admitted_docs.get(rid) {
                                let (d, c) = doc_bounds.get(rid).ok_or(Error::Invalid)?;
                                depth = depth.max(d + 1);
                                cost += c;
                                if *rev != number(r, "revision")? {
                                    return Err(Error::Invalid);
                                }
                                refs.push(json!({"kind":"document","id":did(rid),"revision":1}));
                            } else if items.iter().any(|v| {
                                v.get("kind").and_then(Value::as_str) == Some("artifacts")
                                    && v.get("id").and_then(Value::as_str) == Some(rid)
                            }) {
                                return Ok(None);
                            } else {
                                return Err(Error::Invalid);
                            }
                        }
                        _ => return Err(Error::Invalid),
                    }
                }
                if depth > 8 || cost > 32 {
                    return Err(Error::Invalid);
                }
                doc_bounds.insert(id.to_owned(), (depth, cost));
                Ok(Some(
                    json!({"kind":"document_upsert","title":title,"body":body,"references":refs}),
                ))
            })();
            match candidate {
                Ok(Some(op)) => {
                    let active = op["kind"] == "document_upsert";
                    events.push(event(&owner, did(id), op));
                    converted += 1;
                    if active {
                        admitted_docs.insert(id.into(), revision);
                    }
                    s.mark(
                        "ma_items",
                        index,
                        "converted",
                        "current_plain_artifact_snapshot_no_history_invention",
                        Some(did(id)),
                        Some(revision),
                    );
                    progress = true;
                }
                Ok(None) => again.push((index, item)),
                Err(_) => {
                    s.mark(
                        "ma_items",
                        index,
                        "quarantined",
                        "artifact_visibility_type_or_reference_invalid",
                        None,
                        Some(revision),
                    );
                    progress = true;
                }
            }
        }
        if !progress {
            for (i, _) in again {
                s.mark(
                    "ma_items",
                    i,
                    "quarantined",
                    "artifact_cycle_or_unconverted_dependency",
                    None,
                    None,
                );
            }
            break;
        }
        pending = again;
    }
    let source_scopes = admitted_sources
        .values()
        .filter_map(|(_, r)| r.get("binding_id").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    if source_scopes.len() > 32 {
        return Err(Error::Limit);
    }
    if converted > CONVERTED_LIMIT {
        return Err(Error::Limit);
    }
    // Mark only structural/support rows; unsupported authority stays quarantined.
    for table in [
        "ma_meta",
        "ma_schema_migrations",
        "ma_source_bindings",
        "ma_source_forgets",
        "ma_memory_identities",
        "ma_memory_supports",
    ] {
        for i in 0..s.rows(table).len() {
            s.mark(
                table,
                i,
                "support",
                "retained_snapshot_structure_or_suppression_evidence_not_authority",
                None,
                None,
            );
        }
    }
    let relations = s.tables.iter().map(|(k, v)| (k.clone(), v.len())).collect();
    let quarantined = s
        .outcomes
        .iter()
        .filter(|r| r.decision == "quarantined")
        .count();
    let supporting = s
        .outcomes
        .iter()
        .filter(|r| r.decision == "support")
        .count();
    let report = AdapterReport {
        purpose: "musuri-schema23-adapter-report-v1".into(),
        snapshot_sha256: s.sha,
        source_schema: 23,
        total_rows: s.outcomes.len(),
        converted_objects: converted,
        quarantined_rows: quarantined,
        supporting_rows: supporting,
        source_admission: if block_all {
            "all_sources_withheld_unscoped_legacy_suppression"
        } else if !blocked_sources.is_empty() {
            "scoped_sources_withheld_for_legacy_suppression"
        } else {
            "current_sources_only_no_legacy_authority"
        }
        .into(),
        snapshot_current_projection_only: true,
        relations,
        rows: s.outcomes,
    };
    let overview = json!({"purpose":report.purpose,"snapshot_sha256":report.snapshot_sha256,"source_schema":23,"owner_binding":"explicit single-owner legacy deployment, not a row owner column","deployment_id":s.header.deployment_id,"source_version":s.header.source_version,"exported_at":s.header.exported_at,"transaction_snapshot":s.header.snapshot,"journal_watermark":watermark,"total_rows":report.total_rows,"converted_objects":converted,"quarantined_rows":quarantined,"supporting_rows":supporting,"source_admission":report.source_admission,"projection_policy":"legacy current revision N becomes new revision1; no prior value/history invented; withdrawn identities are report barriers, implicated source admission withheld","relations":report.relations});
    let mut pages = vec![overview];
    let mut chunk = Vec::<Value>::new();
    for row in &report.rows {
        let value = serde_json::to_value(row).map_err(|_| Error::Invalid)?;
        let mut candidate = chunk.clone();
        candidate.push(value.clone());
        if bytes(&candidate)?.len() > 24000 && !chunk.is_empty() {
            pages.push(json!({"purpose":"legacy-relation-row-reconciliation-v1","snapshot_sha256":report.snapshot_sha256,"rows":chunk}));
            chunk = vec![];
        }
        chunk.push(value);
    }
    if !chunk.is_empty() {
        pages.push(json!({"purpose":"legacy-relation-row-reconciliation-v1","snapshot_sha256":report.snapshot_sha256,"rows":chunk}));
    }
    for (i, page) in pages.into_iter().enumerate() {
        events.push(event(&owner,format!("adapter-report-{i}"),json!({"kind":"document_upsert","title":format!("Legacy schema23 adapter report part {}",i+1),"body":String::from_utf8(bytes(&page)?).map_err(|_|Error::Invalid)?,"references":[]})));
    }
    if events.len() > 512 {
        return Err(Error::Limit);
    }
    for (i, e) in events.iter_mut().enumerate() {
        e["sequence"] = json!(i + 1);
    }
    let transfer = json!({"format":1,"purpose":"moosuri-private-transfer-v1","bundle_id":Uuid::new_v4().to_string(),"source_manifest":{"deployment_id":s.header.deployment_id,"source_version":s.header.source_version,"source_schema":"source-grounded-musuri-schema23-current-relations","exported_at":stamp(&s.header.exported_at)?,"coverage":format!("current projection only; snapshot SHA256 {}; {} rows accounted; {} quarantined; {}",report.snapshot_sha256,report.total_rows,quarantined,report.source_admission)},"events":events});
    Ok((transfer, report))
}
/// Direct supported use path: relation snapshot -> accepted importer -> new root.
/// No credentials/DSN/private source/old runtime executable are ever consulted.
pub async fn import_snapshot(
    input: &Path,
    owner: &str,
    enrollment: Enrollment,
    dest: &Path,
    reports: &Path,
) -> Result<Imported> {
    let raw = Zeroizing::new({
        let mut v = vec![];
        backup::files::read_file(input, LIMIT)
            .map_err(|_| Error::Invalid)?
            .take(LIMIT + 1)
            .read_to_end(&mut v)
            .map_err(|_| Error::Invalid)?;
        v
    });
    let snapshot = parse(&raw, owner)?;
    let (transfer, adapter) = convert(snapshot)?;
    let temp = backup::files::Temporary::new(dest).map_err(|_| Error::Output)?;
    let intermediate = temp.0.join("private-transfer.json");
    let encoded = Zeroizing::new(bytes(&transfer)?);
    if encoded.len() as u64 > LIMIT {
        return Err(Error::Limit);
    }
    let mut file = backup::files::create_file(&intermediate).map_err(|_| Error::Output)?;
    file.write_all(&encoded).map_err(|_| Error::Output)?;
    file.sync_all().map_err(|_| Error::Output)?;
    drop(file);
    let adapter_json = serde_json::to_value(&adapter).map_err(|_| Error::Output)?;
    let report = legacy_import::import_bundle_with_relation_report(
        &intermediate,
        owner,
        enrollment,
        dest,
        reports,
        Some(&adapter_json),
    )
    .await
    .map_err(|_| Error::Import)?;
    Ok(Imported { report, adapter })
}
#[cfg(test)]
mod tests;
