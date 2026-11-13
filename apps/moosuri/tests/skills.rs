//! Actual-process authoring checks; all content is synthetic and no Store is opened.
use std::{
    collections::BTreeMap,
    fs,
    os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    },
    path::Path,
    process::{Command, Output},
};
fn command(root: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_moosuri"));
    cmd.current_dir(root)
        .arg("--data-dir")
        .arg(root.join("missing-installation"));
    cmd
}
fn init(root: &Path, slug: &str, output: &Path) -> Output {
    command(root)
        .args(["skills", "init", slug, "--output"])
        .arg(output)
        .output()
        .unwrap()
}
fn check(root: &Path, directory: &Path) -> Output {
    command(root)
        .args(["skills", "check"])
        .arg(directory)
        .output()
        .unwrap()
}
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().into_string().unwrap(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}
fn root() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}
fn rejected(result: Output, reason: &str) {
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains(reason),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn cli_init_and_check_work_in_repository_paths_without_installation_state() {
    let root = root();
    fs::write(
        root.path().join("Cargo.toml"),
        "synthetic repository marker",
    )
    .unwrap();
    fs::create_dir(root.path().join(".git")).unwrap();
    let before_mode = fs::metadata(root.path()).unwrap().permissions().mode();
    let generated = command(root.path())
        .args(["skills", "init", "synthetic-summary"])
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "{}",
        String::from_utf8_lossy(&generated.stderr)
    );
    let package = root.path().join("synthetic-summary");
    let before = snapshot(&package);
    assert_eq!(before.len(), 4);
    assert_eq!(before["LICENSE"], include_bytes!("../../../LICENSE-APACHE"));
    let examples: serde_json::Value = serde_json::from_slice(&before["examples.json"]).unwrap();
    assert_eq!(examples["examples"][0]["kind"], "normal");
    assert_eq!(examples["examples"][1]["kind"], "unsupported");
    let checked = check(root.path(), Path::new("synthetic-summary"));
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    assert!(String::from_utf8_lossy(&checked.stdout).contains("Content compatibility only"));
    assert_eq!(snapshot(&package), before);
    assert!(
        Command::new(env!("CARGO_BIN_EXE_moosuri"))
            .current_dir(root.path())
            .args(["skills", "check", "synthetic-summary"])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::metadata(root.path()).unwrap().permissions().mode(),
        before_mode
    );
    let custom = root.path().join("different-directory-name");
    assert!(
        init(root.path(), "another-summary", &custom)
            .status
            .success()
    );
    assert!(check(root.path(), &custom).status.success());
    assert!(!root.path().join("missing-installation").exists());
}

#[test]
fn cli_content_diagnostics_and_byte_limits_are_read_only_and_never_echo_content() {
    let root = root();
    let package = root.path().join("package");
    assert!(
        init(root.path(), "synthetic-summary", &package)
            .status
            .success()
    );
    let original = snapshot(&package);
    for (file, text, reason) in [
        (
            "moosuri.toml",
            String::from_utf8(original["moosuri.toml"].clone())
                .unwrap()
                .replace("knowledge_search", "private-canary-shell"),
            "moosuri.toml/LICENSE",
        ),
        (
            "moosuri.toml",
            "format_version = 1\nhooks = []\n".into(),
            "moosuri.toml",
        ),
        (
            "SKILL.md",
            "---\nname: private-canary\n---\n".into(),
            "SKILL.md",
        ),
        ("examples.json", "{}".into(), "examples.json"),
        ("LICENSE", "x".repeat(32769), "skills_file_limit"),
        ("SKILL.md", "\0".into(), "skills_compatibility"),
    ] {
        fs::write(package.join(file), text).unwrap();
        let before = snapshot(&package);
        let out = check(root.path(), &package);
        assert!(!String::from_utf8_lossy(&out.stderr).contains("private-canary"));
        rejected(out, reason);
        assert_eq!(snapshot(&package), before);
        fs::write(package.join(file), &original[file]).unwrap();
    }
    fs::write(package.join("LICENSE"), "x".repeat(32768)).unwrap();
    fs::write(package.join("SKILL.md"), "y".repeat(32768)).unwrap();
    rejected(check(root.path(), &package), "skills_file_limit");
    assert!(!root.path().join("missing-installation").exists());
}

