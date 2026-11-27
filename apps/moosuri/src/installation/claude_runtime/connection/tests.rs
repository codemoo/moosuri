//! Pure, root-free checks of the connection's authority boundaries: request
//! shape, replay decisions, root evidence reproduction, worker config and unit
//! content. Nothing here uses a bypass that exists in production code.
use super::evidence::{self, ProfileConfig};
use super::record::{Attempt, Evidence, Phase, Record, Storage, owner_key};
use super::*;
use crate::installation::claude_runtime::{
    PinnedCli, PreparationState, ServiceIdentity, prereq::PinnedFile,
};
use moosuri_connectors::claude_code::IsolationReceipt;
use serde_json::{Value, json};

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn owner() -> Owner {
    Owner {
        account_id: Uuid::from_u128(1),
        tenant_id: Uuid::from_u128(2),
        volume_id: Uuid::from_u128(3),
        generation: 1,
    }
}
fn request() -> Request {
    Request {
        request_id: Uuid::from_u128(0x10),
        owner: owner(),
        email: "owner@example.com".into(),
        model: "claude-opus-4-7".into(),
        effort: Some("xhigh".into()),
        hosting_terms_accepted: true,
    }
}
fn sha(bytes: &[u8]) -> String {
    host::sha256_bytes(bytes)
}
fn pin(path: &str, byte: char) -> PinnedFile {
    PinnedFile {
        path: path.into(),
        sha256: byte.to_string().repeat(64),
    }
}
fn runtime() -> PreparedRuntime {
    PreparedRuntime {
        version: 1,
        request_id: Uuid::from_u128(0x10),
        owner: owner(),
        service: ServiceIdentity {
            username: "msc-00000000000000000000000010".into(),
            uid: 901,
            gid: 902,
            home: "/var/lib/moosuri-claude/msc-00000000000000000000000010".into(),
        },
        rootless_socket: "/run/user/901/docker.sock".into(),
        engine: pin("/usr/bin/docker", '1'),
        official_cli: PinnedCli {
            path: "/opt/moosuri-claude/claude/2.1.322/claude".into(),
            sha256: "2".repeat(64),
            version: "2.1.322".into(),
        },
        rootless_wrapper: pin(
            "/opt/moosuri-claude/rootless/docker-v29.1.3/dockerd-rootless.sh",
            '3',
        ),
        image: format!("ubuntu@sha256:{}", "4".repeat(64)),
        helper: pin("/usr/local/bin/moosuri", '5'),
        state: PreparationState::HostPreparedNotAccepted,
    }
}
fn storage() -> Storage {
    Storage {
        auth_root: "/var/lib/moosuri-claude-fs/mounts/u/auth".into(),
        scratch_root: "/var/lib/moosuri-claude-fs/mounts/u/scratch".into(),
        filesystem_unit: "var-lib-moosuri\\x2dclaude\\x2dfs-mounts-u.mount".into(),
        capacity_bytes: 128 * 1024 * 1024,
        maximum_inodes: 32768,
    }
}

// ---- request boundaries ------------------------------------------------

#[test]
fn request_shape_is_exact() {
    assert!(request().validate().is_ok());
    let mut r = request();
    r.hosting_terms_accepted = false;
    assert_eq!(
        r.validate().unwrap_err(),
        "claude_connection_terms_required"
    );
    let bad = |f: &dyn Fn(&mut Request)| {
        let mut r = request();
        f(&mut r);
        assert_eq!(
            r.validate().unwrap_err(),
            "claude_connection_request_invalid"
        );
    };
    bad(&|r| r.email = "no-at-sign".into());
    bad(&|r| r.email = "a@b".into());
    bad(&|r| r.email = "a b@example.com".into());
    bad(&|r| r.email = "a@@example.com".into());
    bad(&|r| r.email = format!("{}@example.com", "x".repeat(250)));
    bad(&|r| r.model = "gpt-5".into());
    bad(&|r| r.model = "claude-".into());
    bad(&|r| r.model = "claude-opus 4".into());
    bad(&|r| r.model = "Claude-opus".into());
    bad(&|r| r.effort = Some(String::new()));
    bad(&|r| r.effort = Some("high; rm".into()));
    let mut r = request();
    r.effort = None;
    assert!(r.validate().is_ok());
    r.request_id = Uuid::nil();
    assert!(r.validate().is_err());
}

