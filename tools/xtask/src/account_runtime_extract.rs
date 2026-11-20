//! Synthetic offline extraction of one owner's manual knowledge graph. This
//! creates an inert archive, never a Store, runtime volume or recovery authority.
use crate::{Check, account_runtime_inventory as inventory, ensure};
use rusqlite::{
    Connection, params,
    types::{Value as Cell, ValueRef},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
const SCOPE: &str = "manual-knowledge-v1";
const APP_ID: i64 = 0x4d535845;
const MAX_BYTES: u64 = 128 * 1024 * 1024;
const TABLES: [&str; 11] = [
    "knowledge_state",
    "knowledge_collections",
    "knowledge_sources",
    "knowledge_source_versions",
    "knowledge_memories",
    "knowledge_memory_versions",
    "knowledge_memory_timeline",
    "knowledge_evidence",
    "knowledge_writes",
    "knowledge_suppressions",
    "knowledge_batches",
];
const BLOCKED: [&str; 9] = [
    "source_connections",
    "source_provenance",
    "source_batch_receipts",
    "source_sync_runs",
    "source_run_starts",
    "automatic_memory_grants",
    "automatic_memory_jobs",
    "automatic_memory_facts",
    "automatic_memory_origins",
];
const OWNER_DDL: &str = "CREATE TABLE extraction_owner(singleton INTEGER PRIMARY KEY CHECK(singleton=1),scope TEXT NOT NULL,account_id TEXT NOT NULL,tenant_id TEXT NOT NULL,volume_id TEXT NOT NULL,generation INTEGER NOT NULL CHECK(generation>0),source_schema INTEGER NOT NULL,source_sha256 TEXT NOT NULL,inventory_schema_sha256 TEXT NOT NULL) STRICT;";
const ACCOUNT_DDL: &str = "CREATE TABLE accounts(id TEXT PRIMARY KEY NOT NULL,tenant_id TEXT NOT NULL UNIQUE REFERENCES tenants(id),name TEXT NOT NULL,email TEXT NOT NULL,UNIQUE(tenant_id,id)) STRICT;";
fn sql<T>(r: rusqlite::Result<T>) -> Check<T> {
    r.map_err(|_| "SQLite extraction check failed (no source values emitted)".into())
}
fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
fn field<'a>(v: &'a Value, key: &str) -> Check<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("invalid extraction manifest {key}"))
}