#[test]
fn cli_check_rejects_extra_missing_linked_executable_and_special_files() {
    let root = root();
    for case in 0..8 {
        let package = root.path().join(format!("case-{case}"));
        assert!(
            init(root.path(), "synthetic-summary", &package)
                .status
                .success()
        );
        let skill = package.join("SKILL.md");
        let mut socket = None;
        match case {
            0 => fs::write(package.join("extra"), "synthetic unknown file").unwrap(),
            1 => fs::remove_file(package.join("LICENSE")).unwrap(),
            2 => {
                fs::remove_file(&skill).unwrap();
                symlink("LICENSE", &skill).unwrap();
            }
            3 => {
                fs::remove_file(&skill).unwrap();
                fs::hard_link(package.join("LICENSE"), &skill).unwrap();
            }
            4 => fs::set_permissions(&skill, fs::Permissions::from_mode(0o755)).unwrap(),
            5 => {
                fs::remove_file(&skill).unwrap();
                socket = Some(UnixListener::bind(&skill).unwrap());
            }
            6 => {
                fs::remove_file(&skill).unwrap();
                fs::create_dir(&skill).unwrap();
            }
            _ => fs::write(&skill, [0xff, 0xfe]).unwrap(),
        }
        let result = check(root.path(), &package);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        drop(socket);
    }
    assert!(!root.path().join("missing-installation").exists());
}

#[test]
fn cli_paths_reject_existing_destinations_traversal_links_and_application_data() {
    let root = root();
    let existing = root.path().join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("sentinel"), "synthetic retained bytes").unwrap();
    let before = snapshot(&existing);
    rejected(
        init(root.path(), "valid-name", &existing),
        "skills_destination_exists",
    );
    assert_eq!(snapshot(&existing), before);
    let file = root.path().join("file");
    fs::write(&file, "synthetic retained file").unwrap();
    rejected(
        init(root.path(), "valid-name", &file),
        "skills_destination_exists",
    );
    assert_eq!(fs::read(&file).unwrap(), b"synthetic retained file");
    for (link, target) in [
        ("linked", &existing),
        ("dangling", &root.path().join("absent")),
    ] {
        let linked = root.path().join(link);
        symlink(target, &linked).unwrap();
        rejected(
            init(root.path(), "valid-name", &linked),
            "skills_destination_exists",
        );
        assert!(!check(root.path(), &linked).status.success());
        assert!(
            !init(root.path(), "valid-name", &linked.join("child"))
                .status
                .success()
        );
        assert!(
            fs::symlink_metadata(linked)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
    assert!(
        !init(root.path(), "../escape", &root.path().join("escape"))
            .status
            .success()
    );
    rejected(
        init(
            root.path(),
            "valid-name",
            Path::new("existing/../traversal"),
        ),
        "skills_invalid_path",
    );
    rejected(
        check(root.path(), Path::new("existing/../existing")),
        "skills_invalid_path",
    );
    assert!(
        !init(
            root.path(),
            "valid-name",
            &root.path().join("missing-parent/new")
        )
        .status
        .success()
    );
    for marker in [
        "assistant.sqlite",
        "writer.lock",
        "provider-secrets.key",
        "auth-epochs.journal",
        "knowledge-invalidations.journal",
        "destructive-intents.journal",
    ] {
        let installation = root.path().join(format!("app-{marker}"));
        fs::create_dir(&installation).unwrap();
        fs::write(installation.join(marker), "synthetic data marker").unwrap();
        fs::create_dir(installation.join("nested")).unwrap();
        rejected(
            init(root.path(), "valid-name", &installation.join("nested/new")),
            "skills_application_data_path",
        );
        rejected(
            check(root.path(), &installation),
            "skills_application_data_path",
        );
        assert_eq!(
            fs::read(installation.join(marker)).unwrap(),
            b"synthetic data marker"
        );
        assert!(!installation.join("nested/new").exists());
    }
    assert!(!root.path().join("missing-installation").exists());
}

#[test]
fn simultaneous_init_publishes_one_complete_package_without_replacement_or_partial_debris() {
    let root = root();
    let package = root.path().join("package");
    let mut first = command(root.path())
        .args(["skills", "init", "first-name", "--output"])
        .arg(&package)
        .spawn()
        .unwrap();
    let mut second = command(root.path())
        .args(["skills", "init", "second-name", "--output"])
        .arg(&package)
        .spawn()
        .unwrap();
    let statuses = [
        first.wait().unwrap().success(),
        second.wait().unwrap().success(),
    ];
    assert_eq!(statuses.into_iter().filter(|s| *s).count(), 1);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    assert_eq!(snapshot(&package).len(), 4);
    assert!(check(root.path(), &package).status.success());
}