#[test]
fn request_json_rejects_unknown_fields() {
    let mut value = serde_json::to_value(request()).unwrap();
    assert!(serde_json::from_value::<Request>(value.clone()).is_ok());
    value["path"] = json!("/tmp/x");
    assert!(serde_json::from_value::<Request>(value).is_err());
}

// ---- replay / owner decisions ------------------------------------------

fn record(phase: Phase) -> Record {
    let r = request();
    let mut rec = new_record(&r, &sha(r.email.as_bytes()), r.request_id, None);
    rec.attempt.phase = phase;
    if matches!(
        phase,
        Phase::Prepared
            | Phase::LoginIssued
            | Phase::Activating
            | Phase::WorkerReady
            | Phase::Active
    ) {
        rec.evidence = Some(evidence_for(&runtime(), &storage()));
        rec.attempt.files = Some(Hashes {
            profile: A.into(),
            email: A.into(),
            worker: A.into(),
        });
    }
    rec
}

#[test]
fn exact_replay_only_and_one_current_setup_per_owner() {
    let r = request();
    let email = sha(r.email.as_bytes());
    assert_eq!(decide(None, &r, &email).unwrap(), Plan::Fresh);
    for phase in [
        Phase::Reserved,
        Phase::Prepared,
        Phase::LoginIssued,
        Phase::Activating,
        Phase::Active,
        Phase::Cleanup,
    ] {
        assert_eq!(
            decide(Some(&record(phase)), &r, &email).unwrap(),
            Plan::Replay,
            "{phase:?}"
        );
    }
    assert_eq!(
        decide(Some(&record(Phase::Retired)), &r, &email).unwrap_err(),
        "claude_connection_request_retired"
    );
    // Same request id, any changed input is a conflict, never a silent update.
    let rec = record(Phase::Prepared);
    let changes: Vec<Box<dyn Fn(&mut Request)>> = vec![
        Box::new(|r: &mut Request| r.email = "other@example.com".into()),
        Box::new(|r: &mut Request| r.model = "claude-sonnet-4".into()),
        Box::new(|r: &mut Request| r.effort = None),
    ];
    for change in changes {
        let mut changed = request();
        change(&mut changed);
        let sha = sha(changed.email.as_bytes());
        assert_eq!(
            decide(Some(&rec), &changed, &sha).unwrap_err(),
            "claude_connection_request_conflict"
        );
    }
    // A different request id for the same owner.
    let mut next = request();
    next.request_id = Uuid::from_u128(0x11);
    for (phase, want) in [
        (Phase::Retired, Some(Plan::Reuse)),
        (Phase::Cleanup, Some(Plan::FinishCleanup)),
        (Phase::Reserved, Some(Plan::Supersede)),
        (Phase::Prepared, Some(Plan::Supersede)),
        (Phase::LoginIssued, None),
        (Phase::Activating, None),
        (Phase::Active, None),
    ] {
        let got = decide(Some(&record(phase)), &next, &email);
        match want {
            Some(plan) => assert_eq!(got.unwrap(), plan, "{phase:?}"),
            None => assert_eq!(got.unwrap_err(), "claude_connection_setup_in_progress"),
        }
    }
    // The prepared runtime's own id is never restarted as a new attempt.
    let mut later = record(Phase::Retired);
    later.attempt.request_id = Uuid::from_u128(0x12);
    assert_eq!(
        decide(Some(&later), &r, &email).unwrap_err(),
        "claude_connection_request_retired"
    );
}

