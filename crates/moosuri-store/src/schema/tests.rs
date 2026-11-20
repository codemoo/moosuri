use super::*;
use rusqlite::Connection;

// Read the independently versioned SQL directory, so forgetting to register a
// newly added migration (including the tail) fails this test.
pub(crate) fn sql_files() -> Vec<(i64, String)> {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations/assistant");
    let mut files = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .map(|path| {
            let version = path.file_name().unwrap().to_str().unwrap()[..4]
                .parse::<i64>()
                .unwrap();
            (version, std::fs::read_to_string(path).unwrap())
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|(version, _)| *version);
    files
}

pub(crate) fn predecessor(files: &[(i64, String)], captured: i64) -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch("PRAGMA trusted_schema=OFF;PRAGMA foreign_keys=OFF;")
        .unwrap();
    for (version, sql) in files.iter().filter(|(version, _)| *version <= captured) {
        conn.execute_batch(sql).unwrap();
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            *version
        );
    }
    conn
}

/// Rebuild synthetic historical fixtures from the registered SQL prefix. Copy
/// only columns that existed at that boundary, excluding all later objects.
/// Callers keep the real Store journal/keys and exercise normal reopen afterward.
pub(crate) fn replace_with_predecessor(source: &mut Connection, captured: i64) {
    after(captured, 0).unwrap();
    let files = MIGRATIONS
        .iter()
        .map(|migration| (migration.version, migration.sql.to_owned()))
        .collect::<Vec<_>>();
    let historical = predecessor(&files, captured);
    let tables = historical
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let triggers = historical
        .prepare("SELECT name,sql FROM sqlite_schema WHERE type='trigger' ORDER BY name")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    let quote = |name: &str| format!("\"{}\"", name.replace('"', "\"\""));
    // Copy a snapshot rather than replaying historical mutation side effects.
    for (name, _) in &triggers {
        historical
            .execute_batch(&format!("DROP TRIGGER {}", quote(name)))
            .unwrap();
    }
    for table in &tables {
        let columns = historical
            .prepare(&format!("PRAGMA table_info({})", quote(table)))
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let source_columns = source
            .prepare(&format!("PRAGMA table_info({})", quote(table)))
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(
            columns.iter().all(|column| source_columns.contains(column)),
            "historical {table} columns must exist in the synthetic source"
        );
        let names = columns
            .iter()
            .map(|name| quote(name))
            .collect::<Vec<_>>()
            .join(",");
        let rows = source
            .prepare(&format!("SELECT {names} FROM {}", quote(table)))
            .unwrap()
            .query_map([], |row| {
                (0..columns.len())
                    .map(|at| row.get::<_, rusqlite::types::Value>(at))
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        historical
            .execute_batch(&format!("DELETE FROM {}", quote(table)))
            .unwrap();
        let placeholders = vec!["?"; columns.len()].join(",");
        let mut insert = historical
            .prepare(&format!(
                "INSERT INTO {} ({names}) VALUES ({placeholders})",
                quote(table)
            ))
            .unwrap();
        for row in rows {
            insert.execute(rusqlite::params_from_iter(row)).unwrap();
        }
    }
    for (_, sql) in &triggers {
        historical.execute_batch(sql).unwrap();
    }
    assert!(
        !historical
            .prepare("PRAGMA foreign_key_check")
            .unwrap()
            .exists([])
            .unwrap()
    );
    rusqlite::backup::Backup::new(&historical, source)
        .unwrap()
        .run_to_completion(128, std::time::Duration::from_millis(1), None)
        .unwrap();
}

pub(crate) fn shape(conn: &Connection) -> Vec<(String, String, String, Option<String>)> {
    conn.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

#[test]
fn registry_covers_every_sql_file_in_order_without_byte_changes() {
    let files = sql_files();
    assert_eq!(files.len(), MIGRATIONS.len());
    for ((version, sql), migration) in files.iter().zip(MIGRATIONS) {
        assert_eq!(*version, migration.version);
        assert_eq!(sql, migration.sql);
    }
    assert_eq!(CURRENT_VERSION, 90);
    assert!(after(CURRENT_VERSION, 0).unwrap().is_empty());
}

#[test]
fn registry_rejects_missing_duplicate_out_of_order_and_unsupported_ranges() {
    for captured in [-1, CURRENT_VERSION + 1] {
        assert!(after(captured, 0).is_err());
    }
    assert!(after(18, 19).is_err());
    assert!(after(19, -1).is_err());
    assert!(after(CURRENT_VERSION, CURRENT_VERSION + 1).is_err());
    for mutation in [0, 1, 2, 3] {
        let mut broken = MIGRATIONS.to_vec();
        match mutation {
            0 => {
                broken.remove(71);
            }
            1 => {
                broken[71] = broken[70];
            }
            2 => {
                broken.swap(70, 71);
            }
            _ => {
                broken.pop();
            }
        }
        // Even a current/no-op caller must reject a malformed registry.
        assert!(suffix(&broken, CURRENT_VERSION, 19, CURRENT_VERSION).is_err());
    }
    assert!(migration(0).is_none());
    assert!(migration(i64::MIN).is_none());
    assert!(migration(CURRENT_VERSION + 1).is_none());
}

#[test]
fn registry_startup_sql_reaches_current_from_every_predecessor() {
    let files = sql_files();
    let expected = shape(&predecessor(&files, CURRENT_VERSION));
    for captured in 0..=CURRENT_VERSION {
        let conn = predecessor(&files, captured);
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        let pending = after(captured, 0).unwrap();
        crate::migrate_startup(&conn, pending, 1..=14).unwrap();
        // SQL-only fixture: actual durable journal/meta initialization remains
        // covered by Store open and destructive-intent boundary tests.
        if captured < 15 {
            conn.execute_batch(migration(15).unwrap().sql).unwrap();
        }
        crate::migrate_startup(&conn, pending, 16..=65).unwrap();
        crate::migrate_startup(&conn, pending, 66..=CURRENT_VERSION).unwrap();
        assert_eq!(shape(&conn), expected, "startup predecessor {captured}");
        assert_eq!(
            conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            CURRENT_VERSION
        );
        assert_eq!(
            conn.pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            conn.pragma_query_value(None, "trusted_schema", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert!(
            !conn
                .prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap()
        );
    }
}

#[test]
fn startup_never_bypasses_the_schema15_journal_handoff() {
    let conn = predecessor(&sql_files(), 14);
    assert!(crate::migrate_startup(&conn, after(14, 0).unwrap(), 15..=15).is_err());
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        14
    );
}
