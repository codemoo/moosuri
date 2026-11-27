//! Root-owned, credential-FREE host preparation for one owner's Claude
//! subscription runtime. It downloads and verifies pinned artifacts, allocates a
//! dedicated nonroot service identity with subordinate ranges, starts its
//! rootless systemd user service and verifies that engine's resource contract
//! and the immutable base image.
//!
//! It deliberately does NOT authenticate any account, generate or accept an
//! isolation Profile, bind a runtime model, mark inference ready or claim that
//! product setup completed. The typed descriptor it returns is the input of the
//! next (native isolation acceptance) stage and carries no URL, code or auth data.
mod artifact;
pub(crate) mod connection;
mod home;
mod identity;
mod prereq;
mod receipt;
mod request;
mod service;
#[cfg(test)]
mod tests;

use crate::engine_login::Owner;
use crate::installation::kiri::host::{self, Exec, Layout, Result, System};
use clap::Args;
use fs2::FileExt;
use prereq::PinnedFile;
use receipt::{Phase, Receipt};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

pub(crate) use request::Request;

/// Root-owned immutable artifacts.
const ARTIFACTS: &str = "/opt/moosuri-claude";
/// Root-only receipts, lock and download scratch; never a worker-writable path.
const CONTROL: &str = "/var/lib/moosuri-claude-control";
const SHELL: &str = "/usr/sbin/nologin";
/// Member allocation starts at 20000; the service identity must stay below it.
const MAX_SERVICE_UID: u32 = 20_000;

struct Pin {
    url: &'static str,
    sha256: &'static str,
    /// Exact size when known; otherwise only `max` bounds the download.
    size: Option<u64>,
    max: u64,
}
struct Pins {
    cli: Pin,
    cli_version: &'static str,
    wrapper: Pin,
    wrapper_tag: &'static str,
    image: &'static str,
}
const PINS: Pins = Pins {
    cli: Pin {
        url: "https://downloads.claude.ai/claude-code-releases/2.1.322/linux-x64/claude",
        sha256: "bfb3703ad5380242328459bc62ada4bde8ae7ba57c6e1eeadb9c00e08874c478",
        size: Some(341_914_128),
        max: 341_914_128,
    },
    cli_version: "2.1.322",
    wrapper: Pin {
        url: "https://raw.githubusercontent.com/moby/moby/docker-v29.1.3/contrib/dockerd-rootless.sh",
        sha256: "8eb00d61b5e78ffc0dcd628f056389b5859cc61d6fe3ce17b4a3a4b08143c8a5",
        size: None,
        max: 32_768,
    },
    wrapper_tag: "docker-v29.1.3",
    image: "ubuntu@sha256:562111b88caa79c71391c0c4d3c2114a7d5c35915c20561fd893cb1e07929e6b",
};

#[derive(Args, Clone, Debug)]
pub(crate) struct Options {
    /// Private root-owned JSON request (version, request_id, owner).
    #[arg(long)]
    pub request: PathBuf,
}

/// Preparation only. Never acceptance, authentication or inference readiness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PreparationState {
    HostPreparedNotAccepted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ServiceIdentity {
    pub username: String,
    pub uid: u32,
    pub gid: u32,
    pub home: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PinnedCli {
    pub path: PathBuf,
    pub sha256: String,
    pub version: String,
}

/// Typed output for the next stage. Facts observed during this run only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreparedRuntime {
    pub version: u32,
    pub request_id: Uuid,
    pub owner: Owner,
    pub service: ServiceIdentity,
    pub rootless_socket: PathBuf,
    pub engine: PinnedFile,
    pub official_cli: PinnedCli,
    pub rootless_wrapper: PinnedFile,
    pub image: String,
    pub helper: PinnedFile,
    pub state: PreparationState,
}

