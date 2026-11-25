//! Complete a web invitation using the installation's pinned Linux profile.
use super::*;
use crate::{kiri_invite, kiri_storage};
use clap::Args;
use moosuri_store::runtime_registry::admin;
use tempfile::Builder;

#[derive(Args)]
pub struct Options {
    #[arg(long)]
    pub experimental: bool,
    /// The same private deployment configuration the recovery worker uses.
    #[arg(long)]
    pub config: PathBuf,
    /// Stable request ID displayed in the member administration page.
    #[arg(long)]
    pub request_id: Uuid,
    /// Only for the first account on a fresh installation. Later users are invited on the web.
    #[arg(long, requires = "first_email")]
    pub first_name: Option<String>,
    #[arg(long, requires = "first_name")]
    pub first_email: Option<String>,
    /// Dedicated host UID/GID selected for this member.
    #[arg(long)]
    pub uid: u32,
    #[arg(long)]
    pub operations_root: PathBuf,
    #[arg(long)]
    pub enable_text_preview: bool,
    #[arg(long)]
    pub enable_google_readonly: bool,
    /// Enable member-local GWS using the installation's Google egress relay.
    #[arg(long, conflicts_with = "enable_google_readonly")]
    pub enable_local_gws: bool,
    #[arg(long)]
    pub password_stdin: bool,
}
const FILE: &str = "members.json";
#[derive(Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
struct Enrollments {
    members: Vec<Member>,
}
fn separate(root: &Path, other: &Path) -> Result<(), String> {
    if root.starts_with(other) || other.starts_with(root) {
        return Err("onboard_enrollment_directory_not_dedicated".into());
    }
    Ok(())
}
fn enrollments(root: &Path) -> Result<Enrollments, String> {
    trusted_path(root, true)?;
    for entry in fs::read_dir(root).map_err(|_| "enrollments_unavailable")? {
        let entry = entry.map_err(|_| "enrollments_unavailable")?;
        let name = entry.file_name();
        if name != FILE && !name.to_str().is_some_and(|n| n.starts_with(".enrollment-")) {
            return Err("onboard_enrollment_directory_not_dedicated".into());
        }
    }
    let path = root.join(FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => read_json(&path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Enrollments::default()),
        Err(_) => Err("enrollments_unavailable".into()),
    }
}
pub(super) fn include(
    config: &mut Config,
    path: &Path,
    private_directory: &Path,
) -> Result<(), String> {
    if let Some(root) = config.enrollment_directory.as_ref() {
        for other in [
            path,
            private_directory,
            &config.registry_directory,
            &config.mounts_root,
            &config.ipc_root,
            &config.allocations_root,
            &config.engine_socket,
        ]
        .into_iter()
        .chain(config.model_sockets_root.iter().map(PathBuf::as_path))
        .chain(config.google_sockets_root.iter().map(PathBuf::as_path))
        .chain(config.browser_sockets_root.iter().map(PathBuf::as_path))
        {
            separate(root, other)?;
        }
        config.members.extend(enrollments(root)?.members);
    }
    config.validate()
}
fn matching(config: &Config, member: &Member) -> Result<bool, String> {
    let mut members = config.members.iter().filter(|m| {
        m.account_id == member.account_id || m.uid == member.uid || m.email == member.email
    });
    match (members.next(), members.next()) {
        (None, None) => Ok(false),
        (Some(p), None)
            if p.account_id == member.account_id
                && p.generation == member.generation
                && config.provision(p) == config.provision(member) =>
        {
            Ok(true)
        }
        _ => Err("onboard_member_conflicts_with_recovery_profile".into()),
    }
}
fn register(
    config_path: &Path,
    operations_root: &Path,
    expected: &Config,
    member: Member,
) -> Result<(), String> {
    let root = expected
        .enrollment_directory
        .as_ref()
        .ok_or("onboard_enrollment_directory_required")?;
    trusted_path(root, true)?;
    let dir = File::open(root).map_err(|_| "enrollments_unavailable")?;
    FileExt::lock_exclusive(&dir).map_err(|_| "enrollments_busy")?;
    let mut current = load(config_path, operations_root)?;
    if current.provision(&member) != expected.provision(&member)
        || current.enrollment_directory.as_ref() != Some(root)
    {
        return Err("onboard_installation_profile_changed".into());
    }
    if matching(&current, &member)? {
        return Ok(());
    }
    current.members.push(member.clone());
    current.validate()?;
    let mut list = enrollments(root)?;
    list.members.push(member);
    let mut file = Builder::new()
        .prefix(".enrollment-")
        .tempfile_in(root)
        .map_err(|_| "enrollments_unavailable")?;
    serde_json::to_writer(file.as_file_mut(), &list).map_err(|_| "enrollments_write_failed")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "enrollments_write_failed")?;
    file.persist(root.join(FILE))
        .map_err(|_| "enrollments_write_failed")?;
    dir.sync_all()
        .map_err(|_| "enrollments_write_failed".into())
}
pub(super) fn context(
    config_path: &Path,
    operations_root: &Path,
) -> Result<(Config, PathBuf), String> {
    let config = load(config_path, operations_root)?;
    let root = config
        .mounts_root
        .parent()
        .ok_or("onboard_storage_layout_invalid")?
        .to_owned();
    if config.mounts_root != root.join("mounts")
        || config.ipc_root != root.join("ipc")
        || config.allocations_root != root.join("assignments")
    {
        return Err("onboard_storage_layout_invalid".into());
    }
    let enrollment = config
        .enrollment_directory
        .as_ref()
        .ok_or("onboard_enrollment_directory_required")?;
    separate(enrollment, &root)?;
    separate(operations_root, &root)?;
    separate(operations_root, &config.registry_directory)?;
    trusted_path(operations_root, true)?;
    Ok((config, root))
}

