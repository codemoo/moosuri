use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

mod source_connections;

type Check<T> = Result<T, String>;

fn read(path: &Path) -> Check<String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn table(root: &Path, name: &str, expected: &[&str]) -> Check<Vec<Vec<String>>> {
    let content = read(&root.join("audit").join(name))?;
    let mut lines = content.lines();
    let header: Vec<_> = lines.next().unwrap_or("").split('\t').collect();
    if header != expected {
        return Err(format!("{name}: unexpected header"));
    }
    lines
        .enumerate()
        .map(|(i, line)| {
            let cells: Vec<String> = line.split('\t').map(String::from).collect();
            if cells.len() != expected.len() || cells.iter().any(|s| s.trim().is_empty()) {
                return Err(format!("{name}: incomplete row {}", i + 2));
            }
            Ok(cells)
        })
        .collect()
}

fn ensure(ok: bool, message: impl Into<String>) -> Check<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}

fn phase(s: &str) -> bool {
    s.strip_prefix('P')
        .and_then(|s| s.parse::<u8>().ok())
        .is_some_and(|n| n <= 10)
}

fn definitions(root: &Path, file: &str, prefix: char, count: usize) -> Check<BTreeSet<String>> {
    let content = read(&root.join("docs").join(file))?;
    let found: Vec<String> = content
        .lines()
        .filter_map(|line| {
            let id = line.strip_prefix("| ")?.split(" | ").next()?;
            (id.starts_with(prefix) && id[1..].parse::<usize>().is_ok()).then(|| id.to_owned())
        })
        .collect();
    let expected: BTreeSet<_> = (1..=count).map(|n| format!("{prefix}{n:02}")).collect();
    ensure(
        found.len() == count && found.into_iter().collect::<BTreeSet<_>>() == expected,
        format!("{file}: missing or duplicate {prefix} definition"),
    )?;
    Ok(expected)
}

fn markdowns(path: &Path, result: &mut Vec<PathBuf>) -> Check<()> {
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let p = entry.path();
        if p.is_dir() {
            if !p.file_name().is_some_and(|n| {
                ["node_modules", "target", ".git", "archive"]
                    .iter()
                    .any(|skip| n == *skip)
            }) {
                markdowns(&p, result)?;
            }
        } else if p.extension().is_some_and(|e| e == "md") {
            result.push(p);
        }
    }
    Ok(())
}

fn planning_markdowns(root: &Path) -> Check<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let p = entry.map_err(|e| e.to_string())?.path();
        if p.is_file() && p.extension().is_some_and(|e| e == "md") {
            files.push(p);
        }
    }
    for directory in ["docs", "openspec", "community", ".github"] {
        markdowns(&root.join(directory), &mut files)?;
    }
    files.sort();
    Ok(files)
}

fn links_and_stale_text(root: &Path) -> Check<usize> {
    let mut count = 0;
    for path in planning_markdowns(root)? {
        let content = read(&path)?;
        for stale in [
            "Rust/WASM 웹",
            "웹은 Leptos/WASM",
            "PostgreSQL 하나를 제품 정본",
            "설치별 소유자 한 명",
            "TypeScript test 선언221",
        ] {
            ensure(
                !content.contains(stale),
                format!("{}: stale assumption: {stale}", path.display()),
            )?;
        }
        for part in content.split("](").skip(1) {
            let raw = part.split(')').next().unwrap_or("");
            let link = raw.trim().trim_matches(['<', '>']);
            if link.contains("://") || link.starts_with('#') || link.starts_with("mailto:") {
                continue;
            }
            let target = link.split('#').next().unwrap_or("");
            ensure(
                !target.is_empty(),
                format!("{}: empty link", path.display()),
            )?;
            ensure(
                !Path::new(target).is_absolute(),
                format!("{}: absolute local link", path.display()),
            )?;
            let resolved = path.parent().unwrap().join(target);
            ensure(
                resolved.exists(),
                format!("{}: missing link {target}", path.display()),
            )?;
            let canonical = resolved.canonicalize().map_err(|e| e.to_string())?;
            ensure(
                canonical.starts_with(root),
                format!("{}: link escapes package", path.display()),
            )?;
            count += 1;
        }
    }
    Ok(count)
}

