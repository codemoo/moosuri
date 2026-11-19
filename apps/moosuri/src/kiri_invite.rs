//! Experimental Linux installer operation for new, preallocated personal runtimes.
//! Passwords cross only the operator's terminal/private stdin and the dedicated
//! UID child's stdin. They are never arguments, operation records or diagnostics.
use crate::{kiri_lifecycle, kiri_provision};
use clap::Args;
use fs2::FileExt;
use moosuri_store::runtime_registry::{self, Allocation, InviteState};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{IsTerminal, Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use uuid::Uuid;
use zeroize::Zeroizing;
const CAP: u64 = 64 * 1024;
#[derive(Args)]
#[group(id = "KiriInviteOptions")]
pub struct Options {
    /// Stable invitation request ID; retain it across every interrupted retry.
    #[arg(long)]
    pub request_id: Uuid,
    #[arg(long)]
    pub name: String,
    /// Existing root-owned0700 metadata-only installer operation directory.
    #[arg(long)]
    pub operations_root: PathBuf,
    /// Read one bounded newline-terminated temporary password only when needed.
    #[arg(long)]
    pub password_stdin: bool,
    #[command(flatten)]
    pub profile: kiri_provision::Options,
}
fn private_root(root: &Path) -> Result<(), String> {
    if !root.is_absolute() || fs::canonicalize(root).ok().as_deref() != Some(root) {
        return Err("invite_private_absolute_root_required".into());
    }
    let m = fs::symlink_metadata(root).map_err(|_| "invite_private_root_unavailable")?;
    if !m.is_dir() || m.uid() != 0 || m.mode() & 0o077 != 0 {
        return Err("invite_private_root_permissions".into());
    }
    for p in root.ancestors().skip(1) {
        let m = fs::symlink_metadata(p).map_err(|_| "invite_ancestor_unavailable")?;
        if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("invite_untrusted_ancestor".into());
        }
    }
    Ok(())
}
fn operation_lock(options: &Options) -> Result<File, String> {
    private_root(&options.operations_root)?;
    let p = options
        .operations_root
        .join(format!("{}.lock", options.request_id));
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(p)
        .map_err(|_| "invite_lock_unavailable")?;
    let m = f.metadata().map_err(|_| "invite_lock_unavailable")?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o077 != 0 || m.nlink() != 1 || m.len() != 0 {
        return Err("invite_untrusted_lock".into());
    }
    f.try_lock_exclusive()
        .map_err(|_| "invite_already_running")?;
    Ok(f)
}
fn profile(options: &Options) -> Value {
    let p = &options.profile;
    let mut profile = json!({"name":options.name.trim(),"email":p.email.trim().to_ascii_lowercase(),"uid":p.uid,"image":p.image,"registry":p.registry_directory,"mounts":p.mounts_root,"ipc":p.ipc_root,"allocations":p.allocations_root,"model":p.model_sockets_root,"google":p.google_sockets_root,"text_preview":p.enable_text_preview,"engine":p.engine_socket,"memory":p.memory_bytes,"cpu":p.nano_cpus,"pids":p.pids,"disk":p.max_volume_bytes});
    if let Some(browser) = &p.browser_sockets_root {
        profile["browser"] = json!(browser);
    }
    if p.enable_local_gws {
        profile["local_gws"] = json!(true);
    }
    profile
}
fn record(options: &Options, a: &Allocation, stage: &str) -> Value {
    json!({"version":1,"request_id":options.request_id,"allocation":a,"profile":profile(options),"stage":stage})
}
fn check_record(options: &Options, a: &Allocation) -> Result<(), String> {
    let path = options
        .operations_root
        .join(format!("{}.json", options.request_id));
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Ok(m)
            if m.is_file()
                && m.uid() == 0
                && m.mode() & 0o077 == 0
                && m.nlink() == 1
                && m.len() <= CAP => {}
        _ => return Err("invite_untrusted_operation_record".into()),
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "invite_operation_unavailable")?;
    let mut bytes = Vec::new();
    file.take(CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "invite_operation_unavailable")?;
    let old: Value = serde_json::from_slice(&bytes).map_err(|_| "invite_operation_corrupt")?;
    let want = record(options, a, "");
    if old["version"] != 1
        || old["request_id"] != want["request_id"]
        || old["allocation"] != want["allocation"]
        || old["profile"] != want["profile"]
    {
        return Err("invite_retry_profile_conflict".into());
    }
    Ok(())
}
fn save(options: &Options, a: &Allocation, stage: &str) -> Result<(), String> {
    let mut temp = tempfile::Builder::new()
        .prefix(&format!("{}.tmp-", options.request_id))
        .tempfile_in(&options.operations_root)
        .map_err(|_| "invite_operation_write_failed")?;
    let bytes = serde_json::to_vec(&record(options, a, stage))
        .map_err(|_| "invite_operation_encode_failed")?;
    temp.write_all(&bytes)
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| "invite_operation_write_failed")?;
    temp.persist(
        options
            .operations_root
            .join(format!("{}.json", options.request_id)),
    )
    .map_err(|_| "invite_operation_publish_failed")?;
    File::open(&options.operations_root)
        .and_then(|f| f.sync_all())
        .map_err(|_| "invite_operation_publish_failed".to_owned())
}
fn state(options: &Options, a: &Allocation) -> Result<InviteState, String> {
    runtime_registry::invitation_state(&options.profile.registry_directory, options.request_id, a)
        .map_err(|_| "invite_allocation_changed".into())
}
fn not_fenced(options: &Options, a: &Allocation) -> Result<InviteState, String> {
    let state = state(options, a)?;
    if state == InviteState::Fenced {
        return Err("invite_fenced".into());
    }
    Ok(state)
}
fn emit(
    options: &Options,
    a: &Allocation,
    stage: &str,
    reason: Option<&str>,
) -> Result<(), String> {
    save(options, a, stage)?;
    let p = &options.profile;
    let mount = p.mounts_root.join(&a.volume_id);
    println!(
        "{}",
        json!({"stage":stage,"ready":stage=="ready","request_id":options.request_id,"allocation":a,"reason":reason,
        "preparation":{"uid":p.uid,"mount":mount,"member_data":mount.join("member-data"),"scratch":mount.join("scratch"),"ipc_source":mount.join("ipc"),"ipc_target":p.ipc_root.join(&a.account_id)},
        "next":"Use the same request ID/profile after preparing the documented capped mounts; provide the password only by terminal or --password-stdin when stage is password_required."})
    );
    Ok(())
}
fn executable() -> Result<PathBuf, String> {
    let path = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|_| "invite_executable_unavailable")?;
    let m = fs::symlink_metadata(&path).map_err(|_| "invite_executable_unavailable")?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o6022 != 0 || m.nlink() != 1 {
        return Err("invite_untrusted_executable".into());
    }
    for p in path.ancestors().skip(1) {
        let m = fs::metadata(p).map_err(|_| "invite_executable_unavailable")?;
        if m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("invite_untrusted_executable_ancestor".into());
        }
    }
    Ok(path)
}
#[cfg(target_os = "linux")]
fn member_credentials(uid: u32, parent: libc::pid_t) -> std::io::Result<()> {
    unsafe {
        if libc::setgroups(0, std::ptr::null()) != 0
            || libc::setresgid(uid, uid, uid) != 0
            || libc::setresuid(uid, uid, uid) != 0
            || libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
            || libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0) != 0
            || libc::getppid() != parent
        {
            return Err(std::io::Error::last_os_error());
        }
        libc::umask(0o077);
        Ok(())
    }
}
#[cfg(not(target_os = "linux"))]
fn member_credentials(_: u32, _: libc::pid_t) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}
fn child(
    options: &Options,
    a: &Allocation,
    prepared: &kiri_provision::PreparedMember,
    initialize: bool,
    password: Option<&str>,
) -> Result<bool, String> {
    let mut command = Command::new(executable()?);
    command
        .args(["personal-volume", "--root"])
        .arg(&prepared.data_directory)
        .args([
            "--account",
            &a.account_id,
            "--tenant",
            &a.tenant_id,
            "--volume",
            &a.volume_id,
            "--generation",
            &a.generation.to_string(),
        ]);
    if initialize {
        command.args([
            "initialize",
            "--name",
            options.name.trim(),
            "--email",
            &options.profile.email,
            "--password-stdin",
        ]);
    } else {
        command.args(["initialization-status", "--email", &options.profile.email]);
    }
    command
        .env_clear()
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .env("TMPDIR", &prepared.scratch_directory)
        .current_dir("/")
        .stdin(if initialize {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(if initialize {
            Stdio::null()
        } else {
            Stdio::piped()
        })
        .stderr(Stdio::null());
    let uid = options.profile.uid;
    let parent = unsafe { libc::getpid() };
    // Only async-signal-safe credential/syscall operations between fork and exec.
    unsafe {
        command.pre_exec(move || member_credentials(uid, parent));
    }
    let mut process = command
        .spawn()
        .map_err(|_| "invite_private_child_unavailable")?;
    if let Some(password) = password {
        let mut input = process
            .stdin
            .take()
            .ok_or("invite_private_pipe_unavailable")?;
        input
            .write_all(password.as_bytes())
            .and_then(|_| input.write_all(b"\n"))
            .map_err(|_| "invite_private_pipe_failed")?;
        drop(input);
    }
    let mut out = Vec::new();
    if let Some(output) = process.stdout.take() {
        output
            .take(1025)
            .read_to_end(&mut out)
            .map_err(|_| "invite_child_status_failed")?;
    }
    let status = process.wait().map_err(|_| "invite_child_wait_failed")?;
    if !status.success() {
        return Err("invite_private_initialization_unavailable".into());
    }
    if initialize {
        return Ok(true);
    }
    let value: Value = serde_json::from_slice(&out).map_err(|_| "invite_invalid_child_status")?;
    value
        .get("initialized")
        .and_then(Value::as_bool)
        .ok_or_else(|| "invite_invalid_child_status".into())
}
fn password(options: &Options) -> Result<Option<Zeroizing<String>>, String> {
    if options.password_stdin {
        let mut value = Zeroizing::new(String::new());
        std::io::stdin()
            .lock()
            .take(259)
            .read_to_string(&mut value)
            .map_err(|_| "invite_password_input_unavailable")?;
        let line = value
            .strip_suffix('\n')
            .filter(|s| !s.contains(['\r', '\n']) && s.len() <= 256)
            .ok_or("invite_expected_one_private_password_line")?;
        if !moosuri_core::account_password_valid(line) {
            return Err("invite_password_does_not_meet_account_policy".into());
        }
        return Ok(Some(Zeroizing::new(line.to_owned())));
    }
    if !std::io::stdin().is_terminal() {
        return Ok(None);
    }
    let secret = Zeroizing::new(
        rpassword::prompt_password("Temporary member password / 임시 비밀번호: ")
            .map_err(|_| "invite_password_input_unavailable")?,
    );
    if !moosuri_core::account_password_valid(&secret) {
        return Err("invite_password_does_not_meet_account_policy".into());
    }
    Ok(Some(secret))
}
async fn ready(prepared: &kiri_provision::PreparedMember, a: &Allocation, uid: u32) -> bool {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        match kiri_lifecycle::health(&prepared.ipc_directory.join("personal.sock"), a, uid).await {
            Ok(()) => return true,
            Err("member_identity_mismatch") => return false,
            Err(_) => {
                if tokio::time::Instant::now() >= deadline {
                    return false;
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn planned_invitation(options: &Options) -> Result<(), String> {
    let root = &options.profile.registry_directory;
    if !root.join("account-runtimes.sqlite").exists() {
        return Ok(());
    }
    if let Some(invitation) = runtime_registry::admin::operator_invitation(root, options.request_id)
        .map_err(|_| "invite_planned_request_conflict")?
    {
        if invitation.name != options.name.trim()
            || invitation.email != options.profile.email.trim().to_ascii_lowercase()
        {
            return Err("invite_planned_identity_conflict".into());
        }
    }
    Ok(())
}

pub async fn run(options: &Options) -> Result<(), String> {
    if !cfg!(target_os = "linux")
        || !options.profile.experimental
        || unsafe { libc::geteuid() } != 0
    {
        return Err("invite_explicit_root_linux_only".into());
    }
    if options.request_id.is_nil()
        || options.name.trim().is_empty()
        || options.name.trim().len() > 200
        || options.name.chars().any(char::is_control)
    {
        return Err("invite_invalid_request_or_name".into());
    }
    kiri_provision::validate_limits(&options.profile)?;
    private_root(&options.profile.registry_directory)?;
    let _lock = operation_lock(options)?;
    planned_invitation(options)?;
    let a = runtime_registry::reserve(
        &options.profile.registry_directory,
        options.request_id,
        options.profile.email.clone(),
    )
    .map_err(|_| "invite_reservation_conflict")?;
    planned_invitation(options)?;
    check_record(options, &a)?;
    let initial = not_fenced(options, &a)?;
    save(options, &a, "reserved")?;
    let prepared = match kiri_provision::prepare_member(&options.profile).await {
        Ok(p) => p,
        Err(reason) => return emit(options, &a, "preparation_required", Some(&reason)),
    };
    if prepared.account_id != a.account_id
        || prepared.tenant_id != a.tenant_id
        || prepared.volume_id != a.volume_id
        || prepared.generation != a.generation
    {
        return Err("invite_preflight_identity_changed".into());
    }
    not_fenced(options, &a)?;
    if initial == InviteState::Ready {
        let mut args = options.profile.clone();
        args.start = true;
        kiri_provision::reconcile_member(&args).await?;
        if !ready(&prepared, &a, options.profile.uid).await {
            return emit(
                options,
                &a,
                "service_waiting",
                Some("owner_service_not_ready"),
            );
        }
        if state(options, &a)? != InviteState::Ready {
            return Err("invite_allocation_changed".into());
        }
        return emit(options, &a, "ready", None);
    }
    if prepared.runtime_running {
        if !ready(&prepared, &a, options.profile.uid).await {
            return emit(
                options,
                &a,
                "service_waiting",
                Some("existing_owner_service_not_ready"),
            );
        }
    } else {
        let initialized = match child(options, &a, &prepared, false, None) {
            Ok(value) => value,
            Err(_) => {
                return emit(
                    options,
                    &a,
                    "initialization_waiting",
                    Some("owner_status_unavailable_or_mismatched"),
                );
            }
        };
        if !initialized {
            let Some(secret) = password(options)? else {
                return emit(options, &a, "password_required", None);
            };
            not_fenced(options, &a)?;
            save(options, &a, "initializing")?;
            child(options, &a, &prepared, true, Some(&secret))?;
            drop(secret);
            if !child(options, &a, &prepared, false, None)? {
                return Err("invite_initialization_not_verified".into());
            }
        }
        not_fenced(options, &a)?;
        save(options, &a, "initialized")?;
        let mut args = options.profile.clone();
        args.start = true;
        kiri_provision::reconcile_member(&args).await?;
        if !ready(&prepared, &a, options.profile.uid).await {
            return emit(
                options,
                &a,
                "service_waiting",
                Some("owner_service_not_ready"),
            );
        }
    }
    runtime_registry::activate_invitation(
        &options.profile.registry_directory,
        options.request_id,
        &a,
    )
    .map_err(|_| "invite_activation_fenced_or_changed")?;
    emit(options, &a, "ready", None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flattened_invite_profile_has_unique_cli_groups() {
        Options::augment_args(clap::Command::new("kiri-invite")).debug_assert();
    }
}