#[test]
fn records_bind_owner_phase_and_environment_class() {
    let mut other = owner();
    other.generation = 2;
    assert_ne!(owner_key(&owner()), owner_key(&other));
    assert!(record(Phase::Reserved).valid());
    assert!(record(Phase::Active).valid());
    let mut no_evidence = record(Phase::Active);
    no_evidence.evidence = None;
    assert!(!no_evidence.valid());
    let mut reserved_env = record(Phase::Reserved);
    // A version-8 UUID is the reserved credential-free probe namespace.
    reserved_env.attempt.environment_id =
        Uuid::parse_str("00000000-0000-8000-8000-000000000001").unwrap();
    assert!(!reserved_env.valid());
    let mut value = serde_json::to_value(record(Phase::Reserved)).unwrap();
    value["attempt"]["ready"] = json!(true);
    assert!(serde_json::from_value::<Record>(value).is_err());
    assert!(
        record(Phase::Reserved)
            .attempt
            .environment_id
            .get_version_num()
            == 4
    );
}

// ---- root evidence -------------------------------------------------------

fn receipt(candidate: &str) -> Value {
    json!({"version":1,"nonce":Uuid::from_u128(0x99).to_string(),
        "candidate_profile_digest":candidate,
        "memory_bytes":805306368u64,"cpu_quota":100000,"cpu_period":100000,"pids":32,
        "auth_filesystem_bytes":128*1024*1024,"auth_filesystem_inodes":32768,
        "rootless_uid_namespace":true,"no_new_privileges":true,"seccomp":true,
        "capabilities_dropped":true,"host_and_sibling_inaccessible":true,
        "immutable_mounts":true,"no_external_interface_or_route":true,
        "direct_network_denied":true,"proxy_destination_denied":true,
        "official_cli_version":"2.1.322"})
}

/// What the real admission would print for `expected`, with `mutate` applied
/// to the receipt first. Uses the same published serialization as the probe.
fn output(expected: &ProfileConfig, mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
    let bound = {
        let mut c = expected.clone();
        c.isolation_acceptance_sha256 = sha(&serde_json::to_vec(expected).unwrap());
        c
    };
    let mut value = receipt(&sha(&serde_json::to_vec(&bound).unwrap()));
    mutate(&mut value);
    let typed: IsolationReceipt = serde_json::from_value(value).unwrap();
    let mut accepted = expected.clone();
    accepted.isolation_acceptance_sha256 = sha(&serde_json::to_vec(&typed).unwrap());
    let profile = serde_json::to_string(&accepted).unwrap();
    serde_json::to_vec(&json!({
        "profile_json": profile, "profile_digest": sha(profile.as_bytes()), "receipt": typed
    }))
    .unwrap()
}

fn evidence_for(runtime: &PreparedRuntime, storage: &Storage) -> Evidence {
    let expected = evidence::candidate(runtime, storage).unwrap();
    let a = evidence::verify_output(&expected, &output(&expected, |_| {})).unwrap();
    Evidence {
        runtime: runtime.clone(),
        engine_stack: Vec::new(),
        storage: storage.clone(),
        candidate_sha256: a.candidate_sha256,
        profile_json: a.profile_json,
        profile_digest: a.profile_digest,
        receipt_json: a.receipt_json,
        receipt_sha256: a.receipt_sha256,
        kernel_release: "5.15.0-test".into(),
        boot_id: Uuid::from_u128(7).to_string(),
    }
}

#[test]
fn candidate_is_derived_only_from_root_facts() {
    let c = evidence::candidate(&runtime(), &storage()).unwrap();
    assert!(c.isolation_acceptance_sha256.is_empty());
    assert!(c.hosting_terms_accepted);
    assert_eq!(c.engine_socket, runtime().rootless_socket);
    let text = String::from_utf8(evidence::candidate_bytes(&c).unwrap()).unwrap();
    // Key order is the connector's canonical order.
    let keys = [
        "engine\"",
        "engine_sha256",
        "engine_socket",
        "image",
        "official_cli",
        "cli_sha256",
        "cli_version",
        "helper\"",
        "helper_sha256",
        "auth_root",
        "scratch_root",
        "isolation_acceptance_sha256",
        "hosting_terms_accepted",
    ];
    let mut last = 0;
    for key in keys {
        let at = text
            .find(&format!("\"{}", key))
            .unwrap_or_else(|| panic!("{key}"));
        assert!(at >= last, "{key}");
        last = at;
    }
    let mut other = runtime();
    other.helper = pin("/usr/local/bin/other", '5');
    assert_ne!(c, evidence::candidate(&other, &storage()).unwrap());
    let mut comma = runtime();
    comma.helper = pin("/usr/local/bin/a,b", '5');
    assert!(evidence::candidate(&comma, &storage()).is_err());
    let mut outside = storage();
    outside.auth_root = "/tmp/auth".into();
    assert!(evidence::candidate(&runtime(), &outside).is_err());
}