fn scenarios(root: &Path, features: &BTreeSet<String>) -> Check<BTreeSet<String>> {
    let rows = table(
        root,
        "evaluation-scenarios.tsv",
        &[
            "id", "area", "features", "phases", "layers", "gate", "purpose", "setup", "stimulus",
            "oracle", "fault", "metric", "status",
        ],
    )?;
    ensure(!rows.is_empty(), "scenario catalog is empty")?;
    let areas = [
        ("MEM", "memory"),
        ("CHAT", "chat"),
        ("TASK", "mission"),
        ("SKILL", "skill"),
        ("AUTO", "automation"),
        ("DATA", "data"),
        ("PROV", "provider"),
        ("UX", "ux"),
        ("SEC", "security"),
        ("VAULT", "vault"),
        ("OPS", "operations"),
        ("COMM", "community"),
        ("CRED", "credential"),
    ];
    let mut ids = BTreeSet::new();
    let mut covered = BTreeSet::new();
    for row in &rows {
        let id = &row[0];
        ensure(ids.insert(id.clone()), format!("duplicate scenario {id}"))?;
        ensure(
            areas.iter().any(|(prefix, area)| {
                row[1] == *area
                    && id.strip_prefix(prefix).is_some_and(|n| {
                        n.len() == 2 && n.bytes().all(|b| b.is_ascii_digit()) && n != "00"
                    })
            }),
            format!("{id}: invalid scenario ID/area"),
        )?;
        let refs: Vec<_> = row[2].split(',').collect();
        ensure(
            refs.iter().all(|id| features.contains(*id))
                && refs.iter().collect::<BTreeSet<_>>().len() == refs.len(),
            format!("{id}: invalid/duplicate scenario feature"),
        )?;
        covered.extend(refs.into_iter().map(String::from));
        ensure(
            row[3].split(',').all(phase),
            format!("{id}: invalid scenario phase"),
        )?;
        ensure(
            row[4].split(',').all(|layer| {
                ["contract", "fault", "model", "ui", "load", "device", "live"].contains(&layer)
            }),
            format!("{id}: invalid scenario layer"),
        )?;
        ensure(
            ["blocker", "quality", "performance"].contains(&row[5].as_str()),
            format!("{id}: invalid scenario gate"),
        )?;
        ensure(
            row[12] == "planned",
            format!("{id}: product tests are not implemented"),
        )?;
    }
    let missing: Vec<_> = features.difference(&covered).collect();
    ensure(
        missing.is_empty(),
        format!("features without scenarios: {missing:?}"),
    )?;
    for path in planning_markdowns(root)? {
        for token in read(&path)?.split(|c: char| !c.is_ascii_alphanumeric()) {
            let looks_like_id = areas.iter().any(|(prefix, _)| {
                token
                    .strip_prefix(prefix)
                    .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
            });
            ensure(
                !looks_like_id || ids.contains(token),
                format!("{}: unknown scenario {token}", path.display()),
            )?;
        }
    }
    Ok(ids)
}