fn file_hash(path: &Path) -> Check<String> {
    let mut f = File::open(path).map_err(|_| "archive file unavailable")?;
    let mut h = Sha256::new();
    let mut b = [0; 32768];
    loop {
        let n = f.read(&mut b).map_err(|_| "archive file unreadable")?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
fn uuid(s: &str) -> Check<String> {
    let b = s.as_bytes();
    ensure(
        b.len() == 36
            && b.iter().enumerate().all(|(i, c)| {
                if [8, 13, 18, 23].contains(&i) {
                    *c == b'-'
                } else {
                    c.is_ascii_hexdigit()
                }
            })
            && s != "00000000-0000-0000-0000-000000000000",
        "expected non-nil UUID",
    )?;
    Ok(s.to_ascii_lowercase())
}
fn private_dir(p: &Path) -> Check<()> {
    let m = fs::symlink_metadata(p).map_err(|_| "private directory unavailable")?;
    ensure(
        p.is_absolute()
            && m.is_dir()
            && m.uid() == unsafe { libc::geteuid() }
            && m.mode() & 0o077 == 0
            && fs::canonicalize(p).ok().as_deref() == Some(p),
        "directory must be canonical, owned and private",
    )
}
fn private_file(p: &Path, cap: u64) -> Check<()> {
    let m = fs::symlink_metadata(p).map_err(|_| "private file unavailable")?;
    ensure(
        m.is_file()
            && m.nlink() == 1
            && m.uid() == unsafe { libc::geteuid() }
            && m.mode() & 0o077 == 0
            && m.len() <= cap,
        "file must be regular, owned, private, single-link and bounded",
    )
}
fn new_file(p: &Path) -> Check<File> {
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .read(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(p)
        .map_err(|_| "cannot create new private artifact".into())
}
fn manifest_table<'a>(manifest: &'a Value, name: &str) -> Check<&'a Value> {
    manifest["tables"]
        .as_array()
        .and_then(|v| v.iter().find(|t| t["name"] == name))
        .ok_or_else(|| format!("unclassified table {name}"))
}
fn count(c: &Connection, query: &str) -> Check<i64> {
    sql(c.query_row(query, [], |r| r.get(0)))
}
#[derive(Clone)]
struct Owner {
    account: String,
    tenant: String,
    volume: String,
    generation: i64,
}
impl Owner {
    fn value(&self) -> Value {
        json!({"account_id":self.account,"tenant_id":self.tenant,"volume_id":self.volume,"generation":self.generation})
    }
    fn from_value(v: &Value) -> Check<Self> {
        let generation = v["generation"]
            .as_i64()
            .filter(|n| *n > 0)
            .ok_or("invalid owner generation")?;
        Ok(Self {
            account: uuid(field(v, "account_id")?)?,
            tenant: uuid(field(v, "tenant_id")?)?,
            volume: uuid(field(v, "volume_id")?)?,
            generation,
        })
    }
}
fn columns(manifest: &Value, table: &str) -> Check<Vec<String>> {
    manifest_table(manifest, table)?["columns"]
        .as_array()
        .ok_or("missing classified columns")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or("invalid classified column".into())
        })
        .collect()
}
fn owner_query(manifest: &Value, table: &str) -> Check<String> {
    Ok(field(manifest_table(manifest, table)?, "owner_query")?.to_owned())
}
fn owner_count(c: &Connection, manifest: &Value, table: &str, owner: &Owner) -> Check<i64> {
    let query = owner_query(manifest, table)?;
    if query.is_empty() {
        return Ok(0);
    }
    sql(c.query_row(
        &format!("SELECT count(*) FROM ({query}) WHERE account_id=?1 AND tenant_id=?2"),
        params![owner.account, owner.tenant],
        |r| r.get(0),
    ))
}
fn schema(source: &Connection, target: &Connection) -> Check<()> {
    sql(target.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;"))?;
    let tenant: String = sql(source.query_row(
        "SELECT sql FROM sqlite_schema WHERE name='tenants'",
        [],
        |r| r.get(0),
    ))?;
    sql(target.execute_batch(&tenant))?;
    sql(target.execute_batch(ACCOUNT_DDL))?;
    sql(target.execute_batch(OWNER_DDL))?;
    for table in TABLES.into_iter().chain(["knowledge_fts"]) {
        let ddl: String = sql(source.query_row(
            "SELECT sql FROM sqlite_schema WHERE type='table' AND name=?1",
            [table],
            |r| r.get(0),
        ))?;
        sql(target.execute_batch(&ddl))?;
    }
    sql(target.execute_batch("CREATE INDEX knowledge_evidence_source ON knowledge_evidence(tenant_id,source_id,source_revision);"))?;
    sql(target.pragma_update(None, "application_id", APP_ID))?;
    sql(target.pragma_update(None, "user_version", 1))?;
    Ok(())
}
fn row_hash(
    c: &Connection,
    table: &str,
    cols: &[String],
    query: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Check<(i64, String, u64)> {
    let mut h = Sha256::new();
    h.update(b"moosuri/account-extraction/rows/v1\0");
    h.update(table.as_bytes());
    for col in cols {
        h.update([0]);
        h.update(col.as_bytes());
    }
    let mut stmt = sql(c.prepare(query))?;
    let mut rows = sql(stmt.query(args))?;
    let mut n = 0;
    let mut bytes = 0;
    while let Some(row) = sql(rows.next())? {
        n += 1;
        ensure(n <= 100000, "extraction row bound exceeded")?;
        h.update([255]);
        for i in 0..cols.len() {
            let value = sql(row.get_ref(i))?;
            match value {
                ValueRef::Null => h.update([0]),
                ValueRef::Integer(v) => {
                    h.update([1]);
                    h.update(v.to_le_bytes());
                }
                ValueRef::Real(v) => {
                    h.update([2]);
                    h.update(v.to_bits().to_le_bytes());
                }
                ValueRef::Text(v) | ValueRef::Blob(v) => {
                    bytes += v.len() as u64;
                    ensure(
                        bytes <= MAX_BYTES && v.len() <= 1024 * 1024,
                        "extraction payload bound exceeded",
                    )?;
                    h.update([if matches!(value, ValueRef::Text(_)) {
                        3
                    } else {
                        4
                    }]);
                    h.update((v.len() as u64).to_le_bytes());
                    h.update(v);
                }
            }
        }
    }
    Ok((n, format!("{:x}", h.finalize()), bytes))
}
fn all_query(table: &str, cols: &[String]) -> String {
    let cols = cols.iter().map(|s| q(s)).collect::<Vec<_>>().join(",");
    format!("SELECT {cols} FROM {} ORDER BY {cols}", q(table))
}
fn selected_query(manifest: &Value, table: &str, cols: &[String]) -> Check<String> {
    let fields = cols
        .iter()
        .map(|s| format!("t.{}", q(s)))
        .collect::<Vec<_>>()
        .join(",");
    let owner = owner_query(manifest, table)?;
    Ok(format!(
        "SELECT {fields} FROM {} t WHERE t.rowid IN(SELECT rid FROM ({owner}) WHERE account_id=?1 AND tenant_id=?2) ORDER BY {fields}",
        q(table)
    ))
}
fn copy(
    source: &Connection,
    target: &Connection,
    manifest: &Value,
    table: &str,
    owner: &Owner,
    remaining: &mut u64,
) -> Check<Value> {
    let cols = columns(manifest, table)?;
    let query = selected_query(manifest, table, &cols)?;
    let params: &[&dyn rusqlite::ToSql] = &[&owner.account, &owner.tenant];
    let (n, fingerprint, bytes) = row_hash(source, table, &cols, &query, params)?;
    ensure(
        bytes <= *remaining,
        "total extraction payload bound exceeded",
    )?;
    *remaining -= bytes;
    let insert = format!(
        "INSERT INTO {}({}) VALUES({})",
        q(table),
        cols.iter().map(|s| q(s)).collect::<Vec<_>>().join(","),
        (1..=cols.len())
            .map(|n| format!("?{n}"))
            .collect::<Vec<_>>()
            .join(",")
    );
    let mut input = sql(source.prepare(&query))?;
    let mut input = sql(input.query(params))?;
    let mut output = sql(target.prepare(&insert))?;
    while let Some(row) = sql(input.next())? {
        let values = (0..cols.len())
            .map(|i| sql(row.get::<_, Cell>(i)))
            .collect::<Check<Vec<_>>>()?;
        sql(output.execute(rusqlite::params_from_iter(values.iter())))?;
    }
    let (copied, actual, _) = row_hash(target, table, &cols, &all_query(table, &cols), &[])?;
    ensure(
        copied == n && actual == fingerprint,
        format!("{table}: extraction row reconciliation failed"),
    )?;
    Ok(
        json!({"table":table,"rows":n,"rows_sha256":fingerprint,"payload_bytes":bytes,"disposition":"copied-owner-rows","columns":cols}),
    )
}
fn checkpoints(source: &Connection) -> Check<Value> {
    let mut result = serde_json::Map::new();
    for (table, seq, head) in [
        ("store_meta", "journal_seq", "journal_head"),
        ("knowledge_journal_meta", "sequence", "head"),
        ("destructive_journal_meta", "sequence", "head"),
    ] {
        let value = sql(source
            .query_row(
                &format!("SELECT {seq},lower(hex({head})) FROM {table} WHERE singleton=1"),
                [],
                |r| Ok(json!({"sequence":r.get::<_,i64>(0)?,"head_sha256":r.get::<_,String>(1)?})),
            )
            .optional())?;
        result.insert(table.into(), value.unwrap_or(Value::Null));
    }
    Ok(json!({"heads":result,"authentication":"not-performed","global_journals_copied":false}))
}
use rusqlite::OptionalExtension;
fn normalized(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}
fn fts_records(c: &Connection, owner: &Owner) -> Check<Vec<(String, String, i64, String)>> {
    let mut records = Vec::new();
    let mut source=sql(c.prepare("SELECT s.id,s.revision,v.title,v.text FROM knowledge_sources s JOIN knowledge_collections g ON g.tenant_id=s.tenant_id AND g.id=s.collection_id JOIN knowledge_source_versions v ON v.tenant_id=s.tenant_id AND v.source_id=s.id AND v.revision=s.revision WHERE s.tenant_id=?1 AND s.deleted=0 AND g.deleted=0 ORDER BY s.id"))?;
    let rows = sql(source.query_map([&owner.tenant], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    }))?;
    for row in rows {
        let (id, rev, title, text) = sql(row)?;
        records.push((
            "source".into(),
            id,
            rev,
            normalized(&format!("{title}\n{text}")),
        ));
    }
    let mut memories=sql(c.prepare("SELECT v.memory_id,v.revision,v.draft FROM knowledge_memory_versions v JOIN knowledge_memories m ON m.tenant_id=v.tenant_id AND m.id=v.memory_id WHERE v.tenant_id=?1 AND v.status='active' AND m.status!='forgotten' ORDER BY v.memory_id,v.revision"))?;
    let rows = sql(memories.query_map([&owner.tenant], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    }))?;
    for row in rows {
        let (id, rev, draft) = sql(row)?;
        let draft: Value = serde_json::from_str(&draft).map_err(|_| "invalid memory draft")?;
        records.push((
            "memory".into(),
            id,
            rev,
            normalized(&format!(
                "{}\n{}",
                field(&draft, "subject")?,
                field(&draft, "text")?
            )),
        ));
    }
    records.sort();
    Ok(records)
}
fn rebuild_fts(c: &Connection, owner: &Owner) -> Check<usize> {
    let records = fts_records(c, owner)?;
    let key = format!("t{}", owner.tenant.replace('-', ""));
    for (kind, id, rev, text) in &records {
        sql(c.execute("INSERT INTO knowledge_fts(tenant_key,kind,object_id,revision,normalized) VALUES(?1,?2,?3,?4,?5)",params![key,kind,id,rev,text]))?;
    }
    Ok(records.len())
}
fn exact_keys(v: &Value, expected: &[&str]) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == expected.len() && expected.iter().all(|k| o.contains_key(*k)))
}
fn knowledge_graph(c: &Connection, owner: &Owner) -> Check<()> {
    for query in [
        "SELECT count(*) FROM knowledge_sources s WHERE s.deleted=1 AND EXISTS(SELECT 1 FROM knowledge_source_versions v WHERE v.tenant_id=s.tenant_id AND v.source_id=s.id)",
        "SELECT count(*) FROM knowledge_memories m WHERE m.status='forgotten' AND (EXISTS(SELECT 1 FROM knowledge_memory_versions v WHERE v.tenant_id=m.tenant_id AND v.memory_id=m.id) OR EXISTS(SELECT 1 FROM knowledge_memory_timeline t WHERE t.tenant_id=m.tenant_id AND t.memory_id=m.id))",
        "SELECT count(*) FROM knowledge_sources s WHERE s.deleted=0 AND NOT EXISTS(SELECT 1 FROM knowledge_source_versions v WHERE v.tenant_id=s.tenant_id AND v.source_id=s.id AND v.revision=s.revision)",
        "SELECT count(*) FROM knowledge_sources s JOIN knowledge_collections g ON g.tenant_id=s.tenant_id AND g.id=s.collection_id WHERE g.deleted=1 AND s.deleted=0",
        "SELECT count(*) FROM knowledge_memories m WHERE m.status='active' AND NOT EXISTS(SELECT 1 FROM knowledge_memory_versions v WHERE v.tenant_id=m.tenant_id AND v.memory_id=m.id AND v.revision=m.revision AND v.status='active')",
    ] {
        ensure(
            count(c, query)? == 0,
            "knowledge deletion/current revision invariant failed",
        )?;
    }
    let mut stmt=sql(c.prepare("SELECT s.external_id,v.version,v.title,v.text,v.speaker,v.observed_at,v.valid_from,v.valid_until,v.digest FROM knowledge_source_versions v JOIN knowledge_sources s ON s.tenant_id=v.tenant_id AND s.id=v.source_id"))?;
    let mut rows = sql(stmt.query([]))?;
    while let Some(r) = sql(rows.next())? {
        let content = (
            sql(r.get::<_, String>(0))?,
            sql(r.get::<_, String>(1))?,
            sql(r.get::<_, String>(2))?,
            sql(r.get::<_, String>(3))?,
            sql(r.get::<_, Option<String>>(4))?,
            sql(r.get::<_, i64>(5))?,
            sql(r.get::<_, Option<i64>>(6))?,
            sql(r.get::<_, Option<i64>>(7))?,
        );
        let expected: Vec<u8> = sql(r.get(8))?;
        let bytes =
            serde_json::to_vec(&content).map_err(|_| "source fingerprint serialization failed")?;
        ensure(
            Sha256::digest(&bytes).as_slice() == expected,
            "source content fingerprint failed",
        )?;
    }
    let mut stmt =
        sql(c.prepare("SELECT memory_id,revision,draft FROM knowledge_memory_versions"))?;
    let mut rows = sql(stmt.query([]))?;
    while let Some(row) = sql(rows.next())? {
        let memory: String = sql(row.get(0))?;
        let revision: i64 = sql(row.get(1))?;
        let encoded: String = sql(row.get(2))?;
        let draft: Value = serde_json::from_str(&encoded).map_err(|_| "invalid memory draft")?;
        ensure(
            exact_keys(
                &draft,
                &[
                    "subject",
                    "text",
                    "speaker",
                    "valid_from",
                    "valid_until",
                    "evidence",
                ],
            ),
            "unsupported memory draft fields",
        )?;
        let subject = field(&draft, "subject")?;
        let text = field(&draft, "text")?;
        ensure(
            !subject.trim().is_empty()
                && subject.len() <= 200
                && !subject.chars().any(char::is_control)
                && !text.trim().is_empty()
                && text.len() <= 4096
                && !text.chars().any(|c| c.is_control() && !c.is_whitespace()),
            "unsupported memory text",
        )?;
        ensure(
            draft["speaker"].is_null()
                || draft["speaker"].as_str().is_some_and(|s| {
                    !s.trim().is_empty() && s.len() <= 200 && !s.chars().any(char::is_control)
                }),
            "unsupported memory speaker",
        )?;
        for key in ["valid_from", "valid_until"] {
            ensure(
                draft[key].is_null()
                    || draft[key]
                        .as_i64()
                        .is_some_and(|n| (-62_135_596_800..=253_402_300_799).contains(&n)),
                "unsupported memory validity",
            )?;
        }
        ensure(
            draft["valid_from"]
                .as_i64()
                .zip(draft["valid_until"].as_i64())
                .is_none_or(|(a, b)| a < b),
            "invalid memory validity range",
        )?;
        let evidence = draft["evidence"]
            .as_array()
            .ok_or("invalid evidence list")?;
        ensure(evidence.len() <= 8, "evidence bound exceeded")?;
        let mut expected = BTreeSet::new();
        for e in evidence {
            ensure(
                exact_keys(
                    e,
                    &[
                        "source_id",
                        "source_revision",
                        "start_byte",
                        "end_byte",
                        "quote",
                    ],
                ),
                "unsupported evidence fields",
            )?;
            let id = uuid(field(e, "source_id")?)?;
            let rev = e["source_revision"]
                .as_i64()
                .filter(|n| *n > 0)
                .ok_or("invalid evidence revision")?;
            let start = e["start_byte"].as_u64().ok_or("invalid evidence offset")?;
            let end = e["end_byte"].as_u64().ok_or("invalid evidence offset")?;
            let quote = field(e, "quote")?;
            ensure(
                start < end && end <= 65536 && !quote.is_empty() && quote.len() <= 4096,
                "invalid evidence bounds",
            )?;
            let source:String=sql(c.query_row("SELECT text FROM knowledge_source_versions WHERE tenant_id=?1 AND source_id=?2 AND revision=?3",params![owner.tenant,id,rev],|r|r.get(0)))?;
            ensure(
                source.get(start as usize..end as usize) == Some(quote),
                "evidence quote does not match retained owner source",
            )?;
            let quote_hash = Sha256::digest(quote.as_bytes()).to_vec();
            ensure(
                expected.insert((id, rev, start as i64, end as i64, quote_hash)),
                "duplicate embedded evidence",
            )?;
        }
        let mut stmt=sql(c.prepare("SELECT source_id,source_revision,start_byte,end_byte,quote_hash FROM knowledge_evidence WHERE tenant_id=?1 AND memory_id=?2 AND memory_revision=?3"))?;
        let actual = sql(
            stmt.query_map(params![owner.tenant, memory, revision], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, Vec<u8>>(4)?,
                ))
            }),
        )?
        .map(sql)
        .collect::<Check<BTreeSet<_>>>()?;
        ensure(
            actual == expected,
            "embedded/table evidence reconciliation failed",
        )?;
    }
    let mut stmt =
        sql(c.prepare("SELECT request_id,collection_id,receipt FROM knowledge_batches"))?;
    let mut rows = sql(stmt.query([]))?;
    while let Some(row) = sql(rows.next())? {
        let request: String = sql(row.get(0))?;
        let collection: String = sql(row.get(1))?;
        let receipt: String = sql(row.get(2))?;
        let receipt: Value = serde_json::from_str(&receipt).map_err(|_| "invalid batch receipt")?;
        ensure(
            exact_keys(&receipt, &["request_id", "cursor", "source_ids"])
                && field(&receipt, "request_id")? == request
                && receipt["cursor"].as_i64().is_some_and(|n| n >= 0),
            "unsupported batch receipt",
        )?;
        for id in receipt["source_ids"]
            .as_array()
            .ok_or("invalid batch source list")?
        {
            let id = uuid(id.as_str().ok_or("invalid batch source ID")?)?;
            let exists:i64=sql(c.query_row("SELECT count(*) FROM knowledge_sources WHERE tenant_id=?1 AND id=?2 AND collection_id=?3",params![owner.tenant,id,collection],|r|r.get(0)))?;
            ensure(exists == 1, "batch receipt source outside owner collection")?;
        }
    }
    ensure(
        count(c, "SELECT count(*) FROM pragma_foreign_key_check")? == 0,
        "extracted foreign-key integrity failed",
    )?;
    Ok(())
}
struct Cleanup {
    root: PathBuf,
    complete: bool,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.complete {
            for n in [
                "manifest.json",
                "knowledge.sqlite-journal",
                "knowledge.sqlite",
            ] {
                let _ = fs::remove_file(self.root.join(n));
            }
            let _ = fs::remove_dir(&self.root);
        }
    }
}
fn schema_hash(c: &Connection) -> Check<String> {
    let (_, shadows) = inventory::schema_tables(c)?;
    inventory::schema_digest(c, &shadows)
}
fn report_table(c: &Connection, table: &str, cols: &[&str], disposition: &str) -> Check<Value> {
    let cols = cols.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let (n, sha, bytes) = row_hash(c, table, &cols, &all_query(table, &cols), &[])?;
    Ok(
        json!({"table":table,"columns":cols,"rows":n,"rows_sha256":sha,"payload_bytes":bytes,"disposition":disposition}),
    )
}
fn extract(root: &Path, snapshot: &Path, output: &Path, owner: &Owner) -> Check<Value> {
    private_dir(snapshot)?;
    let entries = fs::read_dir(snapshot)
        .map_err(|_| "snapshot unavailable")?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|_| "snapshot unavailable")?;
    ensure(
        entries.len() == 1 && entries[0].file_name() == "assistant.sqlite",
        "extraction requires a DB-only stopped snapshot; runtime roots, keys, journals and sidecars are refused",
    )?;
    let path = snapshot.join("assistant.sqlite");
    private_file(&path, 512 * 1024 * 1024)?;
    let parent = output.parent().ok_or("output parent required")?;
    private_dir(parent)?;
    let name = output
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("output directory name required")?;
    ensure(
        ![".", "..", ""].contains(&name),
        "output directory name required",
    )?;
    let output = parent.join(name);
    ensure(
        !output.starts_with(snapshot) && !snapshot.starts_with(&output),
        "snapshot and output must be disjoint",
    )?;
    let before = file_hash(&path)?;
    let source = inventory::open_snapshot(&path)?;
    let source_schema = count(&source, "PRAGMA user_version")?;
    let manifest = inventory::load(root, source_schema)?;
    let baseline = inventory::replay(root, &manifest)?;
    inventory::inspect(&baseline, &manifest)?;
    let source_inventory = inventory::inspect(&source, &manifest)?;
    let active:i64=sql(source.query_row("SELECT count(*) FROM accounts WHERE id=?1 AND tenant_id=?2 AND deleted=0 AND status='active' AND recovery_required=0",params![owner.account,owner.tenant],|r|r.get(0)))?;
    ensure(
        active == 1,
        "source owner pair absent or not active/unquarantined",
    )?;
    for table in BLOCKED {
        ensure(
            owner_count(&source, &manifest, table, owner)? == 0,
            format!(
                "{table}: connected/automatic knowledge provenance is outside manual extraction scope"
            ),
        )?;
    }
    let mut excluded = Vec::new();
    for table in manifest["tables"]
        .as_array()
        .ok_or("invalid table manifest")?
    {
        let name = field(table, "name")?;
        if !TABLES.contains(&name) && !["accounts", "tenants", "knowledge_fts"].contains(&name) {
            excluded.push(json!({"table":name,"scope":table["scope"],"source_rows":source_inventory["tables"].as_array().and_then(|v|v.iter().find(|t|t["name"]==name)).ok_or("missing source table count")?["rows"],"owner_rows":if field(table,"owner_query")?.is_empty(){Value::Null}else{json!(owner_count(&source,&manifest,name,owner)?)},"disposition":"outside-requested-partial-scope"}));
        }
    }
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&output)
        .map_err(|_| "output exists or cannot be created; nothing overwritten")?;
    let mut cleanup = Cleanup {
        root: output.clone(),
        complete: false,
    };
    let dbpath = output.join("knowledge.sqlite");
    drop(new_file(&dbpath)?);
    let archive = sql(Connection::open(&dbpath))?;
    schema(&source, &archive)?;
    sql(archive.execute_batch("BEGIN IMMEDIATE;"))?;
    sql(archive.execute("INSERT INTO tenants SELECT ?1", [&owner.tenant]))?;
    let metadata: (String, String) = sql(source.query_row(
        "SELECT name,email FROM accounts WHERE id=?1 AND tenant_id=?2",
        params![owner.account, owner.tenant],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    sql(archive.execute(
        "INSERT INTO accounts VALUES(?1,?2,?3,?4)",
        params![owner.account, owner.tenant, metadata.0, metadata.1],
    ))?;
    sql(archive.execute(
        "INSERT INTO extraction_owner VALUES(1,?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            SCOPE,
            owner.account,
            owner.tenant,
            owner.volume,
            owner.generation,
            source_schema,
            before,
            field(&manifest, "schema_sha256")?
        ],
    ))?;
    let mut tables = vec![
        report_table(&archive, "tenants", &["id"], "copied-owner-identity")?,
        report_table(
            &archive,
            "accounts",
            &["id", "tenant_id", "name", "email"],
            "owner-metadata-projection-no-authentication-or-role",
        )?,
    ];
    let mut remaining = MAX_BYTES;
    for table in TABLES {
        tables.push(copy(
            &source,
            &archive,
            &manifest,
            table,
            owner,
            &mut remaining,
        )?);
    }
    knowledge_graph(&archive, owner)?;
    let indexed = rebuild_fts(&archive, owner)?;
    tables.push(report_table(
        &archive,
        "knowledge_fts",
        &["tenant_key", "kind", "object_id", "revision", "normalized"],
        "rebuilt-from-retained-owner-content-never-copied",
    )?);
    sql(archive.execute_batch("COMMIT;"))?;
    ensure(
        sql(archive.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)))? == "ok",
        "archive integrity failed",
    )?;
    let archive_schema_hash = schema_hash(&archive)?;
    sql(archive.close().map_err(|(_, e)| e))?;
    File::open(&dbpath)
        .and_then(|f| f.sync_all())
        .map_err(|_| "archive fsync failed")?;
    ensure(
        file_hash(&path)? == before,
        "snapshot changed during extraction; discard output and repeat with a stable copy",
    )?;
    let source_checkpoint = checkpoints(&source)?;
    let report = json!({"format":1,"scope":SCOPE,"cutover_ready":false,"complete_account_export":false,"runtime_store_compatible":false,"authenticated_lineage":false,"owner":owner.value(),"source":{"schema":source_schema,"sqlite_sha256":before,"inventory_schema_sha256":manifest["schema_sha256"]},"source_checkpoint":source_checkpoint,"archive":{"file":"knowledge.sqlite","schema_sha256":archive_schema_hash,"sqlite_sha256":file_hash(&dbpath)?},"tables":tables,"excluded_tables":excluded,"not_verified":["Credentials, sessions, installation role and runtime grants are excluded","Other private tables are outside this explicitly partial knowledge scope","Global journal authentication, owner genesis and independent receiver approval are pending","No runtime startup, import, writable cutover, backup or source-free runtime recovery is authorized"]});
    let mut f = new_file(&output.join("manifest.json"))?;
    let bytes = serde_json::to_vec_pretty(&report).map_err(|_| "manifest serialization failed")?;
    f.write_all(&bytes)
        .and_then(|_| f.write_all(b"\n"))
        .and_then(|_| f.sync_all())
        .map_err(|_| "manifest publication failed")?;
    drop(f);
    File::open(&output)
        .and_then(|f| f.sync_all())
        .map_err(|_| "archive directory fsync failed")?;
    File::open(parent)
        .and_then(|f| f.sync_all())
        .map_err(|_| "archive parent fsync failed")?;
    verify(root, &output)?;
    cleanup.complete = true;
    Ok(
        json!({"mode":"partial-owner-knowledge-extraction","scope":SCOPE,"cutover_ready":false,"archive_sha256":report["archive"]["sqlite_sha256"],"copied_tables":13,"fts_records":indexed,"source_schema":source_schema}),
    )
}
fn verify(root: &Path, output: &Path) -> Check<Value> {
    private_dir(output)?;
    let mut names = BTreeSet::new();
    for e in fs::read_dir(output).map_err(|_| "archive directory unavailable")? {
        let e = e.map_err(|_| "archive directory unavailable")?;
        let name = e
            .file_name()
            .into_string()
            .map_err(|_| "invalid archive entry")?;
        ensure(
            ["knowledge.sqlite", "manifest.json"].contains(&name.as_str()) && names.insert(name),
            "archive has unexpected files or incomplete publication",
        )?;
        private_file(
            &e.path(),
            if e.file_name() == "manifest.json" {
                2 * 1024 * 1024
            } else {
                512 * 1024 * 1024
            },
        )?;
    }
    ensure(names.len() == 2, "incomplete archive publication")?;
    let bytes =
        fs::read(output.join("manifest.json")).map_err(|_| "archive manifest unavailable")?;
    ensure(bytes.len() <= 2 * 1024 * 1024, "archive manifest too large")?;
    let report: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid archive manifest")?;
    ensure(
        report["format"] == 1
            && report["scope"] == SCOPE
            && report["cutover_ready"] == false
            && report["complete_account_export"] == false
            && report["runtime_store_compatible"] == false
            && report["authenticated_lineage"] == false,
        "archive scope or authority contract invalid",
    )?;
    let owner = Owner::from_value(&report["owner"])?;
    let source_schema = report["source"]["schema"]
        .as_i64()
        .ok_or("invalid source schema")?;
    let manifest = inventory::load(root, source_schema)?;
    let baseline = inventory::replay(root, &manifest)?;
    ensure(
        report["source"]["inventory_schema_sha256"] == manifest["schema_sha256"],
        "source schema fingerprint changed",
    )?;
    let expected = sql(Connection::open_in_memory())?;
    schema(&baseline, &expected)?;
    let path = output.join("knowledge.sqlite");
    ensure(
        file_hash(&path)? == field(&report["archive"], "sqlite_sha256")?,
        "archive file fingerprint mismatch",
    )?;
    let archive = inventory::open_snapshot(&path)?;
    ensure(
        count(&archive, "PRAGMA application_id")? == APP_ID
            && count(&archive, "PRAGMA user_version")? == 1,
        "not an offline extraction archive",
    )?;
    ensure(
        schema_hash(&archive)? == schema_hash(&expected)?
            && report["archive"]["schema_sha256"] == schema_hash(&expected)?,
        "archive schema mismatch",
    )?;
    ensure(
        sql(archive.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)))? == "ok",
        "archive integrity failed",
    )?;
    let bound:(String,String,String,i64,i64,String,String,String)=sql(archive.query_row("SELECT account_id,tenant_id,volume_id,generation,source_schema,source_sha256,inventory_schema_sha256,scope FROM extraction_owner WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))))?;
    ensure(
        bound
            == (
                owner.account.clone(),
                owner.tenant.clone(),
                owner.volume.clone(),
                owner.generation,
                source_schema,
                field(&report["source"], "sqlite_sha256")?.into(),
                field(&report["source"], "inventory_schema_sha256")?.into(),
                SCOPE.into(),
            )
            && count(&archive, "SELECT count(*) FROM extraction_owner")? == 1,
        "archive owner binding mismatch",
    )?;
    ensure(
        count(&archive, "SELECT count(*) FROM accounts")? == 1
            && count(&archive, "SELECT count(*) FROM tenants")? == 1,
        "archive contains multiple owners",
    )?;
    let matched: i64 = sql(archive.query_row(
        "SELECT count(*) FROM accounts WHERE id=?1 AND tenant_id=?2",
        params![owner.account, owner.tenant],
        |r| r.get(0),
    ))?;
    ensure(matched == 1, "archive account/tenant binding mismatch")?;
    let exclusions = report["excluded_tables"]
        .as_array()
        .ok_or("missing partial-scope exclusions")?;
    let expected_exclusions = manifest["tables"]
        .as_array()
        .ok_or("missing source classification")?
        .iter()
        .filter_map(|t| t["name"].as_str())
        .filter(|n| !TABLES.contains(n) && !["accounts", "tenants", "knowledge_fts"].contains(n))
        .collect::<BTreeSet<_>>();
    let mut actual_exclusions = BTreeSet::new();
    for t in exclusions {
        let name = field(t, "table")?;
        ensure(
            expected_exclusions.contains(name)
                && actual_exclusions.insert(name)
                && t["scope"] == manifest_table(&manifest, name)?["scope"]
                && t["disposition"] == "outside-requested-partial-scope"
                && t["source_rows"].as_i64().is_some_and(|n| n >= 0)
                && (t["owner_rows"].is_null()
                    || t["owner_rows"]
                        .as_i64()
                        .is_some_and(|n| n >= 0 && Some(n) <= t["source_rows"].as_i64())),
            "invalid outside-scope table accounting",
        )?;
    }
    ensure(
        actual_exclusions == expected_exclusions,
        "incomplete outside-scope table accounting",
    )?;
    let reports = report["tables"]
        .as_array()
        .ok_or("missing table reconciliation")?;
    ensure(reports.len() == 14, "incomplete table reconciliation")?;
    let mut seen = BTreeSet::new();
    let mut total = 0;
    for t in reports {
        let name = field(t, "table")?;
        let disposition = match name {
            "accounts" => "owner-metadata-projection-no-authentication-or-role",
            "tenants" => "copied-owner-identity",
            "knowledge_fts" => "rebuilt-from-retained-owner-content-never-copied",
            _ => "copied-owner-rows",
        };
        ensure(
            t["disposition"] == disposition,
            "table scope disposition mismatch",
        )?;
        ensure(seen.insert(name), "duplicate table reconciliation")?;
        let cols = if name == "accounts" {
            vec!["id", "tenant_id", "name", "email"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        } else if name == "tenants" {
            vec!["id".into()]
        } else if name == "knowledge_fts" {
            vec!["tenant_key", "kind", "object_id", "revision", "normalized"]
                .into_iter()
                .map(str::to_owned)
                .collect()
        } else {
            ensure(TABLES.contains(&name), "unsupported table reconciliation")?;
            columns(&manifest, name)?
        };
        ensure(
            t["columns"] == json!(cols),
            "table column reconciliation mismatch",
        )?;
        let (n, sha, bytes) = row_hash(&archive, name, &cols, &all_query(name, &cols), &[])?;
        total += bytes;
        ensure(
            t["rows"] == n && t["rows_sha256"] == sha && t["payload_bytes"] == bytes,
            "table row fingerprint mismatch",
        )?;
        if TABLES.contains(&name) {
            let wrong: i64 = sql(archive.query_row(
                &format!("SELECT count(*) FROM {} WHERE tenant_id<>?1", q(name)),
                [&owner.tenant],
                |r| r.get(0),
            ))?;
            ensure(wrong == 0, "archive contains another tenant")?;
        }
    }
    ensure(total <= 3 * MAX_BYTES, "archive payload bound exceeded")?;
    knowledge_graph(&archive, &owner)?;
    let expected_fts = fts_records(&archive, &owner)?;
    let mut fts=sql(archive.prepare("SELECT kind,object_id,cast(revision AS INTEGER),normalized FROM knowledge_fts ORDER BY kind,object_id,cast(revision AS INTEGER),normalized"))?;
    let actual = sql(fts.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
        ))
    }))?
    .map(sql)
    .collect::<Check<Vec<_>>>()?;
    ensure(
        actual == expected_fts,
        "archive FTS contains stale or foreign content",
    )?;
    let foreign: i64 = sql(archive.query_row(
        "SELECT count(*) FROM knowledge_fts WHERE tenant_key<>?1",
        [format!("t{}", owner.tenant.replace('-', ""))],
        |r| r.get(0),
    ))?;
    ensure(foreign == 0, "archive FTS contains another owner token")?;
    Ok(
        json!({"mode":"source-free-offline-archive-verification","scope":SCOPE,"cutover_ready":false,"verified_tables":14,"fts_records":expected_fts.len(),"archive_sha256":report["archive"]["sqlite_sha256"]}),
    )
}
pub fn run(root: &Path, args: &[String]) -> Check<()> {
    if args.len() == 2 && args[0] == "--verify" {
        let result = verify(root, &PathBuf::from(&args[1]))?;
        println!("{result}");
        return Ok(());
    }
    let mut options = BTreeMap::new();
    let mut args = args.iter();
    while let Some(key) = args.next() {
        ensure(
            [
                "--scope",
                "--snapshot",
                "--output",
                "--account",
                "--tenant",
                "--volume",
                "--generation",
            ]
            .contains(&key.as_str())
                && options
                    .insert(
                        key.as_str(),
                        args.next()
                            .ok_or("missing extraction option value")?
                            .as_str(),
                    )
                    .is_none(),
            "unknown or duplicate extraction option",
        )?;
    }
    ensure(
        options.len() == 7 && options.get("--scope") == Some(&SCOPE),
        "usage: account-runtime-extract --scope manual-knowledge-v1 --snapshot DIRECTORY --output DIRECTORY --account UUID --tenant UUID --volume UUID --generation N | --verify DIRECTORY",
    )?;
    let owner = Owner {
        account: uuid(options["--account"])?,
        tenant: uuid(options["--tenant"])?,
        volume: uuid(options["--volume"])?,
        generation: options["--generation"]
            .parse::<i64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or("positive generation required")?,
    };
    let result = extract(
        root,
        &PathBuf::from(options["--snapshot"]),
        &PathBuf::from(options["--output"]),
        &owner,
    )?;
    println!("{result}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    const A: &str = "11111111-1111-4111-8111-111111111111";
    const TA: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    const B: &str = "22222222-2222-4222-8222-222222222222";
    const TB: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    const SA: &str = "33333333-3333-4333-8333-333333333333";
    const SB: &str = "44444444-4444-4444-8444-444444444444";
    const MA: &str = "55555555-5555-4555-8555-555555555555";
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }
    fn owner() -> Owner {
        Owner {
            account: A.into(),
            tenant: TA.into(),
            volume: "99999999-9999-4999-8999-999999999999".into(),
            generation: 3,
        }
    }
    struct Fixture {
        _dir: tempfile::TempDir,
        source: PathBuf,
        output: PathBuf,
    }
    fn source_version(c: &Connection, tenant: &str, id: &str, rev: i64, title: &str, text: &str) {
        let external = if tenant == TA { "manual-a" } else { "manual-b" };
        let version = format!("v{rev}");
        let content = (
            external,
            version.as_str(),
            title,
            text,
            Option::<String>::None,
            1000i64,
            Option::<i64>::None,
            Option::<i64>::None,
        );
        let digest = Sha256::digest(serde_json::to_vec(&content).unwrap());
        c.execute("INSERT INTO knowledge_source_versions VALUES(?1,?2,?3,?4,?5,?6,NULL,1000,NULL,NULL,?7)",params![tenant,id,rev,version,title,text,digest.as_slice()]).unwrap();
    }
    fn fixture(schema_version: i64) -> Fixture {
        let d = tempfile::tempdir().unwrap();
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let base = d.path().canonicalize().unwrap();
        let source = base.join("source");
        fs::DirBuilder::new().mode(0o700).create(&source).unwrap();
        let output = base.join("archive");
        let path = source.join("assistant.sqlite");
        drop(new_file(&path).unwrap());
        let c =
            inventory::replay(&root(), &inventory::load(&root(), schema_version).unwrap()).unwrap();
        c.execute("INSERT INTO tenants VALUES(?1)", [TA]).unwrap();
        c.execute("INSERT INTO tenants VALUES(?1)", [TB]).unwrap();
        for (account, tenant, email, name) in [
            (A, TA, "a@example.invalid", "Synthetic A"),
            (B, TB, "b@example.invalid", "PRIVATE-B-NAME-CANARY"),
        ] {
            c.execute("INSERT INTO accounts(id,tenant_id,name,email,role,password_hash,must_change_password,auth_epoch) VALUES(?1,?2,?3,?4,'member','PRIVATE-PASSWORD-HASH-CANARY',0,0)",params![account,tenant,name,email]).unwrap();
        }
        if schema_version >= 85 {
            for (account, tenant, id) in [(A, TA, SA), (B, TB, SB)] {
                c.execute("INSERT INTO threads(tenant_id,id,account_id,title,created_at,updated_at) VALUES(?1,?2,?3,'Private mail read',1000,1000)",params![tenant,id,account]).unwrap();
                c.execute("INSERT INTO member_google_thread_reads(tenant_id,account_id,request_id,thread_id,volume_id,generation,auth_epoch,session_digest,app_revision,connection_id,source_epoch,phase,created_at,updated_at,cipher) VALUES(?1,?2,?3,?3,'99999999-9999-4999-8999-999999999999',3,0,zeroblob(32),'99999999-8888-4888-8888-999999999999','99999999-7777-4777-8777-999999999999',1,'completed',1000,1000,?4)",params![tenant,account,id,b"PRIVATE-GOOGLE-HEADERS-CANARY".as_slice()]).unwrap();
            }
        }
        for (tenant, id, source_id) in [
            (TA, "66666666-6666-4666-8666-666666666666", SA),
            (TB, "77777777-7777-4777-8777-777777777777", SB),
        ] {
            c.execute("INSERT INTO knowledge_state VALUES(?1,5)", [tenant])
                .unwrap();
            c.execute(
                "INSERT INTO knowledge_collections VALUES(?1,?2,'Manual','manual',3,2,1,0)",
                params![tenant, id],
            )
            .unwrap();
            c.execute(
                "INSERT INTO knowledge_sources VALUES(?1,?2,?3,?4,2,0)",
                params![
                    tenant,
                    source_id,
                    id,
                    if tenant == TA { "manual-a" } else { "manual-b" }
                ],
            )
            .unwrap();
        }
        source_version(&c, TA, SA, 1, "Earlier", "Earlier owner fact");
        source_version(&c, TA, SA, 2, "Current", "Owner A queue fact");
        source_version(&c, TB, SB, 1, "Foreign", "PRIVATE-B-TEXT-CANARY");
        source_version(&c, TB, SB, 2, "Foreign", "PRIVATE-B-CURRENT-CANARY");
        c.execute(
            "INSERT INTO knowledge_memories VALUES(?1,?2,2,'active',1000,2000)",
            params![TA, MA],
        )
        .unwrap();
        for (revision, text, from) in [
            (1, "Earlier remembered fact", Some(1000)),
            (2, "Owner A queue fact", Some(2000)),
        ] {
            let evidence = if revision == 2 {
                vec![
                    json!({"source_id":SA,"source_revision":2,"start_byte":0,"end_byte":18,"quote":"Owner A queue fact"}),
                ]
            } else {
                vec![]
            };
            let draft = json!({"subject":"Queue","text":text,"speaker":null,"valid_from":from,"valid_until":null,"evidence":evidence});
            c.execute(
                "INSERT INTO knowledge_memory_versions VALUES(?1,?2,?3,?4,'active',?5)",
                params![TA, MA, revision, draft.to_string(), from],
            )
            .unwrap();
            c.execute(
                "INSERT INTO knowledge_memory_timeline VALUES(?1,?2,?3,?4)",
                params![TA, MA, revision, from],
            )
            .unwrap();
        }
        let quote = Sha256::digest(b"Owner A queue fact");
        c.execute(
            "INSERT INTO knowledge_evidence VALUES(?1,?2,2,?3,2,0,18,?4)",
            params![TA, MA, SA, quote.as_slice()],
        )
        .unwrap();
        // A deleted source and forgotten memory retain only tombstones/suppression.
        c.execute("INSERT INTO knowledge_sources VALUES(?1,'88888888-8888-4888-8888-888888888888','66666666-6666-4666-8666-666666666666','deleted',3,1)",[TA]).unwrap();
        c.execute("INSERT INTO knowledge_memories VALUES(?1,'aaaaaaaa-1111-4111-8111-111111111111',3,'forgotten',1000,2000)",[TA]).unwrap();
        c.execute("INSERT INTO knowledge_suppressions VALUES(?1,'88888888-8888-4888-8888-888888888888',zeroblob(32))",[TA]).unwrap();
        // Source FTS is deliberately poisoned/stale; it must never be copied.
        c.execute("INSERT INTO knowledge_fts VALUES('foreign-token','source','foreign',1,'PRIVATE-B-INDEX-CANARY')",[]).unwrap();
        c.execute("INSERT INTO knowledge_fts VALUES(?1,'memory','aaaaaaaa-1111-4111-8111-111111111111',1,'DELETED-STALE-INDEX-CANARY')",[format!("t{}",TA.replace('-',""))]).unwrap();
        let mut target = Connection::open(&path).unwrap();
        {
            let backup = rusqlite::backup::Backup::new(&c, &mut target).unwrap();
            backup
                .run_to_completion(64, std::time::Duration::from_millis(1), None)
                .unwrap();
        }
        drop(target);
        drop(c);
        Fixture {
            _dir: d,
            source,
            output,
        }
    }
    fn edit(f: &Fixture, body: impl FnOnce(&Connection)) {
        let c = Connection::open(f.source.join("assistant.sqlite")).unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        body(&c);
    }
    #[test]
    fn owner_graph_is_copied_exactly_and_verifies_without_the_source() {
        for version in [81, 82, 83, 84, 85, 86, 87] {
            let f = fixture(version);
            let before = file_hash(&f.source.join("assistant.sqlite")).unwrap();
            let result = extract(&root(), &f.source, &f.output, &owner()).unwrap();
            assert_eq!(result["source_schema"], version);
            assert_eq!(result["fts_records"], 3);
            assert_eq!(
                file_hash(&f.source.join("assistant.sqlite")).unwrap(),
                before
            );
            assert_eq!(fs::read_dir(&f.source).unwrap().count(), 1);
            let c = inventory::open_snapshot(&f.output.join("knowledge.sqlite")).unwrap();
            assert_eq!(
                count(&c, "SELECT count(*) FROM knowledge_source_versions").unwrap(),
                2
            );
            assert_eq!(
                count(&c, "SELECT count(*) FROM knowledge_memory_versions").unwrap(),
                2
            );
            assert_eq!(
                count(
                    &c,
                    "SELECT count(*) FROM knowledge_memories WHERE status='forgotten'"
                )
                .unwrap(),
                1
            );
            assert_eq!(
                count(&c, "SELECT count(*) FROM knowledge_sources WHERE deleted=1").unwrap(),
                1
            );
            assert_eq!(
                count(
                    &c,
                    "SELECT count(*) FROM knowledge_fts WHERE knowledge_fts MATCH 'queue'"
                )
                .unwrap(),
                3
            );
            assert_eq!(
                count(&c, "SELECT count(*) FROM pragma_foreign_key_check").unwrap(),
                0
            );
            drop(c);
            let bytes = fs::read(f.output.join("knowledge.sqlite")).unwrap();
            for canary in [
                "PRIVATE-B-NAME-CANARY",
                "PRIVATE-B-TEXT-CANARY",
                "PRIVATE-B-CURRENT-CANARY",
                "PRIVATE-B-INDEX-CANARY",
                "DELETED-STALE-INDEX-CANARY",
                "PRIVATE-PASSWORD-HASH-CANARY",
                "PRIVATE-GOOGLE-HEADERS-CANARY",
            ] {
                assert!(!bytes.windows(canary.len()).any(|w| w == canary.as_bytes()));
            }
            if version >= 85 {
                let report: Value =
                    serde_json::from_slice(&fs::read(f.output.join("manifest.json")).unwrap())
                        .unwrap();
                assert!(
                    report["excluded_tables"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|t| t["table"] == "member_google_thread_reads"
                            && t["source_rows"] == 2
                            && t["owner_rows"] == 1)
                );
            }
            fs::remove_file(f.source.join("assistant.sqlite")).unwrap();
            fs::remove_dir(&f.source).unwrap();
            assert_eq!(verify(&root(), &f.output).unwrap()["verified_tables"], 14);
        }
    }
    #[test]
    fn owner_pair_unknown_schema_and_unclassified_tables_fail_before_output() {
        let f = fixture(82);
        let mut wrong = owner();
        wrong.tenant = TB.into();
        assert!(
            extract(&root(), &f.source, &f.output, &wrong)
                .unwrap_err()
                .contains("owner pair")
        );
        assert!(!f.output.exists());
        edit(&f, |c| {
            c.execute_batch("PRAGMA user_version=999;").unwrap();
        });
        assert!(
            extract(&root(), &f.source, &f.output, &owner())
                .unwrap_err()
                .contains("unsupported inventory schema")
        );
        assert!(!f.output.exists());
        edit(&f, |c| {
            c.execute_batch(
                "PRAGMA user_version=82;CREATE TABLE unclassified_owner_data(secret TEXT);",
            )
            .unwrap();
        });
        assert!(
            extract(&root(), &f.source, &f.output, &owner())
                .unwrap_err()
                .contains("unclassified application tables")
        );
    }
    #[test]
    fn connected_or_automatic_provenance_is_not_silently_dropped() {
        let f = fixture(82);
        edit(&f, |c| {
            c.execute("INSERT INTO automatic_memory_origins VALUES(?1,?2,?3,2,'synthetic','synthetic-model',1)",params![TA,MA,SA]).unwrap();
        });
        assert!(
            extract(&root(), &f.source, &f.output, &owner())
                .unwrap_err()
                .contains("outside manual extraction scope")
        );
        assert!(!f.output.exists());
    }
    #[test]
    fn embedded_foreign_evidence_fails_and_partial_output_is_removed() {
        let f = fixture(82);
        edit(&f, |c| {
            let mut draft:Value=serde_json::from_str(&c.query_row("SELECT draft FROM knowledge_memory_versions WHERE tenant_id=?1 AND memory_id=?2 AND revision=2",params![TA,MA],|r|r.get::<_,String>(0)).unwrap()).unwrap();
            draft["evidence"][0]["source_id"] = json!(SB);
            c.execute("UPDATE knowledge_memory_versions SET draft=?1 WHERE tenant_id=?2 AND memory_id=?3 AND revision=2",params![draft.to_string(),TA,MA]).unwrap();
        });
        assert!(extract(&root(), &f.source, &f.output, &owner()).is_err());
        assert!(!f.output.exists());
    }
    #[test]
    fn source_digest_evidence_quote_and_deleted_plaintext_fail_closed() {
        for mode in 0..3 {
            let f = fixture(82);
            edit(&f, |c| match mode {
                0 => {
                    c.execute("UPDATE knowledge_source_versions SET digest=zeroblob(32) WHERE tenant_id=?1",[TA]).unwrap();
                }
                1 => {
                    c.execute(
                        "UPDATE knowledge_evidence SET quote_hash=zeroblob(32) WHERE tenant_id=?1",
                        [TA],
                    )
                    .unwrap();
                }
                _ => {
                    c.execute(
                        "UPDATE knowledge_sources SET deleted=1 WHERE tenant_id=?1 AND id=?2",
                        params![TA, SA],
                    )
                    .unwrap();
                }
            });
            assert!(extract(&root(), &f.source, &f.output, &owner()).is_err());
            assert!(!f.output.exists());
        }
    }
    #[test]
    fn archive_corruption_owner_rebinding_and_missing_completion_marker_are_rejected() {
        let f = fixture(82);
        extract(&root(), &f.source, &f.output, &owner()).unwrap();
        let report = f.output.join("manifest.json");
        let original = fs::read(&report).unwrap();
        let mut value: Value = serde_json::from_slice(&original).unwrap();
        value["owner"]["account_id"] = json!(B);
        fs::write(&report, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            verify(&root(), &f.output)
                .unwrap_err()
                .contains("owner binding")
        );
        fs::write(&report, &original).unwrap();
        let mut relabeled: Value = serde_json::from_slice(&original).unwrap();
        relabeled["tables"][1]["disposition"] = json!("full-authentication-migration");
        fs::write(&report, serde_json::to_vec(&relabeled).unwrap()).unwrap();
        assert!(
            verify(&root(), &f.output)
                .unwrap_err()
                .contains("disposition")
        );
        fs::write(&report, &original).unwrap();
        let path = f.output.join("knowledge.sqlite");
        let c = Connection::open(&path).unwrap();
        c.execute("DELETE FROM knowledge_fts", []).unwrap();
        drop(c);
        assert!(
            verify(&root(), &f.output)
                .unwrap_err()
                .contains("file fingerprint")
        );
        fs::remove_file(&report).unwrap();
        assert!(
            verify(&root(), &f.output)
                .unwrap_err()
                .contains("incomplete")
        );
    }
    #[test]
    fn existing_output_source_overlap_sidecars_links_and_unknown_files_are_preserved_or_rejected() {
        let f = fixture(82);
        fs::DirBuilder::new().mode(0o700).create(&f.output).unwrap();
        fs::write(f.output.join("keep"), b"keep").unwrap();
        assert!(
            extract(&root(), &f.source, &f.output, &owner())
                .unwrap_err()
                .contains("nothing overwritten")
        );
        assert_eq!(fs::read(f.output.join("keep")).unwrap(), b"keep");
        assert!(
            extract(&root(), &f.source, &f.source.join("nested"), &owner())
                .unwrap_err()
                .contains("disjoint")
        );
        for name in ["assistant.sqlite-wal", "writer.lock", "unknown-data"] {
            let p = f.source.join(name);
            fs::write(&p, b"untouched").unwrap();
            assert!(
                extract(&root(), &f.source, &f.output, &owner())
                    .unwrap_err()
                    .contains("DB-only stopped snapshot")
            );
            assert_eq!(fs::read(&p).unwrap(), b"untouched");
            fs::remove_file(p).unwrap();
        }
        let db = f.source.join("assistant.sqlite");
        fs::set_permissions(&db, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            extract(&root(), &f.source, &f.output, &owner())
                .unwrap_err()
                .contains("private")
        );
        fs::set_permissions(&db, fs::Permissions::from_mode(0o600)).unwrap();
        let hard = f.source.parent().unwrap().join("hardlinked.sqlite");
        fs::hard_link(&db, &hard).unwrap();
        assert!(
            extract(&root(), &f.source, &f.output, &owner())
                .unwrap_err()
                .contains("single-link")
        );
        fs::remove_file(hard).unwrap();
        let linked = f.source.parent().unwrap().join("linked");
        std::os::unix::fs::symlink(&f.source, &linked).unwrap();
        assert!(
            extract(&root(), &linked, &f.output, &owner())
                .unwrap_err()
                .contains("canonical")
        );
    }
    #[test]
    fn purged_historical_revision_keeps_its_temporal_barrier() {
        let f = fixture(82);
        edit(&f, |c| {
            let draft = json!({"subject":"Queue","text":"Future independent assertion","speaker":null,"valid_from":3000,"valid_until":null,"evidence":[]});
            c.execute("UPDATE knowledge_memories SET revision=3,updated_at=3000 WHERE tenant_id=?1 AND id=?2",params![TA,MA]).unwrap();
            c.execute(
                "INSERT INTO knowledge_memory_versions VALUES(?1,?2,3,?3,'active',3000)",
                params![TA, MA, draft.to_string()],
            )
            .unwrap();
            c.execute(
                "INSERT INTO knowledge_memory_timeline VALUES(?1,?2,3,3000)",
                params![TA, MA],
            )
            .unwrap();
            c.execute("DELETE FROM knowledge_memory_versions WHERE tenant_id=?1 AND memory_id=?2 AND revision=2",params![TA,MA]).unwrap();
        });
        extract(&root(), &f.source, &f.output, &owner()).unwrap();
        let c = inventory::open_snapshot(&f.output.join("knowledge.sqlite")).unwrap();
        assert_eq!(
            count(&c, "SELECT count(*) FROM knowledge_memory_timeline").unwrap(),
            3
        );
        let eligibility = "SELECT count(*) FROM knowledge_memory_versions v WHERE v.memory_id=?1 AND v.revision=(SELECT max(t.revision) FROM knowledge_memory_timeline t WHERE t.memory_id=v.memory_id AND t.tenant_id=v.tenant_id AND (t.valid_from IS NULL OR t.valid_from<=?2)) AND v.status='active'";
        for (at, expected) in [(1500, 1), (2500, 0), (3500, 1)] {
            assert_eq!(
                c.query_row(eligibility, params![MA, at], |r| r.get::<_, i64>(0))
                    .unwrap(),
                expected
            );
        }
    }
    #[test]
    fn command_line_extract_and_source_free_verify_use_explicit_scope_and_owner() {
        let f = fixture(82);
        let args = [
            "--scope",
            SCOPE,
            "--snapshot",
            f.source.to_str().unwrap(),
            "--output",
            f.output.to_str().unwrap(),
            "--account",
            A,
            "--tenant",
            TA,
            "--volume",
            "99999999-9999-4999-8999-999999999999",
            "--generation",
            "3",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        run(&root(), &args).unwrap();
        fs::remove_file(f.source.join("assistant.sqlite")).unwrap();
        fs::remove_dir(&f.source).unwrap();
        run(
            &root(),
            &["--verify".into(), f.output.to_str().unwrap().into()],
        )
        .unwrap();
        assert!(run(&root(), &["--scope".into(), "all-data".into()]).is_err());
    }
    #[test]
    fn outside_scope_data_is_counted_and_never_claimed_as_migrated() {
        let f = fixture(82);
        edit(&f, |c| {
            c.execute("INSERT INTO media_objects(tenant_id,account_id,id,state,bytes,created_at,auth_epoch) VALUES(?1,?2,'dddddddd-dddd-4ddd-8ddd-dddddddddddd','ready',?3,1000,0)",params![TA,A,b"PRIVATE-A-EXCLUDED-FILE-CANARY".as_slice()]).unwrap();
            c.execute("INSERT INTO sessions(digest,tenant_id,account_id,auth_epoch,password_change_only,expires_at) VALUES(?1,?2,?3,0,0,4102444800)",params![[41u8;32].as_slice(),TA,A]).unwrap();
            // B's unsupported automatic work must not block A's manual slice.
            c.execute("INSERT INTO automatic_memory_jobs(tenant_id,id,collection_id,source_id,source_revision,grant_revision,state,model,created_at) VALUES(?1,'eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee','77777777-7777-4777-8777-777777777777',?2,2,1,'queued','synthetic',1000)",params![TB,SB]).unwrap();
        });
        extract(&root(), &f.source, &f.output, &owner()).unwrap();
        let manifest: Value =
            serde_json::from_slice(&fs::read(f.output.join("manifest.json")).unwrap()).unwrap();
        let excluded = manifest["excluded_tables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["table"] == "media_objects")
            .unwrap();
        assert_eq!(excluded["owner_rows"], 1);
        assert_eq!(excluded["source_rows"], 1);
        let jobs = manifest["excluded_tables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["table"] == "automatic_memory_jobs")
            .unwrap();
        assert_eq!(jobs["source_rows"], 1);
        assert_eq!(jobs["owner_rows"], 0);
        let sessions = manifest["excluded_tables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["table"] == "sessions")
            .unwrap();
        assert_eq!(sessions["owner_rows"], 1);
        let archive = inventory::open_snapshot(&f.output.join("knowledge.sqlite")).unwrap();
        assert_eq!(
            count(
                &archive,
                "SELECT count(*) FROM sqlite_schema WHERE name='sessions'"
            )
            .unwrap(),
            0
        );
        drop(archive);
        assert_eq!(manifest["complete_account_export"], false);
        let bytes = fs::read(f.output.join("knowledge.sqlite")).unwrap();
        assert!(
            !bytes
                .windows(b"PRIVATE-A-EXCLUDED-FILE-CANARY".len())
                .any(|w| w == b"PRIVATE-A-EXCLUDED-FILE-CANARY")
        );
    }
}