#[test]
fn exact_native_output_is_reproduced_from_root_inputs() {
    let expected = evidence::candidate(&runtime(), &storage()).unwrap();
    let admitted = evidence::verify_output(&expected, &output(&expected, |_| {})).unwrap();
    let stored: ProfileConfig = serde_json::from_str(&admitted.profile_json).unwrap();
    assert_eq!(stored.isolation_acceptance_sha256, admitted.receipt_sha256);
    assert_eq!(
        admitted.profile_digest,
        sha(admitted.profile_json.as_bytes())
    );
    evidence::validate_stored(&evidence_for(&runtime(), &storage())).unwrap();
}

#[test]
fn foreign_or_noncanonical_output_is_never_evidence() {
    let expected = evidence::candidate(&runtime(), &storage()).unwrap();
    let good = output(&expected, |_| {});
    let refused = |bytes: &[u8]| evidence::verify_output(&expected, bytes).is_err();

    // Valid evidence for a different profile (other engine hash) is a mismatch.
    let mut forged = expected.clone();
    forged.engine_sha256 = "9".repeat(64);
    assert!(refused(&output(&forged, |_| {})));
    // …as is a self-consistent output for another helper or auth root.
    let mut other = runtime();
    other.helper = pin("/usr/local/bin/moosuri", '6');
    let other = evidence::candidate(&other, &storage()).unwrap();
    assert!(refused(&output(&other, |_| {})));
    // Semantically equal JSON with different key order is not canonical.
    let parsed: Value = serde_json::from_slice(&good).unwrap();
    let reordered: Value = serde_json::from_str(parsed["profile_json"].as_str().unwrap()).unwrap();
    let text = serde_json::to_string(&reordered).unwrap();
    assert_ne!(text, parsed["profile_json"].as_str().unwrap());
    let mut forged_output = parsed.clone();
    forged_output["profile_json"] = json!(text);
    forged_output["profile_digest"] = json!(sha(text.as_bytes()));
    assert!(refused(&serde_json::to_vec(&forged_output).unwrap()));
    // A receipt that names another candidate (replay of an older probe).
    assert!(refused(&output(
        &expected,
        |r| r["candidate_profile_digest"] = json!("0".repeat(64))
    )));
    // Any unmet kernel control, bad nonce, wrong version.
    for (field, value) in [
        ("memory_bytes", json!(1u64)),
        ("seccomp", json!(false)),
        ("proxy_destination_denied", json!(false)),
        ("nonce", json!("not-a-uuid")),
        ("official_cli_version", json!("2.1.321")),
    ] {
        assert!(refused(&output(&expected, |r| r[field] = value)), "{field}");
    }
    // Digest, extra fields, oversize, garbage, prior acceptance.
    let mut digest = parsed.clone();
    digest["profile_digest"] = json!("0".repeat(64));
    assert!(refused(&serde_json::to_vec(&digest).unwrap()));
    let mut extra = parsed.clone();
    extra["ready"] = json!(true);
    assert!(refused(&serde_json::to_vec(&extra).unwrap()));
    assert!(refused(&vec![b' '; evidence::OUTPUT_CAP + 1]));
    assert!(refused(b"not json"));
    let mut accepted = expected.clone();
    accepted.isolation_acceptance_sha256 = A.into();
    assert!(evidence::verify_output(&accepted, &good).is_err());
}

