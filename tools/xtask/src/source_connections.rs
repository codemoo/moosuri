use super::{Check, ensure, phase, table};
use std::collections::BTreeSet;
use std::path::Path;

const HEADERS: &[&str] = &[
    "id",
    "category",
    "distribution",
    "state",
    "features",
    "phase",
    "code_targets",
    "spec_path",
    "scenarios",
    "evidence",
];
const PRIVATE_COLLECTOR: &str = "src/musuri/collectors/kakao.py";
const PRIVATE_RETRIEVAL: &str = "agent/src/musuri_agent/kakao_threads.py";
const CONTRACT: &str = "docs/data-connections.md";

fn public_kakao_target(value: &str) -> bool {
    value
        .to_ascii_lowercase()
        .split(|c: char| c.is_whitespace() || matches!(c, '+' | ';' | ',' | '`' | '(' | ')'))
        .any(|part| {
            part.contains('/')
                && part.contains("kakao")
                && !(part.starts_with("docs/") && part.ends_with(".md"))
        })
}

fn references(value: &str, known: &BTreeSet<String>) -> bool {
    let mut seen = BTreeSet::new();
    value
        .split(',')
        .all(|id| known.contains(id) && seen.insert(id))
}

fn repository_path(root: &Path, value: &str, must_exist: bool) -> bool {
    if value.is_empty()
        || value.contains([':', '\\'])
        || value.chars().any(char::is_whitespace)
        || value.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return false;
    }
    let path = root.join(value);
    if must_exist && !path.is_file() {
        return false;
    }
    // Planned files can be absent, but an existing symlink must not escape the repo.
    for ancestor in path.ancestors() {
        if ancestor.symlink_metadata().is_ok() {
            return ancestor.canonicalize().is_ok_and(|p| p.starts_with(root));
        }
    }
    false
}

fn registry(
    root: &Path,
    rows: &[Vec<String>],
    features: &BTreeSet<String>,
    scenarios: &BTreeSet<String>,
) -> Check<()> {
    let mut ids = BTreeSet::new();
    for row in rows {
        ensure(row.len() == HEADERS.len(), "invalid connector row")?;
        let id = &row[0];
        ensure(
            !id.is_empty()
                && id
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
                && ids.insert(id.as_str()),
            "invalid/duplicate connector ID",
        )?;
        ensure(
            matches!(
                (row[1].as_str(), row[2].as_str(), row[3].as_str()),
                ("official", "bundled", "planned")
                    | ("custom", "protocol", "planned")
                    | ("private", "excluded", "excluded")
            ),
            format!(
                "{id}: invalid connector category/distribution/state; runtime completion needs a new evidence contract"
            ),
        )?;
        ensure(
            references(&row[4], features) && phase(&row[5]) && references(&row[8], scenarios),
            format!("{id}: invalid feature/phase/scenario references"),
        )?;
        ensure(
            row[7].starts_with("openspec/")
                && row[7].ends_with("/spec.md")
                && repository_path(root, &row[7], true),
            format!("{id}: missing or unsafe spec path"),
        )?;
        ensure(
            row[9] == "-",
            format!("{id}: planned/excluded connector cannot claim runtime evidence"),
        )?;
        if row[3] == "excluded" {
            ensure(
                row[6] == "-",
                format!("{id}: excluded connector has code targets"),
            )?;
        } else {
            let mut targets = BTreeSet::new();
            ensure(
                row[4].split(',').any(|f| f == "F70")
                    && row[6].split(',').all(|target| {
                        (target.starts_with("crates/")
                            || target.starts_with("web/")
                            || target.starts_with("migrations/"))
                            && repository_path(root, target, false)
                            && !public_kakao_target(target)
                            && targets.insert(target)
                    }),
                format!("{id}: invalid/duplicate code targets or missing F70 mapping"),
            )?;
        }
        let expected = match id.as_str() {
            "gmail" | "slack" => Some(("official", "bundled", "planned")),
            "custom_http" => Some(("custom", "protocol", "planned")),
            "kakao" => Some(("private", "excluded", "excluded")),
            _ => None,
        };
        if let Some(expected) = expected {
            ensure(
                (row[1].as_str(), row[2].as_str(), row[3].as_str()) == expected,
                format!("{id}: required connector scope changed"),
            )?;
        }
        if id == "kakao" {
            ensure(
                row[4] == "F38",
                "private Kakao must retain its F38 exclusion",
            )?;
        }
    }
    ensure(
        ["gmail", "slack", "custom_http", "kakao"]
            .iter()
            .all(|id| ids.contains(id)),
        "missing required connector scope record",
    )
}

