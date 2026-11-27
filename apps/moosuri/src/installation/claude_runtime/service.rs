//! Rootless systemd user service for the dedicated identity: worker-owned unit
//! (never silently trusted again), target-only `user@UID` delegation drop-in,
//! linger, resource-contract verification and the immutable base image.
//! Nothing here authenticates an account or accepts an isolation profile.
use super::home::{self, Dir};
use super::prereq::{self, Tools};
use super::{ARTIFACTS, ServiceIdentity};
use crate::installation::kiri::host::{Exec, Input, Layout, Out, Result, args};
use serde_json::Value;
use std::{collections::BTreeMap, fs, os::unix::fs::FileTypeExt, path::Path, time::Duration};

pub(super) const UNIT: &str = "moosuri-claude-docker.service";
const UNIT_CAP: u64 = 4096;

pub(super) fn socket_path(uid: u32) -> String {
    format!("/run/user/{uid}/docker.sock")
}

pub(super) fn unit_text(wrapper: &Path) -> String {
    format!(
        "[Service]\n\
         Type=notify\n\
         NotifyAccess=all\n\
         ExecStart={} --host=unix://%t/docker.sock --exec-opt native.cgroupdriver=systemd --log-driver=none --bridge=none --iptables=false --ip-masq=false\n\
         Restart=on-failure\n\
         TimeoutStartSec=90\n\
         TimeoutStopSec=30\n\
         KillMode=mixed\n\
         Delegate=yes\n\
         UMask=0077\n\
         Environment=PATH=/usr/sbin:/usr/bin:/sbin:/bin\n\
         Environment=TMPDIR=%h/.cache\n\
         Environment=DOCKERD_ROOTLESS_ROOTLESSKIT_NET=slirp4netns\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n",
        wrapper.display()
    )
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn run_ok(
    exec: &dyn Exec,
    program: &str,
    a: &[String],
    limit: u64,
    reason: &str,
) -> Result<Vec<u8>> {
    exec.run(program, a, Input::None, secs(limit))?.ok(reason)
}

/// `runuser -u <user> -- env <fixed environment> <tail…>`
fn as_service(svc: &ServiceIdentity, extra_env: &[String], tail: &[String]) -> Vec<String> {
    let mut a = args(&["-u", svc.username.as_str(), "--", "env"]);
    a.push(format!("XDG_RUNTIME_DIR=/run/user/{}", svc.uid));
    a.push(format!(
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{}/bus",
        svc.uid
    ));
    a.extend_from_slice(extra_env);
    a.extend_from_slice(tail);
    a
}

/// Kernel checks as the actual worker catch root-only executable ancestors.
pub(super) fn verify_artifact_access(
    exec: &dyn Exec,
    svc: &ServiceIdentity,
    paths: &[&Path],
) -> Result<()> {
    for path in paths {
        let path = path.to_str().ok_or("claude_runtime_path_invalid")?;
        let tail = args(&["/usr/bin/test", "-r", path, "-a", "-x", path]);
        exec.run(
            "runuser",
            &as_service(svc, &[], &tail),
            Input::None,
            secs(10),
        )?
        .ok("claude_runtime_worker_artifact_inaccessible")?;
    }
    Ok(())
}

fn user_ctl(
    exec: &dyn Exec,
    tools: &Tools,
    svc: &ServiceIdentity,
    rest: &[&str],
    limit: u64,
) -> Result<Out> {
    let mut tail = vec![tools.systemctl.display().to_string(), "--user".to_owned()];
    tail.extend(rest.iter().map(|s| (*s).to_owned()));
    exec.run(
        "runuser",
        &as_service(svc, &[], &tail),
        Input::None,
        secs(limit),
    )
}

/// Create the worker-owned unit when absent; an existing one must be exactly
/// the rendered bytes. A changed unit is a conflict, never repaired in place.
pub(super) fn ensure_unit(layout: &Layout, svc: &ServiceIdentity, wrapper: &Path) -> Result<()> {
    let home = layout.at(svc.home.to_str().ok_or("claude_runtime_path_invalid")?);
    layout.safe_ancestors(&home)?;
    let root = Dir::open_home(&home, svc.uid, svc.gid)?;
    root.child(".cache")?;
    let dir = root.child(".config")?.child("systemd")?.child("user")?;
    let want = unit_text(wrapper);
    let matches = |found: Option<Vec<u8>>| found.is_some_and(|b| b.as_slice() == want.as_bytes());
    match dir.read_file(UNIT, UNIT_CAP)? {
        Some(found) if found.as_slice() == want.as_bytes() => Ok(()),
        Some(_) => Err("claude_runtime_user_unit_conflict".into()),
        None if dir.create_file(UNIT, want.as_bytes())? => Ok(()),
        None if matches(dir.read_file(UNIT, UNIT_CAP)?) => Ok(()),
        None => Err("claude_runtime_user_unit_conflict".into()),
    }
}

/// Only the target `user@UID.service` receives delegation; the global
/// `user@.service` template is never edited.
pub(super) fn install_dropin(
    layout: &Layout,
    exec: &dyn Exec,
    uid: u32,
    home: &Path,
) -> Result<()> {
    let dir = layout.at(&format!("/etc/systemd/system/user@{uid}.service.d"));
    layout.ensure_dir(&dir, 0o755)?;
    layout.write_file(
        &dir.join("moosuri-claude.conf"),
        format!("[Unit]\nRequiresMountsFor={}\n[Service]\nDelegate=cpu cpuset io memory pids\nMemoryMax=1610612736\nTasksMax=256\nCPUQuota=200%\n", home.display()).as_bytes(),
        0o644,
        &[],
    )?;
    run_ok(
        exec,
        "systemctl",
        &args(&["daemon-reload"]),
        60,
        "claude_runtime_systemd_reload_failed",
    )
    .map(|_| ())
}

pub(super) fn start_manager(exec: &dyn Exec, tools: &Tools, svc: &ServiceIdentity) -> Result<()> {
    run_ok(
        exec,
        "loginctl",
        &args(&["enable-linger", svc.username.as_str()]),
        30,
        "claude_runtime_linger_failed",
    )?;
    let manager = format!("user@{}.service", svc.uid);
    run_ok(
        exec,
        "systemctl",
        &args(&["start", manager.as_str()]),
        120,
        "claude_runtime_user_manager_start_failed",
    )?;
    let mut reachable = false;
    for attempt in 0..20 {
        if user_ctl(exec, tools, svc, &["show-environment"], 15)?.code == 0 {
            reachable = true;
            break;
        }
        if attempt < 19 {
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    if !reachable {
        return Err("claude_runtime_user_manager_unavailable".into());
    }
    let linger = run_ok(
        exec,
        "loginctl",
        &args(&["show-user", svc.username.as_str(), "--property=Linger"]),
        30,
        "claude_runtime_linger_failed",
    )?;
    if !String::from_utf8_lossy(&linger).contains("Linger=yes") {
        return Err("claude_runtime_linger_failed".into());
    }
    Ok(())
}

fn unit_state(
    exec: &dyn Exec,
    tools: &Tools,
    svc: &ServiceIdentity,
) -> Result<BTreeMap<String, String>> {
    let out = user_ctl(
        exec,
        tools,
        svc,
        &[
            "show",
            UNIT,
            "--property=FragmentPath",
            "--property=DropInPaths",
            "--property=ActiveState",
        ],
        30,
    )?
    .ok("claude_runtime_service_state_unavailable")?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect())
}

pub(super) fn start_service(exec: &dyn Exec, tools: &Tools, svc: &ServiceIdentity) -> Result<()> {
    let fail = |reason: &str| -> Result<()> { Err(reason.to_owned()) };
    if user_ctl(exec, tools, svc, &["daemon-reload"], 30)?.code != 0 {
        return fail("claude_runtime_user_reload_failed");
    }
    // The manager must have loaded exactly the verified home unit and no
    // worker-added drop-in that could alter its ExecStart.
    let loaded = unit_state(exec, tools, svc)?;
    let expected = format!("{}/.config/systemd/user/{UNIT}", svc.home.display());
    if loaded.get("FragmentPath") != Some(&expected)
        || loaded.get("DropInPaths").is_some_and(|v| !v.is_empty())
    {
        return fail("claude_runtime_user_unit_conflict");
    }
    if user_ctl(exec, tools, svc, &["enable", UNIT], 30)?.code != 0 {
        return fail("claude_runtime_service_enable_failed");
    }
    if user_ctl(exec, tools, svc, &["start", UNIT], 120)?.code != 0 {
        return fail("claude_runtime_service_start_failed");
    }
    if unit_state(exec, tools, svc)?
        .get("ActiveState")
        .map(String::as_str)
        != Some("active")
    {
        return fail("claude_runtime_service_not_active");
    }
    Ok(())
}

/// Explicit `--host`, and an empty isolated DOCKER_CONFIG, under the target uid.
fn docker(
    exec: &dyn Exec,
    tools: &Tools,
    svc: &ServiceIdentity,
    rest: &[&str],
    limit: u64,
) -> Result<Out> {
    let mut tail = vec![
        tools.engine.path.display().to_string(),
        "--host".to_owned(),
        format!("unix://{}", socket_path(svc.uid)),
    ];
    tail.extend(rest.iter().map(|s| (*s).to_owned()));
    let config = format!("DOCKER_CONFIG={ARTIFACTS}/docker-config-empty");
    exec.run(
        "runuser",
        &as_service(svc, &[config], &tail),
        Input::None,
        secs(limit),
    )
}

fn resource_contract(info: &Value) -> bool {
    let rootless = info["SecurityOptions"].as_array().is_some_and(|a| {
        a.iter()
            .filter_map(Value::as_str)
            .any(|s| s == "name=rootless" || s.starts_with("name=rootless,"))
    });
    rootless
        && info["CgroupDriver"] == "systemd"
        && (info["CgroupVersion"] == "2" || info["CgroupVersion"] == 2)
        && ["MemoryLimit", "CpuCfsPeriod", "CpuCfsQuota", "PidsLimit"]
            .iter()
            .all(|k| info[*k] == true)
}

/// Canonical socket owned by the target uid, rootless, systemd cgroup v2 and
/// the memory/CPU/pids limits actually reported by the running engine.
pub(super) fn verify_engine(
    layout: &Layout,
    exec: &dyn Exec,
    tools: &Tools,
    svc: &ServiceIdentity,
) -> Result<()> {
    let socket = socket_path(svc.uid);
    let actual = layout.at(&socket);
    let metadata = fs::symlink_metadata(&actual).map_err(|_| "claude_runtime_socket_missing")?;
    let (uid, _) = home::owner_ids(svc.uid, svc.gid);
    if !metadata.file_type().is_socket()
        || std::os::unix::fs::MetadataExt::uid(&metadata) != uid
        || prereq::logical_canonical(layout, &actual)?.to_str() != Some(socket.as_str())
    {
        return Err("claude_runtime_socket_untrusted".into());
    }
    let out = docker(exec, tools, svc, &["info", "--format", "{{json .}}"], 60)?
        .ok("claude_runtime_engine_unavailable")?;
    let info: Value =
        serde_json::from_slice(&out).map_err(|_| "claude_runtime_engine_info_invalid")?;
    if !resource_contract(&info) {
        return Err("claude_runtime_resource_contract_unsupported".into());
    }
    Ok(())
}

fn image_present(
    exec: &dyn Exec,
    tools: &Tools,
    svc: &ServiceIdentity,
    image: &str,
) -> Result<bool> {
    let out = docker(
        exec,
        tools,
        svc,
        &[
            "image",
            "inspect",
            "--format",
            "{{json .RepoDigests}}",
            image,
        ],
        60,
    )?;
    if out.code != 0 {
        return Ok(false);
    }
    let digests: Vec<String> = serde_json::from_slice(&out.stdout)
        .map_err(|_| "claude_runtime_image_verification_failed")?;
    let qualified = format!("docker.io/library/{image}");
    Ok(digests.iter().any(|d| d == image || *d == qualified))
}

/// Pull only the exact immutable digest reference; never a mutable tag.
pub(super) fn ensure_image(
    exec: &dyn Exec,
    tools: &Tools,
    svc: &ServiceIdentity,
    image: &str,
) -> Result<()> {
    if !image_present(exec, tools, svc, image)? {
        docker(exec, tools, svc, &["pull", image], 900)?.ok("claude_runtime_image_pull_failed")?;
        if !image_present(exec, tools, svc, image)? {
            return Err("claude_runtime_image_verification_failed".into());
        }
    }
    Ok(())
}