#[test]
fn stored_evidence_cannot_be_mixed_or_edited() {
    let good = evidence_for(&runtime(), &storage());
    assert!(good.matches(&runtime(), &storage(), "5.15.0-test"));
    assert!(!good.matches(&runtime(), &storage(), "6.8.0"));
    let mut other = runtime();
    other.helper = pin("/usr/local/bin/moosuri", '6');
    assert!(!good.matches(&other, &storage(), "5.15.0-test"));
    let mut image = good.clone();
    image.profile_json = image.profile_json.replace("ubuntu@", "evil@");
    assert!(evidence::validate_stored(&image).is_err());
    let mut swapped = good.clone();
    swapped.receipt_json = swapped.receipt_json.replace("true", "false");
    assert!(evidence::validate_stored(&swapped).is_err());
    let mut digest = good.clone();
    digest.profile_digest = "0".repeat(64);
    assert!(evidence::validate_stored(&digest).is_err());
    let mut candidate = good;
    candidate.candidate_sha256 = "0".repeat(64);
    assert!(evidence::validate_stored(&candidate).is_err());
}

// ---- worker authority -----------------------------------------------------

fn attempt() -> Attempt {
    let mut a = record(Phase::Reserved).attempt;
    a.environment_id = Uuid::from_u128(0x4000_0000_0000_4000_8000_0000_0000_00aa);
    a
}

#[test]
fn worker_config_has_exactly_the_attempts_authority() {
    let a = attempt();
    let bytes = files::render_worker(&a, &owner(), 901, &"7".repeat(64)).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "adapter",
            "connection_revision",
            "effort",
            "model",
            "models",
            "owner",
            "socket_directory",
            "version"
        ]
    );
    assert_eq!(v["version"], 1);
    assert_eq!(v["socket_directory"], "/run/moosuri-claude-worker-901");
    assert_eq!(v["connection_revision"], a.request_id.to_string());
    assert_eq!(v["model"], "claude-opus-4-7");
    assert_eq!(v["effort"], "xhigh");
    assert_eq!(v["models"], json!([]));
    assert_eq!(v["owner"], serde_json::to_value(owner()).unwrap());
    assert_eq!(v["adapter"]["type"], "claude_code");
    let s = &v["adapter"]["settings"];
    let dir = files::dir(&a.environment_id);
    assert_eq!(
        s["profile_file"],
        dir.join("profile.json").to_str().unwrap()
    );
    assert_eq!(
        s["expected_email_file"],
        dir.join("owner-email").to_str().unwrap()
    );
    assert_eq!(s["profile_digest"], "7".repeat(64));
    assert_eq!(s["environment_id"], a.environment_id.to_string());
    assert_eq!(s.as_object().unwrap().len(), 4);
    // The login environment is never the request/revision or a reserved v8 id.
    assert_ne!(s["environment_id"], v["connection_revision"]);
    assert_eq!(a.environment_id.get_version_num(), 4);
    // The email never appears in worker config: only its protected file path.
    assert!(!String::from_utf8(bytes).unwrap().contains("example.com"));
    // No uid 0, no malformed digest, no reserved environment.
    assert!(files::render_worker(&a, &owner(), 0, &"7".repeat(64)).is_err());
    assert!(files::render_worker(&a, &owner(), 901, "short").is_err());
    let mut reserved = a;
    reserved.environment_id = Uuid::parse_str("00000000-0000-8000-8000-000000000001").unwrap();
    assert!(files::render_worker(&reserved, &owner(), 901, &"7".repeat(64)).is_err());
}

#[test]
fn worker_config_without_effort_serializes_null() {
    let mut a = attempt();
    a.effort = None;
    let v: Value =
        serde_json::from_slice(&files::render_worker(&a, &owner(), 901, &"7".repeat(64)).unwrap())
            .unwrap();
    assert!(v["effort"].is_null());
}

