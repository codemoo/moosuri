//! Read-only migration inventory. Never opens Store, loads credentials, exports
//! rows, starts runners, or treats inventory success as cutover authorization.
use crate::{Check, ensure, read};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn sql<T>(r: rusqlite::Result<T>) -> Check<T> {
    // SQLite diagnostics can include source values. Only bounded public labels
    // and counts are reported by this command.
    r.map_err(|_| "SQLite inspection failed (no source values emitted)".into())
}
fn field<'a>(v: &'a Value, key: &str) -> Check<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("inventory manifest missing {key}"))
}
fn array<'a>(v: &'a Value, key: &str) -> Check<&'a Vec<Value>> {
    v.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("inventory manifest missing {key}"))
}
fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}
fn scalar(c: &Connection, query: &str) -> Check<i64> {
    sql(c.query_row(query, [], |r| r.get(0)))
}
pub(super) fn load(root: &Path, schema: i64) -> Check<Value> {
    ensure(
        (81..=87).contains(&schema),
        "unsupported inventory schema; regenerate and review classification first",
    )?;
    let mut v: Value = serde_json::from_str(&read(
        &root.join(format!("audit/account-runtime-schema{schema}.json")),
    )?)
    .map_err(|_| "invalid inventory manifest JSON".to_owned())?;
    ensure(
        v["format"] == 1 && v["schema"] == schema,
        "inventory manifest version mismatch",
    )?;
    if v.get("base_schema").is_some() {
        ensure(
            schema > 81 && v["base_schema"] == schema - 1,
            "unsupported inventory supplement",
        )?;
        let base = load(root, schema - 1)?;
        for key in ["migrations", "tables"] {
            let mut merged = array(&base, key)?.clone();
            if key == "tables" && v.get("updated_tables").is_some() {
                let mut replaced = BTreeSet::new();
                for update in array(&v, "updated_tables")? {
                    let name = field(update, "name")?;
                    ensure(replaced.insert(name), "duplicate updated table name")?;
                    let previous = merged
                        .iter_mut()
                        .find(|row| row["name"] == name)
                        .ok_or("updated table missing from base schema")?;
                    *previous = update.clone();
                }
            }
            merged.extend(array(&v, key)?.iter().cloned());
            if key == "tables" {
                merged.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            }
            v[key] = json!(merged);
        }
    }
    ensure(
        array(&v, "tables")?.len()
            == match schema {
                81 => 202,
                82 => 203,
                83 => 204,
                84 => 207,
                85 => 208,
                86 | 87 => 209,
                _ => unreachable!(),
            },
        "inventory manifest table count mismatch",
    )?;
    Ok(v)
}
fn migration_files(root: &Path) -> Check<BTreeMap<i64, PathBuf>> {
    let mut paths = BTreeMap::new();
    for entry in fs::read_dir(root.join("migrations/assistant"))
        .map_err(|_| "cannot list source migrations")?
    {
        let path = entry.map_err(|_| "cannot list source migrations")?.path();
        if path.extension().is_some_and(|s| s == "sql") {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("invalid migration name")?;
            let version = name
                .get(..4)
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or("invalid migration version")?;
            ensure(
                paths.insert(version, path).is_none(),
                "duplicate migration version",
            )?;
        }
    }
    Ok(paths)
}
pub(super) fn replay(root: &Path, manifest: &Value) -> Check<Connection> {
    let schema = manifest["schema"]
        .as_i64()
        .ok_or("invalid manifest schema")?;
    let migrations = array(manifest, "migrations")?;
    ensure(
        migrations.len() as i64 == schema,
        "incomplete migration pin list",
    )?;
    let paths = migration_files(root)?;
    let c = sql(Connection::open_in_memory())?;
    sql(c.execute_batch("PRAGMA foreign_keys=ON;"))?;
    for (index, pin) in migrations.iter().enumerate() {
        let version = index as i64 + 1;
        let path = paths.get(&version).ok_or("missing source migration")?;
        ensure(
            path.file_name().and_then(|s| s.to_str()) == Some(field(pin, "name")?),
            "source migration name changed; review inventory",
        )?;
        let bytes = fs::read(path).map_err(|_| "cannot read source migration")?;
        ensure(
            digest(&bytes) == field(pin, "sha256")?,
            format!("source migration {version} changed; regenerate and review inventory"),
        )?;
        let body = std::str::from_utf8(&bytes).map_err(|_| "source migration is not UTF-8")?;
        // Match Store's schema70 FK boundary. Schema15 has a journal handoff in
        // Store; here only its SQL is replayed into a disposable empty database.
        if version == 70 {
            sql(c.execute_batch("PRAGMA foreign_keys=OFF;"))?;
        }
        sql(c.execute_batch(&format!("BEGIN IMMEDIATE;{body}\nCOMMIT;")))?;
        if version == 70 {
            sql(c.execute_batch("PRAGMA foreign_keys=ON;"))?;
        }
        ensure(
            scalar(&c, "PRAGMA user_version")? == version,
            "migration did not set expected schema version",
        )?;
    }
    Ok(c)
}
pub(super) fn schema_tables(c: &Connection) -> Check<(BTreeMap<String, String>, BTreeSet<String>)> {
    let mut tables = BTreeMap::new();
    let mut shadows = BTreeSet::new();
    let mut stmt = sql(c.prepare("PRAGMA main.table_list"))?;
    let rows = sql(stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    }))?;
    for row in rows {
        let (db, name, kind) = sql(row)?;
        if db != "main" || name.starts_with("sqlite_") {
            continue;
        }
        match kind.as_str() {
            "table" | "virtual" => {
                tables.insert(name, kind);
            }
            "shadow" => {
                shadows.insert(name);
            }
            _ => return Err("unclassified schema object kind".into()),
        }
    }
    Ok((tables, shadows))
}
pub(super) fn schema_digest(c: &Connection, shadows: &BTreeSet<String>) -> Check<String> {
    let mut raw = String::new();
    let mut stmt = sql(c.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT GLOB 'sqlite_*' ORDER BY name,type"))?;
    let rows = sql(stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    }))?;
    for row in rows {
        let (kind, name, table, body) = sql(row)?;
        if shadows.contains(&table) {
            continue;
        }
        for item in [&kind, &name, &table, &body] {
            raw.push_str(item);
            raw.push('\0');
        }
    }
    Ok(digest(raw.as_bytes()))
}
fn verify_schema(c: &Connection, manifest: &Value) -> Check<BTreeSet<String>> {
    ensure(
        scalar(c, "PRAGMA user_version")? == manifest["schema"].as_i64().unwrap(),
        "snapshot schema version unsupported; regenerate and review inventory",
    )?;
    let (mut actual, shadows) = schema_tables(c)?;
    let mut names = BTreeSet::new();
    for row in array(manifest, "tables")? {
        let name = field(row, "name")?;
        ensure(names.insert(name), "duplicate table classification")?;
        ensure(
            actual.remove(name).as_deref() == Some(field(row, "kind")?),
            format!("missing/changed classified table {name}"),
        )?;
        let body: String = sql(c.query_row(
            "SELECT sql FROM sqlite_schema WHERE name=?1 AND type='table'",
            [name],
            |r| r.get(0),
        ))?;
        ensure(
            digest(body.as_bytes()) == field(row, "sql_sha256")?,
            format!("table shape changed: {name}"),
        )?;
        for key in ["scope", "key_transition", "verification", "on_failure"] {
            ensure(
                !field(row, key)?.trim().is_empty(),
                "incomplete table classification",
            )?;
        }
        let owner_query = field(row, "owner_query")?;
        let scope = field(row, "scope")?;
        ensure(
            [
                "personal",
                "split-account-record",
                "split-lifecycle-record",
                "split-usage-record",
                "installation-owner-record",
                "partitioned-model",
                "installation",
                "installation-private",
                "installation-secret",
                "shared-readonly",
                "regenerate",
                "rekey",
                "new-recovery-anchor",
            ]
            .contains(&scope),
            "unsupported table classification",
        )?;
        ensure(
            owner_query.is_empty()
                != [
                    "personal",
                    "split-account-record",
                    "split-lifecycle-record",
                    "split-usage-record",
                    "installation-owner-record",
                    "partitioned-model",
                ]
                .contains(&scope),
            "ownership query/classification mismatch",
        )?;
        array(row, "columns")?;
    }
    ensure(
        actual.is_empty(),
        format!("unclassified application tables: {}", actual.len()),
    )?;
    ensure(
        schema_digest(c, &shadows)? == field(manifest, "schema_sha256")?,
        "schema index/trigger/view fingerprint changed; review inventory",
    )?;
    Ok(shadows)
}
fn check_owner(c: &Connection, table: &Value, rows: i64) -> Check<(i64, i64)> {
    let name = field(table, "name")?;
    let owner = field(table, "owner_query")?;
    if owner.is_empty() {
        return Ok((0, rows));
    }
    let nullable = field(table, "scope")? == "partitioned-model";
    // Every source row has one exact owner path; joins cannot silently drop an
    // orphan or expand a row into several owners. TenantId != AccountId.
    let (mapped, distinct): (i64, i64) = sql(c.query_row(
        &format!("SELECT count(*),count(DISTINCT rid) FROM ({owner})"),
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    ensure(
        mapped == rows && distinct == rows,
        format!(
            "{name}: missing/ambiguous owner paths ({} rows)",
            (rows - distinct).abs() + (mapped - distinct).abs()
        ),
    )?;
    let invalid = scalar(
        c,
        &format!(
            "SELECT count(*) FROM ({owner}) o LEFT JOIN accounts a ON a.id=o.account_id AND a.tenant_id=o.tenant_id WHERE a.id IS NULL {}",
            if nullable {
                "AND (o.account_id IS NOT NULL OR o.tenant_id IS NOT NULL)"
            } else {
                ""
            }
        ),
    )?;
    ensure(
        invalid == 0,
        format!("{name}: invalid account/tenant owner pairs ({invalid} rows)"),
    )?;
    let private = scalar(
        c,
        &format!("SELECT count(*) FROM ({owner}) WHERE account_id IS NOT NULL"),
    )?;
    Ok((private, rows - private))
}
#[derive(Default)]
struct ForeignKey {
    parent: String,
    columns: BTreeMap<i64, (String, String)>,
}
fn foreign_keys(c: &Connection, name: &str) -> Check<BTreeMap<i64, ForeignKey>> {
    let mut result: BTreeMap<i64, ForeignKey> = BTreeMap::new();
    let mut stmt = sql(c.prepare(&format!("PRAGMA foreign_key_list({})", quoted(name))))?;
    let rows = sql(stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    }))?;
    for row in rows {
        let (id, seq, parent, child_col, parent_col) = sql(row)?;
        let fk = result.entry(id).or_default();
        fk.parent = parent;
        fk.columns.insert(seq, (child_col, parent_col));
    }
    Ok(result)
}
fn check_relationships(c: &Connection, tables: &[Value]) -> Check<usize> {
    let indexed: BTreeMap<_, _> = tables
        .iter()
        .map(|t| Ok((field(t, "name")?, t)))
        .collect::<Check<_>>()?;
    let mut checked = 0;
    for table in tables {
        let name = field(table, "name")?;
        let child_owner = field(table, "owner_query")?;
        for fk in foreign_keys(c, name)?.values() {
            let parent = indexed
                .get(fk.parent.as_str())
                .ok_or("foreign key parent is unclassified")?;
            let parent_owner = field(parent, "owner_query")?;
            if child_owner.is_empty() || parent_owner.is_empty() {
                continue;
            }
            let join = fk
                .columns
                .values()
                .map(|(a, b)| format!("t.{}=p.{}", quoted(a), quoted(b)))
                .collect::<Vec<_>>()
                .join(" AND ");
            let invalid = scalar(
                c,
                &format!(
                    "WITH co AS ({child_owner}), po AS ({parent_owner}) SELECT count(*) FROM {} t JOIN {} p ON {join} JOIN co ON co.rid=t.rowid JOIN po ON po.rid=p.rowid WHERE po.account_id IS NOT NULL AND (co.account_id IS NULL OR co.account_id<>po.account_id OR co.tenant_id<>po.tenant_id)",
                    quoted(name),
                    quoted(&fk.parent)
                ),
            )?;
            ensure(
                invalid == 0,
                format!(
                    "{name}: cross-owner foreign key to {} ({invalid} rows)",
                    fk.parent
                ),
            )?;
            checked += 1;
        }
    }
    // A shared selection must never promote a personal compatible connection.
    let shared_private = scalar(
        c,
        "SELECT count(*) FROM installation_model_selection s JOIN personal_compatible_providers p ON p.provider_id=s.provider_id",
    )?;
    ensure(
        shared_private == 0,
        "installation model selection references a private provider",
    )?;
    Ok(checked)
}
pub(super) fn inspect(c: &Connection, manifest: &Value) -> Check<Value> {
    let shadows = verify_schema(c, manifest)?;
    ensure(
        sql(c.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)))? == "ok",
        "snapshot SQLite integrity failed",
    )?;
    let violations = scalar(c, "SELECT count(*) FROM pragma_foreign_key_check")?;
    ensure(
        violations == 0,
        format!("snapshot has {violations} foreign-key violations"),
    )?;
    let mut report = Vec::new();
    let mut total = 0;
    let mut without_owner_columns = Vec::new();
    let mut scopes: BTreeMap<String, (usize, i64)> = BTreeMap::new();
    for table in array(manifest, "tables")? {
        let name = field(table, "name")?;
        let rows = scalar(c, &format!("SELECT count(*) FROM {}", quoted(name)))?;
        let (personal, other) = check_owner(c, table, rows)?;
        total += rows;
        let scope = scopes.entry(field(table, "scope")?.to_owned()).or_default();
        scope.0 += 1;
        scope.1 += rows;
        if !array(table, "columns")?
            .iter()
            .any(|v| v == "tenant_id" || v == "account_id")
        {
            without_owner_columns.push(name);
        }
        let mut item = table.clone();
        item["rows"] = json!(rows);
        item["personal_rows"] = json!(personal);
        item["installation_or_regenerated_rows"] = json!(other);
        report.push(item);
    }
    let joins = check_relationships(c, array(manifest, "tables")?)?;
    Ok(
        json!({"schema":manifest["schema"],"schema_sha256":manifest["schema_sha256"],"application_tables":report.len(),"excluded_fts_shadow_tables":shadows,"rows":total,"scope_table_and_row_counts":scopes,"tables_without_owner_columns":without_owner_columns,"cross_owner_fk_relationships_checked":joins,"tables":report}),
    )
}
fn artifact_manifest(root: &Path) -> Check<Value> {
    let v: Value = serde_json::from_str(&read(&root.join("audit/account-runtime-artifacts.json"))?)
        .map_err(|_| "invalid artifact manifest JSON")?;
    ensure(v["format"] == 1, "artifact manifest format unsupported")?;
    let mut names = BTreeSet::new();
    for entry in array(&v, "store_entries")? {
        let name = field(entry, "name")?;
        ensure(
            !name.contains('/') && names.insert(name),
            "invalid/duplicate store artifact",
        )?;
        for key in ["scope", "action", "verification"] {
            field(entry, key)?;
        }
    }
    ensure(
        names.len() == 11 && names.contains("assistant.sqlite"),
        "incomplete store artifact registry",
    )?;
    for entry in array(&v, "external_boundaries")? {
        for key in ["name", "scope", "action", "verification"] {
            field(entry, key)?;
        }
    }
    Ok(v)
}
fn check_ciphertext_registry(c: &Connection, manifest: &Value, artifacts: &Value) -> Check<Value> {
    let tables = array(manifest, "tables")?;
    let mut output = Vec::new();
    for binding in array(artifacts, "ciphertext_bindings")? {
        let name = field(binding, "table")?;
        if let Some(since) = binding.get("since_schema") {
            let since = since.as_i64().ok_or("invalid ciphertext schema")?;
            let schema = manifest["schema"]
                .as_i64()
                .ok_or("invalid inventory schema")?;
            if schema < since {
                ensure(
                    tables.iter().all(|t| t["name"] != name),
                    "ciphertext table predates declared schema",
                )?;
                continue;
            }
        }
        let table = tables
            .iter()
            .find(|t| t["name"] == name)
            .ok_or("unclassified ciphertext table")?;
        for key in ["aad_source", "transition", "on_unprovable"] {
            field(binding, key)?;
        }
        let mut counts = BTreeMap::new();
        for column in array(binding, "columns")? {
            let column = column.as_str().ok_or("invalid ciphertext field")?;
            ensure(
                array(table, "columns")?.iter().any(|v| v == column),
                "unclassified ciphertext field",
            )?;
            counts.insert(
                column,
                scalar(
                    c,
                    &format!(
                        "SELECT count(*) FROM {} WHERE {} IS NOT NULL",
                        quoted(name),
                        quoted(column)
                    ),
                )?,
            );
        }
        output.push(json!({"table":name,"nonnull_ciphertext_field_counts":counts,"cryptographic_proof":"not-performed"}));
    }
    Ok(json!(output))
}
fn snapshot_files(root: &Path, artifacts: &Value) -> Check<Value> {
    let meta = fs::symlink_metadata(root).map_err(|_| "snapshot directory unavailable")?;
    ensure(
        meta.is_dir() && !meta.file_type().is_symlink(),
        "snapshot must be a regular directory, not a symlink",
    )?;
    let known: BTreeSet<_> = array(artifacts, "store_entries")?
        .iter()
        .map(|r| field(r, "name"))
        .collect::<Check<_>>()?;
    let mut present = BTreeMap::new();
    for entry in fs::read_dir(root).map_err(|_| "cannot list snapshot")? {
        let entry = entry.map_err(|_| "cannot list snapshot")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "invalid snapshot filename")?;
        ensure(
            known.contains(name.as_str()),
            "snapshot contains unclassified files; stop and review without exposing filenames",
        )?;
        ensure(
            ![
                "assistant.sqlite-wal",
                "assistant.sqlite-shm",
                "cutover-checkpoint",
            ]
            .contains(&name.as_str()),
            "snapshot has WAL/SHM or incomplete cutover; supply a consistent stopped/checkpointed copy",
        )?;
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| "snapshot entry unavailable")?;
        ensure(
            meta.is_file() && !meta.file_type().is_symlink(),
            "snapshot entries must be regular non-symlink files",
        )?;
        present.insert(name, meta.len());
    }
    ensure(
        present.contains_key("assistant.sqlite"),
        "snapshot missing assistant.sqlite",
    )?;
    Ok(json!(present))
}
pub(super) fn open_snapshot(path: &Path) -> Check<Connection> {
    // immutable URI prevents SQLite from ever creating SHM/WAL or writing the
    // source. Only an explicitly supplied stable offline snapshot is accepted.
    let path = path
        .canonicalize()
        .map_err(|_| "snapshot database unavailable")?;
    let bytes = path
        .to_str()
        .ok_or("snapshot path must be UTF-8")?
        .as_bytes();
    let encoded: String = bytes.iter().map(|b| format!("%{b:02X}")).collect();
    let c = sql(Connection::open_with_flags(
        format!("file:{encoded}?mode=ro&immutable=1"),
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ))?;
    sql(c.execute_batch("PRAGMA query_only=ON; BEGIN;"))?;
    Ok(c)
}
pub fn run(root: &Path, args: &[String]) -> Check<()> {
    let mut selected = None;
    let mut snapshot = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--schema" if selected.is_none() => {
                selected = Some(
                    iter.next()
                        .ok_or("--schema requires 81 through 87")?
                        .parse::<i64>()
                        .map_err(|_| "--schema requires 81 through 87")?,
                );
            }
            "--snapshot" if snapshot.is_none() => {
                snapshot = Some(PathBuf::from(
                    iter.next()
                        .ok_or("--snapshot requires a stopped snapshot directory")?,
                ));
            }
            _ => {
                return Err(
                    "usage: account-runtime-inventory [--schema 81|82|83|84|85|86|87] [--snapshot DIRECTORY]"
                        .into(),
                );
            }
        }
    }
    let schema = match selected {
        Some(n) => n,
        None => *migration_files(root)?
            .keys()
            .next_back()
            .ok_or("no source migrations")?,
    };
    let manifest = load(root, schema)?;
    let baseline = replay(root, &manifest)?;
    // Always check that the frozen plan describes the actual public SQL, even
    // when inspecting a separate source snapshot.
    let baseline_report = inspect(&baseline, &manifest)?;
    let artifacts = artifact_manifest(root)?;
    let (inventory, file_presence, ciphertext_counts) = if let Some(path) = snapshot.as_ref() {
        let files = snapshot_files(path, &artifacts)?;
        let connection = open_snapshot(&path.join("assistant.sqlite"))?;
        (
            inspect(&connection, &manifest)?,
            files,
            check_ciphertext_registry(&connection, &manifest, &artifacts)?,
        )
    } else {
        (
            baseline_report,
            Value::Null,
            check_ciphertext_registry(&baseline, &manifest, &artifacts)?,
        )
    };
    println!("{}", serde_json::to_string_pretty(&json!({
        "format":1,"mode":if snapshot.is_some(){"explicit-offline-snapshot"}else{"source-schema-replay"},
        "cutover_ready":false,"inventory":inventory,"artifacts":artifacts,"snapshot_file_bytes":file_presence,"ciphertext_counts":ciphertext_counts,
        "not_verified":["Row/payload extraction and target reconciliation", "Undeclared semantic references and embedded JSON ownership", "External file discovery and referenced payload hash reconciliation", "Credential decryption/re-encryption, AAD/binding/runtime proof and selective reauthentication", "Journal authentication, receiver-approved owner genesis and source-free recovery", "Drain/watermark, external receipt reconciliation, generation fencing and pilot cutover"]
    })).map_err(|_| "cannot serialize inventory")?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir()
                .join(format!("moosuri-inventory-test-{}-{n}", std::process::id()));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }
    fn fixture() -> (Connection, Value) {
        let m = load(&root(), 81).unwrap();
        (replay(&root(), &m).unwrap(), m)
    }
    fn owners(c: &Connection) {
        c.execute_batch("INSERT INTO tenants VALUES('tenant-a'),('tenant-b');
          INSERT INTO accounts(id,tenant_id,name,email,role,password_hash,must_change_password,auth_epoch) VALUES
          ('account-a','tenant-a','Synthetic A','a@example.invalid','admin','synthetic-hash',0,0),
          ('account-b','tenant-b','Synthetic B','b@example.invalid','member','synthetic-hash',0,0);").unwrap();
    }
    fn table<'a>(m: &'a Value, name: &str) -> &'a Value {
        m["tables"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap()
    }
    fn devices(c: &Connection) {
        c.execute_batch("INSERT INTO credential_broker_devices VALUES('device-a','account-a','tenant-a',zeroblob(32),'synthetic-enrollment',zeroblob(32),'active');
          INSERT INTO credential_broker_bindings VALUES('device-a','binding-a','synthetic-public-binding',1);
          INSERT INTO credential_broker_auth VALUES('nonce-a','device-a','synthetic-challenge',999);
          INSERT INTO credential_broker_repositories VALUES('device-a','binding-a','example.invalid/repo');
          INSERT INTO credential_model_mail_grants VALUES('device-a','binding-a',0);").unwrap();
    }
    fn providers(c: &Connection) {
        c.execute_batch("BEGIN;
          INSERT INTO providers VALUES('shared',1),('private-b',1);
          INSERT INTO provider_revisions(provider_id,revision,name,config,enabled) VALUES('shared',1,'shared','{}',1),('private-b',1,'private','{}',1);
          INSERT INTO provider_model_probes(provider_id,revision,model,effort,verified_at) VALUES('shared',1,'model','none',0),('private-b',1,'model','none',0);
          INSERT INTO personal_compatible_providers VALUES('private-b','tenant-b','account-b');
          COMMIT;").unwrap();
    }
    #[test]
    fn actual_schema81_enumerates_all_202_tables_and_excludes_only_fts_shadows() {
        let (c, m) = fixture();
        let report = inspect(&c, &m).unwrap();
        assert_eq!(report["application_tables"], 202);
        assert_eq!(
            report["excluded_fts_shadow_tables"]
                .as_array()
                .unwrap()
                .len(),
            5
        );
        assert_eq!(report["cross_owner_fk_relationships_checked"], 187);
        assert_eq!(table(&m, "knowledge_fts")["scope"], "regenerate");
        assert_eq!(
            table(&m, "invitation_deliveries")["scope"],
            "installation-private"
        );
        let absent = m["tables"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| {
                let cols = t["columns"].as_array().unwrap();
                !cols.iter().any(|x| x == "account_id" || x == "tenant_id")
            })
            .count();
        assert_eq!(absent, 34);
    }
    #[test]
    fn schema82_is_pinned_and_validates_when_source_migration_is_present() {
        let m = load(&root(), 82).unwrap();
        assert_eq!(
            table(&m, "gmail_server_resume_bindings")["scope"],
            "personal"
        );
        if root()
            .join("migrations/assistant/0082_gmail_server_resume.sql")
            .exists()
        {
            let c = replay(&root(), &m).unwrap();
            let report = inspect(&c, &m).unwrap();
            assert_eq!(report["application_tables"], 203);
            assert!(verify_schema(&c, &load(&root(), 81).unwrap()).is_err());
        } else {
            eprintln!(
                "schema82 replay awaits separately owned pinned migration; manifest is checked"
            );
        }
    }
    #[test]
    fn schema83_preview_receipts_follow_the_original_run_owner() {
        let manifest = load(&root(), 83).unwrap();
        let c = replay(&root(), &manifest).unwrap();
        let counts = inspect(&c, &manifest).unwrap();
        assert_eq!(counts["application_tables"], 204);
        assert_eq!(
            table(&manifest, "personal_text_previews")["scope"],
            "personal"
        );
    }
    #[test]
    fn owner_ids_stay_distinct_and_parent_only_broker_rows_resolve() {
        let (c, m) = fixture();
        owners(&c);
        devices(&c);
        let report = inspect(&c, &m).unwrap();
        for name in ["accounts", "tenants"] {
            assert_eq!(table(&report, name)["personal_rows"], 2);
        }
        for name in [
            "credential_broker_auth",
            "credential_broker_bindings",
            "credential_broker_repositories",
            "credential_model_mail_grants",
        ] {
            assert_eq!(table(&report, name)["personal_rows"], 1);
        }
    }
    #[test]
    fn missing_parent_cannot_silently_drop_child_during_owner_join() {
        let (c, m) = fixture();
        owners(&c);
        c.execute_batch("PRAGMA foreign_keys=OFF; INSERT INTO credential_broker_auth VALUES('nonce','missing-device','never-emit-this-private-value',1);").unwrap();
        let e = check_owner(&c, table(&m, "credential_broker_auth"), 1).unwrap_err();
        assert!(e.contains("missing/ambiguous owner paths"));
        assert!(!e.contains("never-emit"));
        assert!(
            inspect(&c, &m)
                .unwrap_err()
                .contains("foreign-key violations")
        );
    }
    #[test]
    fn unknown_and_mismatched_owners_fail_even_without_schema_foreign_keys() {
        let (c, m) = fixture();
        owners(&c);
        c.execute_batch("INSERT INTO credential_broker_devices VALUES('device','account-a','tenant-b',zeroblob(32),'private-marker',zeroblob(32),'active');").unwrap();
        assert!(
            inspect(&c, &m)
                .unwrap_err()
                .contains("invalid account/tenant owner pairs")
        );
        c.execute_batch("DELETE FROM credential_broker_devices; INSERT INTO automatic_memory_budget VALUES('missing-tenant','missing-account',1,0,0,0);").unwrap();
        assert!(
            inspect(&c, &m)
                .unwrap_err()
                .contains("invalid account/tenant owner pairs")
        );
    }
    #[test]
    fn provider_rows_partition_by_immutable_owner_mapping_and_reject_cross_account_selection() {
        let (c, m) = fixture();
        owners(&c);
        providers(&c);
        let report = inspect(&c, &m).unwrap();
        assert_eq!(table(&report, "providers")["personal_rows"], 1);
        assert_eq!(
            table(&report, "providers")["installation_or_regenerated_rows"],
            1
        );
        // This probe table has no declared FK. An unknown provider cannot be
        // misclassified as installation-owned merely because no owner matches.
        c.execute_batch(
            "INSERT INTO provider_vision_probes VALUES('missing-provider',1,'model','none');",
        )
        .unwrap();
        assert!(
            inspect(&c, &m)
                .unwrap_err()
                .contains("provider_vision_probes: missing/ambiguous owner paths")
        );
        c.execute_batch("DELETE FROM provider_vision_probes;")
            .unwrap();
        // Both ordinary FKs are valid; only ownership comparison rejects this.
        c.execute_batch("INSERT INTO personal_model_selections VALUES('tenant-a','account-a','private-b',1,'model','none');").unwrap();
        assert_eq!(
            scalar(&c, "SELECT count(*) FROM pragma_foreign_key_check").unwrap(),
            0
        );
        assert!(
            inspect(&c, &m)
                .unwrap_err()
                .contains("cross-owner foreign key")
        );
    }
    #[test]
    fn shared_model_selection_cannot_promote_private_credentials() {
        let (c, m) = fixture();
        owners(&c);
        providers(&c);
        c.execute_batch(
            "INSERT INTO installation_model_selection VALUES(1,'private-b',1,'model','none');",
        )
        .unwrap();
        assert!(
            inspect(&c, &m)
                .unwrap_err()
                .contains("installation model selection references a private provider")
        );
    }
    #[test]
    fn unknown_table_version_shape_or_index_is_rejected() {
        let (c, m) = fixture();
        c.execute_batch("CREATE TABLE new_private_table(secret TEXT);")
            .unwrap();
        assert!(
            verify_schema(&c, &m)
                .unwrap_err()
                .contains("unclassified application tables")
        );
        c.execute_batch("DROP TABLE new_private_table; PRAGMA user_version=83;")
            .unwrap();
        assert!(
            verify_schema(&c, &m)
                .unwrap_err()
                .contains("schema version unsupported")
        );
        c.execute_batch("PRAGMA user_version=81; DROP INDEX sessions_account;")
            .unwrap();
        assert!(
            verify_schema(&c, &m)
                .unwrap_err()
                .contains("index/trigger/view fingerprint")
        );
        c.execute_batch("ALTER TABLE credential_broker_devices ADD COLUMN surprise TEXT;")
            .unwrap();
        assert!(
            verify_schema(&c, &m)
                .unwrap_err()
                .contains("table shape changed")
        );
    }
    #[test]
    fn incomplete_or_duplicated_classification_is_rejected() {
        let (c, mut m) = fixture();
        m["tables"].as_array_mut().unwrap().pop();
        assert!(verify_schema(&c, &m).is_err());
        let duplicate = m["tables"][0].clone();
        m["tables"].as_array_mut().unwrap().push(duplicate);
        assert!(verify_schema(&c, &m).is_err());
    }
    #[test]
    fn snapshot_never_writes_and_refuses_sidecars_unknown_files_or_symlinks() {
        let (c, m) = fixture();
        owners(&c);
        let dir = Scratch::new();
        let path = dir.0.join("assistant.sqlite");
        c.execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        let before = fs::read(&path).unwrap();
        let artifacts = artifact_manifest(&root()).unwrap();
        snapshot_files(&dir.0, &artifacts).unwrap();
        let source = open_snapshot(&path).unwrap();
        assert_eq!(inspect(&source, &m).unwrap()["application_tables"], 202);
        assert!(
            source
                .execute("INSERT INTO tenants VALUES('never-write')", [])
                .is_err()
        );
        drop(source);
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 1);
        for name in [
            "assistant.sqlite-wal",
            "assistant.sqlite-shm",
            "cutover-checkpoint",
            "unclassified-secret-file",
        ] {
            fs::write(dir.0.join(name), b"never-open-contents").unwrap();
            let error = snapshot_files(&dir.0, &artifacts).unwrap_err();
            assert!(!error.contains("unclassified-secret-file"));
            fs::remove_file(dir.0.join(name)).unwrap();
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&path, dir.0.join("provider-secrets.key")).unwrap();
            assert!(
                snapshot_files(&dir.0, &artifacts)
                    .unwrap_err()
                    .contains("non-symlink")
            );
        }
    }
    #[test]
    fn changed_source_migration_is_rejected_before_replay() {
        let dir = Scratch::new();
        fs::create_dir_all(dir.0.join("migrations/assistant")).unwrap();
        let mut content =
            fs::read(root().join("migrations/assistant/0001_accounts_threads.sql")).unwrap();
        content.extend_from_slice(b"\n-- synthetic drift marker\n");
        fs::write(
            dir.0.join("migrations/assistant/0001_accounts_threads.sql"),
            content,
        )
        .unwrap();
        let error = replay(&dir.0, &load(&root(), 81).unwrap()).unwrap_err();
        assert!(error.contains("migration 1 changed"));
        assert!(!error.contains("drift marker"));
    }
    #[test]
    fn empty_snapshot_and_unsupported_source_schema_fail_closed() {
        let dir = Scratch::new();
        assert!(snapshot_files(&dir.0, &artifact_manifest(&root()).unwrap()).is_err());
        assert!(
            load(&root(), 88)
                .unwrap_err()
                .contains("unsupported inventory schema")
        );
        assert!(
            run(&root(), &["--bogus".into()])
                .unwrap_err()
                .contains("usage:")
        );
    }
}