fn verify(root: &Path) -> Check<()> {
    let features = definitions(root, "features.md", 'F', 70)?;
    let requirements = definitions(root, "requirements.md", 'R', 29)?;
    let inventory = table(
        root,
        "source-inventory.tsv",
        &[
            "source",
            "sha256",
            "bytes",
            "disposition",
            "target",
            "phase",
            "features",
            "reason",
            "evidence",
            "test_contract",
        ],
    )?;
    let manifest = table(root, "source-manifest.tsv", &["source", "sha256", "bytes"])?;
    ensure(
        inventory.len() == 523 && manifest.len() == 523,
        "source baseline must contain 523 rows",
    )?;
    let mut indexed = BTreeMap::new();
    let allowed = [
        "rewrite",
        "contract",
        "reference",
        "exclude",
        "optional",
        "replace",
    ];
    for row in &inventory {
        ensure(
            !Path::new(&row[0]).is_absolute() && !row[0].split('/').any(|s| s == ".."),
            "invalid source path",
        )?;
        ensure(
            row[1].len() == 64 && row[1].bytes().all(|c| c.is_ascii_hexdigit()),
            "invalid SHA256",
        )?;
        ensure(row[2].parse::<u64>().is_ok(), "invalid byte count")?;
        ensure(
            allowed.contains(&row[3].as_str()) && phase(&row[5]),
            "invalid disposition/phase",
        )?;
        if row[0].starts_with("tests/") || row[0].contains("/tests/") {
            ensure(
                ["reference", "exclude"].contains(&row[3].as_str()),
                format!("{}: legacy tests must not be a porting backlog", row[0]),
            )?;
        }
        ensure(
            row[6].split(',').all(|id| features.contains(id)),
            format!("{}: unknown feature", row[0]),
        )?;
        ensure(
            indexed.insert(&row[0], (&row[1], &row[2])).is_none(),
            "duplicate inventory source",
        )?;
    }
    let mut seen = BTreeSet::new();
    for row in &manifest {
        ensure(seen.insert(&row[0]), "duplicate manifest source")?;
        ensure(
            indexed.get(&row[0]) == Some(&(&row[1], &row[2])),
            format!("{}: manifest mismatch", row[0]),
        )?;
    }
    let entries = table(
        root,
        "entrypoints.tsv",
        &[
            "source", "line", "kind", "method", "name", "symbol", "target", "phase", "features",
        ],
    )?;
    let mut kinds = BTreeMap::new();
    let mut entry_keys = BTreeSet::new();
    for row in &entries {
        ensure(
            indexed.contains_key(&row[0]) && row[1].parse::<usize>().is_ok(),
            "invalid entrypoint source/line",
        )?;
        ensure(
            phase(&row[7]) && row[8].split(',').all(|id| features.contains(id)),
            "invalid entrypoint plan mapping",
        )?;
        ensure(
            entry_keys.insert((&row[0], &row[1], &row[2], &row[4])),
            "duplicate entrypoint",
        )?;
        *kinds.entry(row[2].as_str()).or_insert(0) += 1;
    }
    ensure(
        entries.len() == 108
            && kinds.get("http_route") == Some(&59)
            && kinds.get("model_tool") == Some(&49),
        format!("unexpected entrypoint counts: {kinds:?}"),
    )?;
    let tests = table(
        root,
        "test-surfaces.tsv",
        &[
            "source",
            "language",
            "declarations",
            "lines",
            "target",
            "phase",
            "features",
        ],
    )?;
    let mut totals = BTreeMap::new();
    let mut test_sources = BTreeSet::new();
    for row in &tests {
        ensure(
            indexed.contains_key(&row[0]) && test_sources.insert(&row[0]),
            "invalid/duplicate test source",
        )?;
        let n = row[2].parse::<usize>().map_err(|e| e.to_string())?;
        ensure(
            n > 0
                && row[3].split(',').count() == n
                && row[3].split(',').all(|s| s.parse::<usize>().is_ok()),
            "invalid test lines/count",
        )?;
        ensure(
            phase(&row[5]) && row[6].split(',').all(|id| features.contains(id)),
            "invalid test plan mapping",
        )?;
        *totals.entry(row[1].as_str()).or_insert(0) += n;
    }
    ensure(
        tests.len() == 99
            && totals.get("python") == Some(&772)
            && totals.get("typescript") == Some(&215),
        "unexpected static test counts",
    )?;
    let scenario_ids = scenarios(root, &features)?;
    let connector_count =
        source_connections::verify(root, &features, &scenario_ids, &inventory, &entries)?;
    let scenario_count = scenario_ids.len();
    let requirement_count = requirements.len();
    let feature_count = features.len();
    let links = links_and_stale_text(root)?;
    println!(
        "Plan verified: 523 sources, 108 entrypoints, 99 historical test files (772 Python / 215 TS declarations), {requirement_count} requirements, {feature_count} features, {scenario_count} planned scenario groups, {connector_count} connector scope records, {links} local links."
    );
    println!(
        "This validates planning metadata and links, not product behavior or cryptographic security."
    );
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args != ["verify-plan"] {
        eprintln!("Usage: cargo run --locked -p xtask -- verify-plan");
        std::process::exit(2);
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let result = root
        .canonicalize()
        .map_err(|e| e.to_string())
        .and_then(|root| verify(&root));
    if let Err(error) = result {
        eprintln!("Plan verification failed: {error}");
        std::process::exit(1);
    }
}