#[test]
fn unit_is_exact_and_rejects_unsafe_inputs() {
    let config = files::path(&attempt().environment_id, "worker.json");
    let text = unit::render(
        901,
        902,
        &storage().filesystem_unit,
        "/usr/local/bin/moosuri".as_ref(),
        &config,
    )
    .unwrap();
    for line in [
        "User=901\n",
        "Group=902\n",
        "SupplementaryGroups=\n",
        "RuntimeDirectory=moosuri-claude-worker-901\n",
        "RuntimeDirectoryMode=0700\n",
        "NoNewPrivileges=yes\n",
        "Requires=user@901.service var-lib-moosuri\\x2dclaude\\x2dfs-mounts-u.mount\n",
        "After=user@901.service var-lib-moosuri\\x2dclaude\\x2dfs-mounts-u.mount\n",
    ] {
        assert!(text.contains(line), "{line}");
    }
    assert_eq!(text.matches("ExecStart=").count(), 1);
    assert!(text.contains(&format!(
        "ExecStart=/usr/local/bin/moosuri engine-login-worker --config {}\n",
        config.display()
    )));
    assert!(!text.contains("sh -c"));
    assert!(text.contains("Environment=PATH=/usr/sbin:/usr/bin:/sbin:/bin\n"));
    assert!(text.contains("MemoryMax=268435456\n") && text.contains("TasksMax=64\n"));
    let fs = storage().filesystem_unit;
    for (helper, unit) in [
        ("/usr/local/bin/moo suri", fs.as_str()),
        ("/usr/local/bin/m\nExecStart=/bin/sh", fs.as_str()),
        ("relative/moosuri", fs.as_str()),
        ("/usr/local/bin/moosuri", "x.service"),
        ("/usr/local/bin/moosuri", "a b.mount"),
    ] {
        assert!(
            unit::render(901, 902, unit, helper.as_ref(), &config).is_err(),
            "{helper} {unit}"
        );
    }
    assert!(unit::render(0, 902, &fs, "/a".as_ref(), &config).is_err());
    assert!(unit::render(901, 0, &fs, "/a".as_ref(), &config).is_err());
}

#[test]
fn endpoint_matches_the_unit_runtime_directory_and_fixed_commands_are_argv() {
    let socket = format!("{}/engine.sock", unit::socket_dir(901));
    let endpoint = Endpoint {
        socket: socket.into(),
        worker_uid: 901,
    };
    endpoint.validate().unwrap();
    let a = unit::as_worker("msc-x", &["/bin/true".to_owned()]);
    assert_eq!(
        a[..6],
        [
            "-u",
            "msc-x",
            "--",
            "env",
            "-i",
            "PATH=/usr/sbin:/usr/bin:/sbin:/bin"
        ]
    );
    assert_eq!(a.last().unwrap(), "/bin/true");
}

#[test]
fn storage_must_be_the_dedicated_bounded_root() {
    assert!(storage().validate().is_ok());
    let mut s = storage();
    s.auth_root = "/var/lib/moosuri-claude-fs/mounts/u/other".into();
    assert!(s.validate().is_err());
    let mut s = storage();
    s.scratch_root = "/var/lib/moosuri-claude-fs/mounts/v/scratch".into();
    assert!(s.validate().is_err());
    let mut s = storage();
    s.capacity_bytes += 1;
    assert!(s.validate().is_err());
    let mut s = storage();
    s.filesystem_unit = "x\ny.mount".into();
    assert!(s.validate().is_err());
}

#[test]
fn restart_publication_requires_exact_record_and_owner_lease() {
    let t = tempfile::tempdir().unwrap();
    let layout = Layout::under(t.path());
    fs::create_dir_all(layout.at("/var/lib")).unwrap();
    let store = Store::new(&layout);
    store.ensure().unwrap();
    let rec = record(Phase::WorkerReady);
    let id = rec.attempt.request_id;
    store.save(&rec).unwrap();
    let lease = store.lock(&rec.owner).unwrap();
    assert!(published_in(&layout, rec.owner, id).is_err());
    drop(lease);
    assert!(published_in(&layout, rec.owner, Uuid::new_v4()).is_err());
    let mut foreign = rec.owner;
    foreign.generation += 1;
    assert!(published_in(&layout, foreign, id).is_err());
    assert_eq!(
        store.load(&rec.owner).unwrap().unwrap().attempt.phase,
        Phase::WorkerReady
    );
    published_in(&layout, rec.owner, id).unwrap();
    published_in(&layout, rec.owner, id).unwrap();
    assert_eq!(
        store.load(&rec.owner).unwrap().unwrap().attempt.phase,
        Phase::Active
    );
}