fn exclusions(inventory: &[Vec<String>], entries: &[Vec<String>]) -> Check<()> {
    for row in inventory {
        ensure(
            !public_kakao_target(&row[4]),
            format!("{}: public Kakao inventory target", row[0]),
        )?;
    }
    for row in entries {
        ensure(
            !public_kakao_target(&row[6]),
            format!("{}: public Kakao entrypoint target", row[4]),
        )?;
    }
    for (source, disposition, target) in [
        (
            PRIVATE_COLLECTOR,
            "exclude",
            "private external producer; not shipped",
        ),
        (PRIVATE_RETRIEVAL, "contract", CONTRACT),
    ] {
        ensure(
            inventory.iter().filter(|row| row[0] == source).count() == 1
                && inventory
                    .iter()
                    .any(|row| row[0] == source && row[3] == disposition && row[4] == target),
            format!(
                "{source}: retain historical source but exclude its public implementation target"
            ),
        )?;
    }
    ensure(
        entries.iter().any(|row| row[4] == "kakao_thread")
            && entries
                .iter()
                .filter(|row| row[4] == "kakao_thread")
                .all(|row| row[6] == CONTRACT),
        "historical kakao_thread must target only the generic source contract",
    )
}

pub(super) fn verify(
    root: &Path,
    features: &BTreeSet<String>,
    scenarios: &BTreeSet<String>,
    inventory: &[Vec<String>],
    entries: &[Vec<String>],
) -> Check<usize> {
    let rows = table(root, "source-connectors.tsv", HEADERS)?;
    registry(root, &rows, features, scenarios)?;
    exclusions(inventory, entries)?;
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reject_false_scope_completion_and_broken_trace_links() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .unwrap();
        let rows = table(&root, "source-connectors.tsv", HEADERS).unwrap();
        let features = super::super::definitions(&root, "features.md", 'F', 70).unwrap();
        let scenarios = super::super::scenarios(&root, &features).unwrap();
        registry(&root, &rows, &features, &scenarios).unwrap();
        for (id, column, value, expected) in [
            (
                "kakao",
                6,
                "crates/moosuri-connectors/src/kakao.rs",
                "excluded connector has code targets",
            ),
            ("custom_http", 1, "official", "invalid connector category"),
            (
                "gmail",
                3,
                "implemented",
                "runtime completion needs a new evidence contract",
            ),
            ("slack", 9, "docs/mail.md", "cannot claim runtime evidence"),
            ("gmail", 4, "F23,F999", "invalid feature/phase/scenario"),
            ("gmail", 8, "DATA99", "invalid feature/phase/scenario"),
            (
                "gmail",
                6,
                "crates/../private/collector.rs",
                "invalid/duplicate code targets",
            ),
            (
                "gmail",
                7,
                "openspec/missing/spec.md",
                "missing or unsafe spec path",
            ),
        ] {
            let mut changed = rows.clone();
            changed.iter_mut().find(|row| row[0] == id).unwrap()[column] = value.into();
            let error = registry(&root, &changed, &features, &scenarios).unwrap_err();
            assert!(error.contains(expected), "{id}/{column}: {error}");
        }
        let mut changed = rows.clone();
        changed.push(rows[0].clone());
        assert!(
            registry(&root, &changed, &features, &scenarios)
                .unwrap_err()
                .contains("duplicate")
        );
        changed.retain(|row| row[0] != "kakao");
        changed.pop();
        assert!(
            registry(&root, &changed, &features, &scenarios)
                .unwrap_err()
                .contains("missing required")
        );
    }

    #[test]
    fn reject_reintroduced_legacy_collector_or_tool_targets() {
        let mut collector = vec![String::new(); 10];
        collector[0] = PRIVATE_COLLECTOR.into();
        collector[3] = "exclude".into();
        collector[4] = "private external producer; not shipped".into();
        let mut retrieval = collector.clone();
        retrieval[0] = PRIVATE_RETRIEVAL.into();
        retrieval[3] = "contract".into();
        retrieval[4] = CONTRACT.into();
        let mut shared = collector.clone();
        shared[0] = "agent/src/musuri_agent/source_adapters.py".into();
        shared[3] = "rewrite".into();
        shared[4] = "crates/moosuri-connectors/src/source/mod.rs".into();
        let inventory = vec![collector, retrieval, shared];
        let mut entry = vec![String::new(); 9];
        entry[4] = "kakao_thread".into();
        entry[6] = CONTRACT.into();
        let mut shared_entry = entry.clone();
        shared_entry[4] = "source_search".into();
        shared_entry[6] = "crates/moosuri-agent/src/tools/source.rs".into();
        let entries = vec![entry, shared_entry];
        exclusions(&inventory, &entries).unwrap();
        for index in 0..inventory.len() {
            let mut changed = inventory.clone();
            changed[index][3] = "rewrite".into();
            changed[index][4] = "crates/moosuri-connectors/src/kakao.rs".into();
            assert!(exclusions(&changed, &entries).is_err());
        }
        for index in 0..entries.len() {
            let mut changed = entries.clone();
            changed[index][6] = "crates/moosuri-agent/src/tools/kakao.rs".into();
            assert!(exclusions(&inventory, &changed).is_err());
        }
    }
}