#[cfg(target_os = "linux")]
fn require_linux_root() -> Result<()> {
    if host::euid() != 0 {
        return Err("claude_runtime_root_required".into());
    }
    if !cfg!(target_arch = "x86_64") {
        return Err("claude_runtime_x86_64_required".into());
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn require_linux_root() -> Result<()> {
    Err("claude_runtime_linux_required".into())
}

pub(crate) fn run(options: &Options) -> std::result::Result<(), String> {
    require_linux_root()?;
    let layout = Layout::system();
    let request = Request::load(&layout, &options.request)?;
    let exec = System {
        docker_config: PathBuf::from(format!("{ARTIFACTS}/docker-config-empty")),
    };
    // The helper is this very canonical executable, never a caller-chosen copy.
    let helper = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|_| "claude_runtime_helper_unknown")?;
    let prepared = prepare_with(&layout, &exec, request, &helper)?;
    let storage = crate::kiri_storage::claude::prepare(
        prepared.request_id,
        prepared.owner,
        prepared.service.uid,
        prepared.service.gid,
    )?;
    println!(
        "{}",
        serde_json::json!({"runtime": prepared, "storage": storage})
    );
    Ok(())
}

/// Production pins; `helper` is the canonical current executable.
pub(crate) fn prepare_with(
    layout: &Layout,
    exec: &dyn Exec,
    request: Request,
    helper: &Path,
) -> Result<PreparedRuntime> {
    prepare_pinned(layout, exec, request, helper, &PINS, &|request, svc| {
        let home = crate::kiri_storage::claude::prepare_home(
            request.request_id,
            request.owner,
            svc.uid,
            svc.gid,
        )?;
        if home != svc.home {
            return Err("claude_runtime_home_claim_changed".into());
        }
        Ok(())
    })
}

/// Create the fixed root-owned directories and take the exclusive setup lock.
fn open_control(layout: &Layout) -> Result<File> {
    for (dir, mode) in [(ARTIFACTS, 0o755), (CONTROL, 0o700)] {
        layout.ensure_dir(&layout.at(dir), mode)?;
    }
    for dir in ["receipts", "downloads"] {
        layout.ensure_dir(&layout.at(&format!("{CONTROL}/{dir}")), 0o700)?;
    }
    let path = layout.at(&format!("{CONTROL}/prepare.lock"));
    layout.safe_ancestors(&path)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&path)
        .map_err(|_| "claude_runtime_lock_unavailable")?;
    let m = lock
        .metadata()
        .map_err(|_| "claude_runtime_lock_unavailable")?;
    if !m.is_file() || m.nlink() != 1 || m.uid() != host::euid() || m.mode() & 0o077 != 0 {
        return Err("claude_runtime_lock_foreign".into());
    }
    lock.try_lock_exclusive()
        .map_err(|_| "claude_runtime_busy")?;
    Ok(lock)
}

fn ensure_artifact_dirs(layout: &Layout, pins: &Pins) -> Result<(String, String)> {
    let cli_dir = format!("{ARTIFACTS}/claude");
    let cli_versioned = format!("{cli_dir}/{}", pins.cli_version);
    let wrapper_dir = format!("{ARTIFACTS}/rootless");
    let wrapper_tagged = format!("{wrapper_dir}/{}", pins.wrapper_tag);
    let config = format!("{ARTIFACTS}/docker-config-empty");
    for dir in [
        &cli_dir,
        &cli_versioned,
        &wrapper_dir,
        &wrapper_tagged,
        &config,
    ] {
        layout.ensure_dir(&layout.at(dir), 0o755)?;
    }
    // Isolated DOCKER_CONFIG must stay empty: it can never carry registry auth.
    let mut entries =
        fs::read_dir(layout.at(&config)).map_err(|_| "claude_runtime_artifact_conflict")?;
    if entries.next().is_some() {
        return Err("claude_runtime_artifact_conflict".into());
    }
    Ok((
        format!("{cli_versioned}/claude"),
        format!("{wrapper_tagged}/dockerd-rootless.sh"),
    ))
}