pub async fn run(o: &Options) -> Result<(), String> {
    if !o.experimental || !cfg!(target_os = "linux") || unsafe { libc::geteuid() } != 0 {
        return Err("onboard_root_linux_only".into());
    }
    let (config, root) = context(&o.config, &o.operations_root)?;
    let _lock = worker::lock(&o.operations_root)?;
    run_locked(o, config, root, true).await
}

pub(super) async fn run_locked(
    o: &Options,
    mut config: Config,
    root: PathBuf,
    allow_terminal: bool,
) -> Result<(), String> {
    if let (Some(name), Some(email)) = (&o.first_name, &o.first_email) {
        admin::reserve_first_invitation(
            &config.registry_directory,
            o.request_id,
            name.clone(),
            email.clone(),
        )
        .map_err(|_| "onboard_first_invitation_conflicts_with_installation")?;
    }
    let invite = admin::operator_invitation(&config.registry_directory, o.request_id)
        .map_err(|_| "onboard_invitation_invalid")?
        .ok_or("onboard_web_invitation_required")?;
    let m = Member {
        account_id: Uuid::parse_str(&invite.account_id)
            .map_err(|_| "onboard_invitation_invalid")?,
        generation: invite.generation,
        email: invite.email.clone(),
        uid: o.uid,
        enable_text_preview: o.enable_text_preview,
        enable_google_readonly: o.enable_google_readonly,
        enable_local_gws: o.enable_local_gws,
        browser_enabled: None,
        vault_origin: None,
    };
    worker::check_claim(&o.operations_root, &config, &invite, &m)?;
    if !matching(&config, &m)? {
        config.members.push(m.clone());
        config.validate()?;
    }
    let allocation = runtime_registry::lookup(&config.registry_directory, invite.email.clone())
        .map_err(|_| "onboard_allocation_unavailable")?;
    let storage = kiri_storage::Options {
        experimental: true,
        registry_directory: config.registry_directory.clone(),
        request_id: o.request_id,
        email: invite.email.clone(),
        root: root.to_owned(),
        uid: o.uid,
        capacity_bytes: config.max_volume_bytes,
    };
    let account = m.account_id;
    let uid = o.uid;
    tokio::task::spawn_blocking(move || kiri_storage::prepare(&storage, &allocation, uid))
        .await
        .map_err(|_| "onboard_storage_worker_failed")??;
    let profile = config.provision(&m);
    register(&o.config, &o.operations_root, &config, m)?;
    // Register first so the relays can discover this reserved member. On
    // timeout, the durable enrollment is reused by the same invitation.
    let mut sockets = Vec::new();
    if let Some(root) = profile.browser_sockets_root.as_ref() {
        sockets.push((
            root,
            "browser.sock",
            "onboard_browser_relay_not_ready_retry_same_invitation",
        ));
    }
    if config.public_origin.is_some()
        && let Some(root) = profile.google_sockets_root.as_ref()
    {
        sockets.push((
            root,
            "google.sock",
            "onboard_gws_relay_not_ready_retry_same_invitation",
        ));
    }
    for (root, socket, reason) in sockets {
        let path = root.join(account.to_string()).join(socket);
        tokio::time::timeout(Duration::from_secs(12), async {
            while !path.try_exists().map_err(|_| reason)? {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok::<_, String>(())
        })
        .await
        .map_err(|_| reason)??;
    }
    kiri_invite::run_with_terminal(
        &kiri_invite::Options {
            request_id: o.request_id,
            name: invite.name,
            operations_root: o.operations_root.clone(),
            password_stdin: o.password_stdin,
            profile,
        },
        allow_terminal,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enrollment_requires_a_separate_tree() {
        let enrollment = Path::new("/srv/enrolled");
        for other in ["/srv", "/srv/enrolled", "/srv/enrolled/members.json"] {
            assert!(separate(enrollment, Path::new(other)).is_err());
        }
        assert!(separate(enrollment, Path::new("/srv/enrolled-2")).is_ok());
    }
}
