//! Focused boundary/oracle tests. A stateful fake host plays useradd, getent,
//! loginctl, systemctl, runuser (user systemctl + rootless docker) and curl, so
//! the real production code runs against a temporary layout. Not run by the
//! author (no shell); main must run them on Linux.
use super::*;
use crate::installation::kiri::host::{Input, Out};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    io::Write,
    os::unix::{fs::PermissionsExt, net::UnixListener},
};

const UID_BASE: u32 = 900;
const TOOLS: [&str; 16] = [
    "newuidmap",
    "newgidmap",
    "rootlesskit",
    "slirp4netns",
    "fuse-overlayfs",
    "loginctl",
    "runuser",
    "getent",
    "useradd",
    "curl",
    "dockerd",
    "containerd",
    "runc",
    "env",
    "systemctl",
    "docker",
];
const HELPER: &str = "/usr/local/bin/moosuri";
const IMAGE: &str =
    "ubuntu@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

struct User {
    name: String,
    uid: u32,
    gecos: String,
    home: String,
}
struct State {
    users: Vec<User>,
    crash_after_useradd: bool,
    memory_limit: bool,
    subids_supported: bool,
    pulled: bool,
    started: bool,
    sockets: Vec<UnixListener>,
}
struct Fake {
    root: PathBuf,
    cli: Vec<u8>,
    wrapper: Vec<u8>,
    calls: RefCell<Vec<String>>,
    st: RefCell<State>,
}

fn reply(code: i32, text: &str) -> Result<Out> {
    Ok(Out {
        code,
        stdout: text.as_bytes().to_vec(),
        stderr: Vec::new(),
    })
}
fn flag(a: &[String], name: &str) -> String {
    let i = a.iter().position(|x| x == name).unwrap();
    a[i + 1].clone()
}

