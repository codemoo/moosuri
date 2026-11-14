use super::*;

// Only private unit tests can feed synthetic objects to Builder. The public
// production API obtains its private carrier exclusively from the fixed adapter.
fn fixture(package: Package, directory: &str) -> Validated {
    external::decode(
        &serde_json::json!({
            "format_version": 2, "repository": package.source,
            "directory": directory, "commit": package.commit,
            "slug": package.slug, "files": package.files,
        })
        .to_string(),
    )
    .unwrap()
}
fn selection(entries: &[Validated]) -> Selection {
    Selection::new(
        SOURCE,
        COMMIT,
        entries
            .iter()
            .map(|e| format!("community/skills/{}", e.package.slug))
            .collect(),
    )
    .unwrap()
}
#[test]
fn six_packages_preserve_curated_pins_and_generate_deterministic_existing_v2_envelopes() {
    let entries = catalog().unwrap(); // Verifies all six unchanged curated v1 digests.
    assert_eq!(entries.len(), 6);
    let selected = selection(&entries);
    let mut forward = Builder::new(&selected);
    let mut reverse = Builder::new(&selected);
    let originals: BTreeMap<_, _> = entries
        .iter()
        .map(|e| (e.package.slug.clone(), e.package.files.clone()))
        .collect();
    for entry in entries {
        let dir = format!("community/skills/{}", entry.package.slug);
        forward.add(fixture(entry.package.clone(), &dir)).unwrap();
    }
    for entry in catalog().unwrap().into_iter().rev() {
        let dir = format!("community/skills/{}", entry.package.slug);
        reverse.add(fixture(entry.package, &dir)).unwrap();
    }
    let actual = forward.finish().unwrap();
    assert_eq!(actual.files(), reverse.finish().unwrap().files());
    assert_eq!(actual.package_count(), 6);
    assert_eq!(actual.files().len(), 7);
    let index: serde_json::Value = serde_json::from_str(&actual.files()["catalog.json"]).unwrap();
    assert_eq!(index["source_commit"], COMMIT);
    let mut prior = String::new();
    for item in index["packages"].as_array().unwrap() {
        let directory = item["directory"].as_str().unwrap();
        assert!(directory > prior.as_str());
        prior = directory.into();
        let bytes = &actual.files()[item["artifact"].as_str().unwrap()];
        let decoded = external::decode(bytes).unwrap();
        assert_eq!(decoded.package.files, originals[&decoded.package.slug]);
        assert_eq!(decoded.package.commit, COMMIT);
        assert_eq!(decoded.digest, item["content_digest"].as_str().unwrap());
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            item["artifact_sha256"]
        );
        assert_eq!(external::encode(&decoded).unwrap(), *bytes);
    }
    // The index never includes a claim about its own Git commit or bytes.
    assert!(index.get("catalog_commit").is_none());
}
#[test]
fn exact_selection_rejects_branches_local_claims_duplicates_and_invalid_bounds() {
    for commit in [
        "main",
        "",
        "ABCDEFabcdefabcdefabcdefabcdefabcdefabcd",
        "not-a-commit",
    ] {
        assert!(Selection::new(SOURCE, commit, vec!["community/skills/example".into()]).is_err());
    }
    for dirs in [
        vec![],
        vec!["../private-canary".into()],
        vec!["a".into(), "a".into()],
        vec!["a".into(); 17],
    ] {
        let error = Selection::new(SOURCE, COMMIT, dirs).err().unwrap();
        assert!(!error.to_string().contains("private-canary"));
    }
    assert!(Selection::new("file:///private-canary", COMMIT, vec!["a".into()]).is_err());
    assert!(
        Selection::new(
            "https://private-canary@github.com/a/b",
            COMMIT,
            vec!["a".into()]
        )
        .is_err()
    );
    let mut packages = catalog().unwrap();
    let selected = selection(&packages);
    let package = packages.remove(0).package;
    let dir = format!("community/skills/{}", package.slug);
    let mut builder = Builder::new(&selected);
    let mut wrong = package.clone();
    wrong.commit = "1".repeat(40);
    assert!(builder.add(fixture(wrong, &dir)).is_err());
    assert!(builder.add(fixture(package.clone(), "other/path")).is_err());
    builder.add(fixture(package.clone(), &dir)).unwrap();
    assert!(builder.add(fixture(package, &dir)).is_err());
    assert!(builder.finish().is_err());
}
#[test]
fn package_mutations_fail_shared_compatibility_and_never_create_an_index() {
    let original = catalog().unwrap().remove(0).package;
    for case in 0..5 {
        let mut package = original.clone();
        match case {
            0 => {
                package
                    .files
                    .insert("script.sh".into(), "private-canary".into());
            }
            1 => {
                package
                    .files
                    .insert("LICENSE".into(), "private-canary".into());
            }
            2 => {
                package
                    .files
                    .get_mut("moosuri.toml")
                    .unwrap()
                    .push_str("hooks = []\n");
            }
            3 => {
                package.files.insert(
                    "moosuri.toml".into(),
                    package.files["moosuri.toml"]
                        .replace("knowledge_search", "private-canary-shell"),
                );
            }
            _ => {
                package
                    .files
                    .insert("SKILL.md".into(), "private-canary".repeat(4000));
            }
        }
        assert!(compatible_contents(package).is_err());
    }
}