fn prepare_pinned(
    layout: &Layout,
    exec: &dyn Exec,
    request: Request,
    helper: &Path,
    pins: &Pins,
    prepare_home: &dyn Fn(&Request, &ServiceIdentity) -> Result<()>,
) -> Result<PreparedRuntime> {
    request.validate()?;
    if !pins.image.contains("@sha256:") {
        return Err("claude_runtime_image_not_immutable".into());
    }
    // 1. Read-only: every prerequisite and trusted binary, before any write.
    let tools = prereq::check(layout, exec)?;
    let helper_file = prereq::trusted_file(layout, helper, prereq::HELPER_CAP)?;
    if helper_file.path != helper {
        return Err("claude_runtime_helper_not_canonical".into());
    }

    // 2. Exclusive root-only control state; bind request/owner exactly.
    let _lock = open_control(layout)?;
    artifact::sweep(layout);
    let store = receipt::Store::new(layout);
    let existing = store.load(&request)?;
    if existing.is_none() {
        store.owner_in_use(&request)?;
        let name = receipt::username(&request.request_id);
        if identity::lookup_user(exec, &name)?.is_some()
            || identity::lookup_group(exec, &name)?.is_some()
        {
            return Err("claude_runtime_account_exists_unmanaged".into());
        }
    }

    // 3. Pinned artifacts (a failed download leaves no account behind).
    let (cli_path, wrapper_path) = ensure_artifact_dirs(layout, pins)?;
    let cli = artifact::ensure(
        layout,
        exec,
        &pins.cli,
        &cli_path,
        Duration::from_secs(1800),
    )?;
    let wrapper = artifact::ensure(
        layout,
        exec,
        &pins.wrapper,
        &wrapper_path,
        Duration::from_secs(120),
    )?;

    // 4. Identity. The receipt precedes useradd so a crash is recoverable
    //    only into the exact marked account.
    let (mut receipt, bytes) = match existing {
        Some(found) => found,
        None => {
            let fresh = Receipt::reserved(&request);
            let bytes = store.save(&fresh, None)?;
            (fresh, bytes)
        }
    };
    if identity::lookup_user(exec, &receipt.username)?.is_none() {
        if receipt.identity.is_some() {
            return Err("claude_runtime_identity_missing".into());
        }
        identity::create(exec, &receipt, tools.system_subids)?;
    }
    let ident = identity::verify(layout, exec, &receipt)?;
    if receipt.identity.is_none() {
        receipt.phase = Phase::Identified;
        receipt.identity = Some(ident.clone());
        store.save(&receipt, Some(&bytes))?;
    }
    let svc = ServiceIdentity {
        username: receipt.username.clone(),
        uid: ident.uid,
        gid: ident.gid,
        home: PathBuf::from(&receipt.home),
    };

    // 5. The whole worker home (including engine images) is capacity bounded.
    prepare_home(&request, &svc)?;
    // Rootless service, then its verified resource contract and image.
    service::ensure_unit(layout, &svc, &wrapper.path)?;
    service::verify_artifact_access(
        exec,
        &svc,
        &[
            &helper_file.path,
            &cli.path,
            &wrapper.path,
            &tools.engine.path,
        ],
    )?;
    service::install_dropin(layout, exec, svc.uid, &svc.home)?;
    service::start_manager(exec, &tools, &svc)?;
    service::start_service(exec, &tools, &svc)?;
    service::verify_engine(layout, exec, &tools, &svc)?;
    service::ensure_image(exec, &tools, &svc, pins.image)?;

    Ok(PreparedRuntime {
        version: 1,
        request_id: request.request_id,
        owner: request.owner,
        rootless_socket: PathBuf::from(service::socket_path(svc.uid)),
        service: svc,
        engine: tools.engine,
        official_cli: PinnedCli {
            path: cli.path,
            sha256: cli.sha256,
            version: pins.cli_version.to_owned(),
        },
        rootless_wrapper: wrapper,
        image: pins.image.to_owned(),
        helper: helper_file,
        state: PreparationState::HostPreparedNotAccepted,
    })
}