impl Fake {
    fn getent(&self, a: &[String]) -> Result<Out> {
        let st = self.st.borrow();
        let Some(u) = st.users.iter().find(|u| u.name == a[1]) else {
            return reply(2, "");
        };
        match a[0].as_str() {
            "passwd" => reply(
                0,
                &format!(
                    "{}:x:{}:{}:{}:{}:{SHELL}\n",
                    u.name, u.uid, u.uid, u.gecos, u.home
                ),
            ),
            "group" => reply(0, &format!("{}:x:{}:\n", u.name, u.uid)),
            _ => reply(1, ""),
        }
    }
    fn useradd(&self, a: &[String]) -> Result<Out> {
        let mut st = self.st.borrow_mut();
        if a.len() == 1 && a[0] == "--help" {
            return reply(
                0,
                if st.subids_supported {
                    "  -F, --add-subids-for-system  add subids\n"
                } else {
                    "usage: useradd\n"
                },
            );
        }
        let n = st.users.len() as u32;
        let (home, gecos, name) = (
            flag(a, "--home-dir"),
            flag(a, "--comment"),
            a.last().unwrap().clone(),
        );
        let dir = self.root.join(home.trim_start_matches('/'));
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        let start = 100_000 + 65_536 * u64::from(n);
        for file in ["etc/subuid", "etc/subgid"] {
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(self.root.join(file))
                .unwrap();
            writeln!(f, "{name}:{start}:65536").unwrap();
        }
        st.users.push(User {
            name,
            uid: UID_BASE + n,
            gecos,
            home,
        });
        if st.crash_after_useradd {
            return Err("claude_runtime_test_interrupted".into());
        }
        reply(0, "")
    }
    fn runuser(&self, a: &[String]) -> Result<Out> {
        let mut i = a.iter().position(|x| x == "env").unwrap() + 1;
        while a[i].contains('=') {
            i += 1;
        }
        let tail = &a[i..];
        if tail[0] == "/usr/bin/test" {
            let path = self.root.join(tail[2].trim_start_matches('/'));
            let inaccessible = path
                .ancestors()
                .take_while(|p| p.starts_with(&self.root) && *p != self.root)
                .any(|p| fs::metadata(p).is_ok_and(|m| m.mode() & 0o005 == 0));
            return reply(if inaccessible { 1 } else { 0 }, "");
        }
        let mut st = self.st.borrow_mut();
        let (uid, home) = st
            .users
            .iter()
            .find(|u| u.name == a[1])
            .map(|u| (u.uid, u.home.clone()))
            .unwrap();
        if tail[0].ends_with("systemctl") {
            return match tail[2].as_str() {
                "show" => reply(
                    0,
                    &format!(
                        "FragmentPath={home}/.config/systemd/user/{}\nDropInPaths=\nActiveState={}\n",
                        service::UNIT,
                        if st.started { "active" } else { "inactive" }
                    ),
                ),
                "start" => {
                    let path = self.root.join(format!("run/user/{uid}/docker.sock"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    if !path.exists() {
                        st.sockets.push(UnixListener::bind(&path).unwrap());
                    }
                    st.started = true;
                    reply(0, "")
                }
                _ => reply(0, ""),
            };
        }
        match tail[3].as_str() {
            "info" => reply(0, &info(st.memory_limit)),
            "image" if st.pulled => reply(0, &json!([tail.last().unwrap()]).to_string()),
            "image" => reply(1, ""),
            "pull" => {
                st.pulled = true;
                reply(0, "")
            }
            _ => reply(1, ""),
        }
    }
    fn curl(&self, a: &[String]) -> Result<Out> {
        let bytes = if flag(a, "--url").ends_with("/claude") {
            &self.cli
        } else {
            &self.wrapper
        };
        fs::write(flag(a, "--output"), bytes).unwrap();
        reply(0, "")
    }
}

fn info(memory: bool) -> String {
    json!({
        "SecurityOptions": ["name=seccomp,profile=builtin", "name=rootless"],
        "CgroupDriver": "systemd",
        "CgroupVersion": "2",
        "MemoryLimit": memory,
        "CpuCfsPeriod": true,
        "CpuCfsQuota": true,
        "PidsLimit": true
    })
    .to_string()
}

impl Exec for Fake {
    fn run(&self, program: &str, a: &[String], _: Input, _: Duration) -> Result<Out> {
        self.calls
            .borrow_mut()
            .push(format!("{program} {}", a.join(" ")));
        match program {
            "getent" => self.getent(a),
            "useradd" => self.useradd(a),
            "loginctl" if a[0] == "show-user" => reply(0, "Linger=yes\n"),
            "systemctl" if a[0] == "start" => {
                let uid = a[1]
                    .trim_start_matches("user@")
                    .trim_end_matches(".service");
                fs::create_dir_all(self.root.join(format!("run/user/{uid}"))).unwrap();
                reply(0, "")
            }
            "loginctl" | "systemctl" => reply(0, ""),
            "runuser" => self.runuser(a),
            "curl" => self.curl(a),
            _ => reply(127, ""),
        }
    }
}

struct Env {
    _dir: tempfile::TempDir,
    root: PathBuf,
    layout: Layout,
    host: Fake,
    pins: Pins,
}

fn mkdir(root: &Path, rel: &str) {
    let mut p = root.to_path_buf();
    for c in rel.split('/') {
        p.push(c);
        if !p.exists() {
            fs::create_dir(&p).unwrap();
        }
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
fn put(path: &Path, bytes: &[u8], mode: u32) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).unwrap();
}
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for d in [
        "usr/bin",
        "usr/sbin",
        "usr/local/bin",
        "usr/lib/systemd/user",
        "etc/systemd/system",
        "sys/fs/cgroup",
        "var/lib",
        "opt",
        "run",
    ] {
        mkdir(&root, d);
    }
    for t in TOOLS {
        put(&root.join("usr/bin").join(t), t.as_bytes(), 0o755);
    }
    put(&root.join("usr/sbin/nologin"), b"nologin", 0o755);
    put(&root.join(HELPER.trim_start_matches('/')), b"helper", 0o755);
    put(&root.join("usr/lib/systemd/user/dbus.socket"), b"", 0o644);
    put(
        &root.join("sys/fs/cgroup/cgroup.controllers"),
        b"cpuset cpu io memory hugetlb pids",
        0o644,
    );
    for f in ["passwd", "group", "subuid", "subgid"] {
        put(&root.join("etc").join(f), b"", 0o644);
    }
    let cli = b"official-claude-test-bytes".to_vec();
    let wrapper = b"#!/bin/sh\nrootless\n".to_vec();
    let pins = Pins {
        cli: Pin {
            url: "https://example.invalid/claude/linux-x64/claude",
            sha256: leak(host::sha256_bytes(&cli)),
            size: Some(cli.len() as u64),
            max: cli.len() as u64,
        },
        cli_version: "9.9.9",
        wrapper: Pin {
            url: "https://example.invalid/dockerd-rootless.sh",
            sha256: leak(host::sha256_bytes(&wrapper)),
            size: None,
            max: 32_768,
        },
        wrapper_tag: "docker-test",
        image: IMAGE,
    };
    Env {
        layout: Layout::under(&root),
        host: Fake {
            root: root.clone(),
            cli,
            wrapper,
            calls: RefCell::default(),
            st: RefCell::new(State {
                users: Vec::new(),
                crash_after_useradd: false,
                memory_limit: true,
                subids_supported: true,
                pulled: false,
                started: false,
                sockets: Vec::new(),
            }),
        },
        pins,
        root,
        _dir: dir,
    }
}

fn request(id: u128, owner: u128, generation: i64) -> Request {
    Request::new(
        Uuid::from_u128(id),
        Owner {
            account_id: Uuid::from_u128(owner),
            tenant_id: Uuid::from_u128(owner + 1),
            volume_id: Uuid::from_u128(owner + 2),
            generation,
        },
    )
}
fn go(e: &Env, r: &Request) -> Result<PreparedRuntime> {
    prepare_pinned(&e.layout, &e.host, r.clone(), Path::new(HELPER), &e.pins)
}
fn calls(e: &Env) -> Vec<String> {
    e.host.calls.borrow().clone()
}
fn useradds(e: &Env) -> usize {
    calls(e)
        .iter()
        .filter(|c| c.starts_with("useradd --system"))
        .count()
}
fn receipt_path(e: &Env, r: &Request) -> PathBuf {
    e.root.join(format!(
        "var/lib/moosuri-claude-control/receipts/{}.json",
        r.request_id.hyphenated()
    ))
}

#[test]
fn request_boundaries() {
    let good = json!({
        "version": 1,
        "request_id": Uuid::from_u128(1),
        "owner": {
            "account_id": Uuid::from_u128(2),
            "tenant_id": Uuid::from_u128(3),
            "volume_id": Uuid::from_u128(4),
            "generation": 1
        }
    });
    assert!(Request::parse(good.to_string().as_bytes()).is_ok());
    let nil = json!(Uuid::nil());
    let mut cases: Vec<Value> = Vec::new();
    for edit in 0..9 {
        let mut v = good.clone();
        match edit {
            0 => v["version"] = json!(2),
            1 => v["request_id"] = nil.clone(),
            2 => v["owner"]["account_id"] = nil.clone(),
            3 => v["owner"]["tenant_id"] = nil.clone(),
            4 => v["owner"]["volume_id"] = nil.clone(),
            5 => v["owner"]["generation"] = json!(0),
            6 => v["owner"]["generation"] = json!(-1),
            // Executable paths, service names, uids etc. are never accepted.
            7 => v["service_name"] = json!("docker.service"),
            _ => v["owner"]["uid"] = json!(0),
        }
        cases.push(v);
    }
    for v in cases {
        assert!(Request::parse(v.to_string().as_bytes()).is_err(), "{v}");
    }
    assert!(Request::parse(b"{").is_err());

    let e = env();
    let file = e.root.join("request.json");
    put(&file, good.to_string().as_bytes(), 0o600);
    assert!(Request::load(&e.layout, Path::new("/request.json")).is_ok());
    put(&file, good.to_string().as_bytes(), 0o644);
    assert_eq!(
        Request::load(&e.layout, Path::new("/request.json")).unwrap_err(),
        "claude_runtime_request_not_private"
    );
    put(&file, &vec![b' '; 17 * 1024], 0o600);
    assert!(Request::load(&e.layout, Path::new("/request.json")).is_err());
    for bad in ["request.json", "/a/../request.json"] {
        assert_eq!(
            Request::load(&e.layout, Path::new(bad)).unwrap_err(),
            "claude_runtime_request_path_invalid"
        );
    }
}

#[test]
fn production_pins_are_exact_https_and_immutable() {
    assert!(PINS.cli.url.starts_with("https://") && PINS.wrapper.url.starts_with("https://"));
    assert_eq!(PINS.cli.size, Some(341_914_128));
    assert_eq!(PINS.wrapper.max, 32_768);
    for sha in [PINS.cli.sha256, PINS.wrapper.sha256] {
        assert!(host::is_hex(sha, 64));
    }
    assert!(PINS.image.starts_with("ubuntu@sha256:") && !PINS.image.contains("24.04"));
    assert!(host::is_hex(
        PINS.image.trim_start_matches("ubuntu@sha256:"),
        64
    ));
}

#[test]
fn prepares_credential_free_descriptor_and_replays_exactly() {
    let e = env();
    let r = request(1, 10, 1);
    let first = go(&e, &r).unwrap();
    assert!(first.service.username.starts_with("msc-") && first.service.username.len() == 31);
    assert_eq!(first.service.uid, UID_BASE);
    assert_eq!(
        first.rootless_socket,
        Path::new("/run/user/900/docker.sock")
    );
    assert_eq!(first.state, PreparationState::HostPreparedNotAccepted);
    assert_eq!(first.image, IMAGE);
    assert_eq!(first.engine.path, Path::new("/usr/bin/docker"));
    assert_eq!(first.helper.path, Path::new(HELPER));
    assert_eq!(first.helper.sha256, host::sha256_bytes(b"helper"));
    assert_eq!(first.official_cli.version, "9.9.9");
    assert_eq!(first.official_cli.sha256, e.pins.cli.sha256);
    assert_eq!(first.rootless_wrapper.sha256, e.pins.wrapper.sha256);
    let text = serde_json::to_string(&first).unwrap().to_lowercase();
    for forbidden in [
        "token",
        "email",
        "password",
        "credential",
        "profile",
        "model",
        "auth",
    ] {
        assert!(!text.contains(forbidden), "{forbidden}");
    }

    let seen = calls(&e);
    let curls: Vec<_> = seen.iter().filter(|c| c.starts_with("curl ")).collect();
    assert_eq!(curls.len(), 2);
    for c in curls {
        assert!(
            c.contains("--proto =https")
                && c.contains("--max-redirs 0")
                && c.contains("--url https://")
        );
    }
    let pull = seen.iter().find(|c| c.contains(" pull ")).unwrap();
    assert!(pull.contains("--host unix:///run/user/900/docker.sock"));
    assert!(pull.contains("DOCKER_CONFIG=/opt/moosuri-claude/docker-config-empty"));
    assert!(pull.ends_with(IMAGE));
    assert!(!seen.iter().any(|c| c.contains("ubuntu:")));

    let dropin = e
        .root
        .join("etc/systemd/system/user@900.service.d/moosuri-claude.conf");
    assert!(
        fs::read_to_string(dropin)
            .unwrap()
            .contains("Delegate=cpu cpuset io memory pids")
    );
    assert!(!e.root.join("etc/systemd/system/user@.service.d").exists());
    let unit = e
        .root
        .join(first.service.home.strip_prefix("/").unwrap())
        .join(".config/systemd/user")
        .join(service::UNIT);
    assert_eq!(
        fs::read_to_string(unit).unwrap(),
        service::unit_text(&first.rootless_wrapper.path)
    );
    let saved: Receipt = serde_json::from_slice(&fs::read(receipt_path(&e, &r)).unwrap()).unwrap();
    assert_eq!(saved.phase, Phase::Identified);
    assert_eq!(saved.owner, r.owner);
    assert_eq!(saved.identity.unwrap().uid, UID_BASE);

    e.host.calls.borrow_mut().clear();
    assert_eq!(go(&e, &r).unwrap(), first);
    let again = calls(&e);
    assert_eq!(useradds(&e), 0);
    assert!(!again.iter().any(|c| c.starts_with("curl ")));
    assert_eq!(e.host.st.borrow().users.len(), 1);
}

#[test]
fn replay_with_different_owner_or_request_fails_closed() {
    let e = env();
    let r = request(1, 10, 1);
    go(&e, &r).unwrap();
    let mut other_owner = r.clone();
    other_owner.owner.generation = 2;
    assert_eq!(
        go(&e, &other_owner).unwrap_err(),
        "claude_runtime_request_owner_mismatch"
    );
    assert_eq!(
        go(&e, &request(2, 10, 1)).unwrap_err(),
        "claude_runtime_owner_already_prepared"
    );
    assert_eq!(e.host.st.borrow().users.len(), 1);
}

#[test]
fn foreign_artifact_receipt_and_directory_entries_are_never_adopted() {
    let e = env();
    mkdir(&e.root, "opt/moosuri-claude");
    mkdir(&e.root, "opt/moosuri-claude/claude");
    mkdir(&e.root, "opt/moosuri-claude/claude/9.9.9");
    let cli = e.root.join("opt/moosuri-claude/claude/9.9.9/claude");
    put(&cli, b"foreign", 0o755);
    assert_eq!(
        go(&e, &request(1, 10, 1)).unwrap_err(),
        "claude_runtime_artifact_conflict"
    );
    assert_eq!(fs::read(&cli).unwrap(), b"foreign");
    assert_eq!(useradds(&e), 0);

    let e = env();
    let r = request(1, 10, 1);
    go(&e, &r).unwrap();
    let mut forged: Receipt =
        serde_json::from_slice(&fs::read(receipt_path(&e, &r)).unwrap()).unwrap();
    forged.username = "msc-someone-else".into();
    fs::write(receipt_path(&e, &r), serde_json::to_vec(&forged).unwrap()).unwrap();
    assert_eq!(go(&e, &r).unwrap_err(), "claude_runtime_receipt_conflict");

    let e = env();
    go(&e, &request(1, 10, 1)).unwrap();
    put(
        &e.root
            .join("var/lib/moosuri-claude-control/receipts/stray.txt"),
        b"x",
        0o600,
    );
    assert_eq!(
        go(&e, &request(2, 20, 1)).unwrap_err(),
        "claude_runtime_receipt_directory_foreign"
    );
}

#[test]
fn worker_modified_unit_is_not_trusted_on_recovery() {
    let e = env();
    let r = request(1, 10, 1);
    let first = go(&e, &r).unwrap();
    let unit = e
        .root
        .join(first.service.home.strip_prefix("/").unwrap())
        .join(".config/systemd/user")
        .join(service::UNIT);
    fs::write(&unit, "[Service]\nExecStart=/bin/sh\n").unwrap();
    e.host.calls.borrow_mut().clear();
    assert_eq!(go(&e, &r).unwrap_err(), "claude_runtime_user_unit_conflict");
    fs::remove_file(&unit).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", &unit).unwrap();
    assert_eq!(go(&e, &r).unwrap_err(), "claude_runtime_user_unit_conflict");
    // Rejected before any systemd/user-manager command touched the service.
    assert!(!calls(&e).iter().any(|c| c.starts_with("systemctl")
        || c.starts_with("runuser")
        || c.starts_with("loginctl")));
}

#[test]
fn absent_resource_support_fails_before_image_pull() {
    let e = env();
    e.host.st.borrow_mut().memory_limit = false;
    assert_eq!(
        go(&e, &request(1, 10, 1)).unwrap_err(),
        "claude_runtime_resource_contract_unsupported"
    );
    assert!(!calls(&e).iter().any(|c| c.contains(" pull ")));
}

#[test]
fn interrupted_useradd_recovers_only_the_exact_marked_account() {
    let e = env();
    let r = request(1, 10, 1);
    e.host.st.borrow_mut().crash_after_useradd = true;
    assert_eq!(go(&e, &r).unwrap_err(), "claude_runtime_test_interrupted");
    e.host.st.borrow_mut().crash_after_useradd = false;
    let done = go(&e, &r).unwrap();
    assert_eq!(done.service.uid, UID_BASE);
    assert_eq!((e.host.st.borrow().users.len(), useradds(&e)), (1, 1));

    // Same interruption, but the account no longer carries this request's marker.
    let e = env();
    e.host.st.borrow_mut().crash_after_useradd = true;
    let _ = go(&e, &r);
    e.host.st.borrow_mut().crash_after_useradd = false;
    e.host.st.borrow_mut().users[0].gecos = "someone else".into();
    assert_eq!(go(&e, &r).unwrap_err(), "claude_runtime_foreign_account");

    // An unmanaged account with the derived name is never adopted or receipted.
    let e = env();
    e.host.st.borrow_mut().users.push(User {
        name: receipt::username(&r.request_id),
        uid: 700,
        gecos: "unmanaged".into(),
        home: "/home/x".into(),
    });
    assert_eq!(
        go(&e, &r).unwrap_err(),
        "claude_runtime_account_exists_unmanaged"
    );
    assert_eq!(
        fs::read_dir(e.root.join("var/lib/moosuri-claude-control/receipts"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn overlapping_subordinate_range_fails_closed() {
    let e = env();
    fs::write(e.root.join("etc/subuid"), "other:100000:65536\n").unwrap();
    assert_eq!(
        go(&e, &request(1, 10, 1)).unwrap_err(),
        "claude_runtime_subid_overlap"
    );
    assert!(!calls(&e).iter().any(|c| c.starts_with("loginctl")));
}

#[test]
fn missing_or_unsupported_prerequisites_fail_before_any_write() {
    let untouched = |e: &Env| {
        assert!(!e.root.join("var/lib/moosuri-claude-control").exists());
        assert!(!e.root.join("opt/moosuri-claude").exists());
        assert!(!e.root.join("var/lib/moosuri-claude").exists());
    };
    let r = request(1, 10, 1);
    let e = env();
    fs::remove_file(e.root.join("usr/bin/newuidmap")).unwrap();
    assert_eq!(go(&e, &r).unwrap_err(), "claude_runtime_missing_newuidmap");
    untouched(&e);

    let e = env();
    e.host.st.borrow_mut().subids_supported = false;
    assert_eq!(
        go(&e, &r).unwrap_err(),
        "claude_runtime_unsupported_useradd_subids"
    );
    untouched(&e);

    let e = env();
    fs::write(e.root.join("sys/fs/cgroup/cgroup.controllers"), "cpu io").unwrap();
    assert_eq!(
        go(&e, &r).unwrap_err(),
        "claude_runtime_cgroup_controllers_unavailable"
    );
    untouched(&e);
}

#[test]
fn private_helper_ancestor_cannot_be_reported_worker_ready() {
    let e = env();
    fs::set_permissions(e.root.join("usr/local"), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        go(&e, &request(1, 10, 1)).unwrap_err(),
        "claude_runtime_worker_artifact_inaccessible"
    );
    assert!(!calls(&e).iter().any(|call| call.starts_with("loginctl")));
}