#[test]
fn begin_retirement_authorizes_cleanup_only_for_exact_live_records() {
    let t = tempfile::tempdir().unwrap();
    let layout = Layout::under(t.path());
    fs::create_dir_all(layout.at("/var/lib")).unwrap();
    let store = Store::new(&layout);
    store.ensure().unwrap();
    let o = owner();
    let id = request().request_id;
    // Missing record.
    assert!(begin_retirement_in(&layout, o, id).is_err());
    assert!(store.load(&o).unwrap().is_none());

    for from in [Phase::Active, Phase::WorkerReady] {
        let rec = record(from);
        store.save(&rec).unwrap();
        // Busy lease preserves state.
        let lease = store.lock(&o).unwrap();
        assert!(begin_retirement_in(&layout, o, id).is_err());
        drop(lease);
        assert_eq!(store.load(&o).unwrap().unwrap(), rec);
        // Wrong request and wrong full owner preserve state.
        assert!(begin_retirement_in(&layout, o, Uuid::new_v4()).is_err());
        let mut foreign = o;
        foreign.generation += 1;
        assert!(begin_retirement_in(&layout, foreign, id).is_err());
        assert_eq!(store.load(&o).unwrap().unwrap(), rec);
        // Exact transition changes only the phase.
        begin_retirement_in(&layout, o, id).unwrap();
        let mut expected = rec.clone();
        expected.attempt.phase = Phase::Cleanup;
        assert_eq!(store.load(&o).unwrap().unwrap(), expected);
        // Replay is idempotent.
        begin_retirement_in(&layout, o, id).unwrap();
        assert_eq!(store.load(&o).unwrap().unwrap(), expected);
    }

    for phase in [Phase::RemovingFiles, Phase::Retired] {
        let rec = record(phase);
        store.save(&rec).unwrap();
        begin_retirement_in(&layout, o, id).unwrap();
        assert_eq!(store.load(&o).unwrap().unwrap(), rec);
    }

    for phase in [
        Phase::Reserved,
        Phase::Prepared,
        Phase::LoginIssued,
        Phase::Activating,
    ] {
        let rec = record(phase);
        store.save(&rec).unwrap();
        assert!(begin_retirement_in(&layout, o, id).is_err());
        assert_eq!(store.load(&o).unwrap().unwrap(), rec);
    }
}

#[test]
fn unseen_or_already_retired_request_cleanup_is_non_destructive() {
    struct Never;
    impl Exec for Never {
        fn run(&self, _: &str, _: &[String], _: host::Input, _: Duration) -> R<host::Out> {
            panic!("retirement must not execute a command in this case")
        }
    }
    let t = tempfile::tempdir().unwrap();
    let layout = Layout::under(t.path());
    fs::create_dir_all(layout.at("/var/lib")).unwrap();
    let store = Store::new(&layout);
    store.ensure().unwrap();
    let rec = record(Phase::Retired);
    let id = rec.attempt.request_id;
    store.save(&rec).unwrap();
    retire_in(&layout, &Never, rec.owner, Uuid::new_v4()).unwrap();
    retire_in(&layout, &Never, rec.owner, id).unwrap();
    assert_eq!(store.load(&rec.owner).unwrap().unwrap(), rec);
    let active = record(Phase::Active);
    store.save(&active).unwrap();
    retire_in(&layout, &Never, active.owner, Uuid::new_v4()).unwrap();
    assert!(retire_in(&layout, &Never, active.owner, id).is_err());
    assert_eq!(store.load(&active.owner).unwrap().unwrap(), active);
}
