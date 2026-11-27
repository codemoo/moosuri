//! Optional host-only preparation of current administrator web invitations.
use super::*;
use crate::kiri_storage;
use moosuri_store::runtime_registry::admin::{self, Invitation};

const FILE: &str = "web-onboarding.json";

#[derive(Args)]
pub struct Options {
    #[arg(long)]
    pub experimental: bool,
    #[arg(long)]
    pub config: PathBuf,
    /// Private installation policy with dedicated UIDs and total worker disk budget.
    #[arg(long)]
    pub policy: PathBuf,
    #[arg(long)]
    pub operations_root: PathBuf,
    /// Resume one worker reservation with operator-controlled password input.
    #[arg(long)]
    pub finish_request: Option<Uuid>,
    #[arg(long, requires = "finish_request")]
    pub password_stdin: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    uid_pool: Vec<u32>,
    max_total_volume_bytes: u64,
    #[serde(default)]
    enable_text_preview: bool,
}

impl Policy {
    fn validate(&self, config: &Config) -> Result<(), String> {
        if self.uid_pool.is_empty()
            || self.uid_pool.len() > 128
            || self.uid_pool.iter().collect::<BTreeSet<_>>().len() != self.uid_pool.len()
            || self.uid_pool.iter().any(|u| !(10000..=60000).contains(u))
            || self.max_total_volume_bytes < config.max_volume_bytes
            || config.max_volume_bytes < 64 << 20
            || config.max_volume_bytes % 4096 != 0
            || (self.enable_text_preview && config.model_sockets_root.is_none())
        {
            return Err("onboard_worker_invalid_policy".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    invitation: Invitation,
    enrollment: PathBuf,
    public_origin: Option<String>,
    profile: kiri_provision::Options,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Queue {
    after: i64,
    claims: BTreeMap<Uuid, Claim>,
}

impl Queue {
    fn check_installation(&self, config: &Config) -> Result<(), String> {
        if self.claims.values().any(|c| {
            c.profile.registry_directory != config.registry_directory
                || config.enrollment_directory.as_ref() != Some(&c.enrollment)
                || c.profile.mounts_root != config.mounts_root
                || c.profile.ipc_root != config.ipc_root
                || c.profile.allocations_root != config.allocations_root
        }) {
            return Err("onboard_worker_operations_belong_to_another_installation".into());
        }
        Ok(())
    }
}

pub(super) fn lock(root: &Path) -> Result<File, String> {
    let directory = File::open(root).map_err(|_| "onboard_operations_unavailable")?;
    directory
        .try_lock_exclusive()
        .map_err(|_| "onboard_operation_in_progress")?;
    Ok(directory)
}

fn queue(root: &Path) -> Result<Queue, String> {
    let path = root.join(FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => read_json(&path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Queue::default()),
        Err(_) => Err("onboard_worker_queue_unavailable".into()),
    }
}

fn save(root: &Path, queue: &Queue) -> Result<(), String> {
    let mut file = tempfile::Builder::new()
        .prefix(".web-queue-")
        .tempfile_in(root)
        .map_err(|_| "onboard_worker_queue_unavailable")?;
    let bytes = serde_json::to_vec(queue).map_err(|_| "onboard_worker_claim_invalid")?;
    if bytes.len() as u64 > LIMIT {
        return Err("onboard_worker_queue_too_large".into());
    }
    file.write_all(&bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "onboard_worker_queue_write_failed")?;
    file.persist(root.join(FILE))
        .map_err(|_| "onboard_worker_queue_write_failed")?;
    File::open(root)
        .and_then(|dir| dir.sync_all())
        .map_err(|_| "onboard_worker_queue_write_failed".into())
}

fn member(i: &Invitation, uid: u32, text: bool) -> Result<Member, String> {
    Ok(Member {
        account_id: Uuid::parse_str(&i.account_id).map_err(|_| "onboard_invitation_invalid")?,
        generation: i.generation,
        email: i.email.clone(),
        uid,
        enable_text_preview: text,
        enable_google_readonly: false,
        enable_local_gws: false,
        browser_enabled: None,
        vault_origin: None,
    })
}

impl Claim {
    fn verify(&self, config: &Config, i: &Invitation, m: &Member) -> Result<(), String> {
        if self.public_origin != config.public_origin
            || &self.invitation != i
            || config.enrollment_directory.as_ref() != Some(&self.enrollment)
            || config.provision(m) != self.profile
        {
            return Err("onboard_worker_pinned_profile_changed".into());
        }
        Ok(())
    }
}

/// Manual and automatic onboarding share the same lock and durable UID exclusions.
pub(super) fn check_claim(
    root: &Path,
    config: &Config,
    i: &Invitation,
    m: &Member,
) -> Result<(), String> {
    let q = queue(root)?;
    q.check_installation(config)?;
    if let Some(c) = q.claims.get(&i.request_id) {
        c.verify(config, i, m)?;
    }
    if q.claims.iter().any(|(req, c)| {
        *req != i.request_id && (c.profile.uid == m.uid || c.invitation.account_id == i.account_id)
    }) {
        return Err("onboard_worker_uid_or_account_reserved".into());
    }
    Ok(())
}

enum Decision {
    Prepare,
    Manual,
    Capacity,
}

fn reserve(
    q: &mut Queue,
    p: &Policy,
    config: &Config,
    i: Invitation,
    storage: &[(Uuid, u32)],
    host_available: impl Fn(u32) -> bool,
) -> Result<Decision, String> {
    if q.claims.contains_key(&i.request_id) {
        return Ok(Decision::Prepare);
    }
    if config
        .members
        .iter()
        .any(|m| m.account_id.to_string() == i.account_id)
        || storage.iter().any(|(req, _)| *req == i.request_id)
    {
        return Ok(Decision::Manual);
    }
    let total = q.claims.values().try_fold(config.max_volume_bytes, |n, c| {
        n.checked_add(c.profile.max_volume_bytes)
    });
    let pending = q
        .claims
        .values()
        .filter(|c| {
            !config
                .members
                .iter()
                .any(|m| m.account_id.to_string() == c.invitation.account_id)
        })
        .count();
    if q.claims.len() >= 128
        || config.members.len() + pending >= 128
        || total.is_none_or(|n| n > p.max_total_volume_bytes)
    {
        return Ok(Decision::Capacity);
    }
    let Some(&uid) = p.uid_pool.iter().find(|&&uid| {
        !q.claims.values().any(|c| c.profile.uid == uid)
            && !config.members.iter().any(|m| m.uid == uid)
            && !storage.iter().any(|(_, u)| *u == uid)
            && host_available(uid)
    }) else {
        return Ok(Decision::Capacity);
    };
    let m = member(&i, uid, p.enable_text_preview)?;
    let profile = config.provision(&m);
    kiri_provision::validate_limits(&profile)?;
    q.claims.insert(
        i.request_id,
        Claim {
            invitation: i,
            public_origin: config.public_origin.clone(),
            profile,
            enrollment: config
                .enrollment_directory
                .clone()
                .ok_or("onboard_enrollment_directory_required")?,
        },
    );
    Ok(Decision::Prepare)
}

fn waiting(root: &Path, req: Uuid) -> Result<bool, String> {
    let path = root.join(format!("{req}.json"));
    if !path
        .try_exists()
        .map_err(|_| "onboard_operation_unavailable")?
    {
        return Ok(false);
    }
    let record: serde_json::Value = read_json(&path)?;
    Ok(record["stage"] == "password_required")
}

async fn handle(
    o: &Options,
    policy: &Policy,
    registry: &Path,
    request_id: Uuid,
    finish: bool,
    web: Option<&crate::kiri_invite::WebInput>,
) -> Result<(), String> {
    let (config, root) = onboard::context(&o.config, &o.operations_root)?;
    if config.registry_directory != registry {
        return Err("onboard_worker_registry_changed".into());
    }
    let i = admin::operator_invitation(registry, request_id)
        .map_err(|_| "onboard_invitation_invalid")?
        .ok_or("onboard_invitation_missing")?;
    let mut q = queue(&o.operations_root)?;
    q.check_installation(&config)?;
    if !q.claims.contains_key(&request_id) {
        if finish {
            return Err("onboard_worker_request_not_claimed".into());
        }
        // Never adopt a manual operation, including a pre-enrollment storage reservation.
        if o.operations_root
            .join(format!("{request_id}.json"))
            .try_exists()
            .map_err(|_| "onboard_operation_unavailable")?
        {
            return Ok(());
        }
        let a = runtime_registry::lookup(&config.registry_directory, i.email.clone())
            .map_err(|_| "onboard_allocation_unavailable")?;
        if runtime_registry::invitation_state(&config.registry_directory, request_id, &a)
            .map_err(|_| "onboard_invitation_changed")?
            != runtime_registry::InviteState::Reserved
        {
            return Ok(());
        }
        policy.validate(&config)?;
        let storage = kiri_storage::reserved_uids(&root)?;
        match reserve(&mut q, policy, &config, i.clone(), &storage, |uid| {
            kiri_storage::unused_uid(uid).is_ok()
                && matches!(config.allocations_root.join(format!("uid-{uid}.json")).symlink_metadata(), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        })? {
            Decision::Manual => return Ok(()),
            Decision::Capacity => {
                println!("request_id={request_id} stage=capacity_waiting");
                return Ok(());
            }
            Decision::Prepare => save(&o.operations_root, &q)?,
        }
    }
    let claim = q
        .claims
        .get(&request_id)
        .ok_or("onboard_worker_request_not_claimed")?;
    let m = member(&i, claim.profile.uid, claim.profile.enable_text_preview)?;
    claim.verify(&config, &i, &m)?;
    if !policy.uid_pool.contains(&m.uid) {
        return Err("onboard_worker_uid_no_longer_allowed".into());
    }
    if !finish && waiting(&o.operations_root, request_id)? {
        println!("request_id={request_id} stage=password_required");
        return Ok(());
    }
    let opts = onboard::Options {
        experimental: true,
        config: o.config.clone(),
        request_id,
        operations_root: o.operations_root.clone(),
        uid: m.uid,
        first_name: None,
        first_email: None,
        enable_text_preview: m.enable_text_preview,
        enable_google_readonly: false,
        enable_local_gws: false,
        password_stdin: finish && o.password_stdin,
    };
    onboard::run_locked_with_input(&opts, config, root, finish && web.is_none(), web).await
}

pub async fn run(o: &Options) -> Result<(), String> {
    if !o.experimental || !cfg!(target_os = "linux") || unsafe { libc::geteuid() } != 0 {
        return Err("onboard_worker_root_linux_only".into());
    }
    let (config, _) = onboard::context(&o.config, &o.operations_root)?;
    let policy: Policy = read_json(&o.policy)?;
    policy.validate(&config)?;
    let _lock = lock(&o.operations_root)?;
    if let Some(request) = o.finish_request {
        return handle(o, &policy, &config.registry_directory, request, true, None).await;
    }
    let after = queue(&o.operations_root)?.after;
    let page = admin::pending_web_invitations(&config.registry_directory, after)
        .map_err(|_| "onboard_worker_registry_unavailable")?;
    let mut failed = false;
    for (_, request) in &page {
        if let Err(reason) = handle(
            o,
            &policy,
            &config.registry_directory,
            *request,
            false,
            None,
        )
        .await
        {
            eprintln!("request_id={request} stage=blocked reason={reason}");
            failed = true;
        }
    }
    let next = page.last().map_or(0, |(position, _)| *position);
    if next != after {
        let mut q = queue(&o.operations_root)?;
        q.after = next;
        save(&o.operations_root, &q)?;
    }
    if failed {
        Err("onboard_worker_incomplete_requests".into())
    } else {
        Ok(())
    }
}

/// Explicit installation profile; never accepted from an HTTP request.
#[derive(Clone)]
pub(crate) struct WebEnrollment {
    pub config: PathBuf,
    pub policy: PathBuf,
    pub operations_root: PathBuf,
}
impl WebEnrollment {
    pub(crate) fn validate(&self, registry: &Path, origin: &str) -> Result<(), String> {
        if !cfg!(target_os = "linux") || unsafe { libc::geteuid() } != 0 {
            return Err("onboard_worker_root_linux_only".into());
        }
        let (config, _) = onboard::context(&self.config, &self.operations_root)?;
        if config.registry_directory != registry || config.public_origin.as_deref() != Some(origin)
        {
            return Err("onboard_web_installation_mismatch".into());
        }
        let policy: Policy = read_json(&self.policy)?;
        policy.validate(&config)
    }
    /// Timer prepares resources; web only finishes its already pinned claim.
    /// Password remains in request memory and the dedicated UID child's pipe.
    pub(crate) async fn redeem(
        &self,
        registry: &Path,
        origin: &str,
        input: &crate::kiri_invite::WebInput,
    ) -> Result<(), String> {
        use admin::invitation_links;
        self.validate(registry, origin)?;
        let _lock = lock(&self.operations_root)?;
        let link = invitation_links::inspect(registry, &input.token)
            .map_err(|_| "invite_link_unavailable")?;
        if link.ready {
            return Ok(());
        }
        let request = link.invitation.request_id;
        let q = queue(&self.operations_root)?;
        if !q.claims.contains_key(&request) {
            return Err("onboard_web_preparing".into());
        }
        let stage: serde_json::Value =
            read_json(&self.operations_root.join(format!("{request}.json")))
                .map_err(|_| "onboard_web_preparing")?;
        if !matches!(
            stage["stage"].as_str(),
            Some(
                "password_required"
                    | "initializing"
                    | "initialized"
                    | "service_waiting"
                    | "web_confirmation_required"
                    | "ready"
            )
        ) {
            return Err("onboard_web_preparing".into());
        }
        let policy: Policy = read_json(&self.policy)?;
        let opts = Options {
            experimental: true,
            config: self.config.clone(),
            policy: self.policy.clone(),
            operations_root: self.operations_root.clone(),
            finish_request: Some(request),
            password_stdin: false,
        };
        handle(&opts, &policy, registry, request, true, Some(input)).await?;
        if !invitation_links::inspect(registry, &input.token)
            .map_err(|_| "invite_link_unavailable")?
            .ready
        {
            return Err("onboard_web_preparing".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    fn config() -> Config {
        serde_json::from_value(json!({"version":1,"registry_directory":"/srv/registry",
            "enrollment_directory":"/srv/enrolled","mounts_root":"/srv/storage/mounts",
            "ipc_root":"/srv/storage/ipc","allocations_root":"/srv/storage/assignments",
            "max_volume_bytes":67108864,"image":format!("sha256:{}","a".repeat(64)),"members":[]}))
        .unwrap()
    }

    fn invite(n: u32) -> Invitation {
        Invitation {
            request_id: Uuid::new_v4(),
            account_id: Uuid::new_v4().to_string(),
            tenant_id: Uuid::new_v4().to_string(),
            volume_id: Uuid::new_v4().to_string(),
            generation: 1,
            email: format!("synthetic-{n}@example.test"),
            name: format!("Synthetic {n}"),
        }
    }

    #[test]
    fn queue_restarts_keep_the_same_uid_and_profile_and_exclude_concurrent_writers() {
        let root = tempfile::tempdir().unwrap();
        let dir = lock(root.path()).unwrap();
        assert!(lock(root.path()).is_err());
        let config = config();
        let i = invite(1);
        let policy = Policy {
            uid_pool: vec![40500, 40501],
            max_total_volume_bytes: 128 << 20,
            enable_text_preview: false,
        };
        let mut q = Queue::default();
        reserve(&mut q, &policy, &config, i.clone(), &[], |_| true).unwrap();
        save(root.path(), &q).unwrap();
        drop(dir);
        let _next_process = lock(root.path()).unwrap();
        let path = root.path().join(FILE);
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut q: Queue = serde_json::from_reader(File::open(&path).unwrap()).unwrap();
        assert!(matches!(
            reserve(&mut q, &policy, &config, i.clone(), &[], |_| false).unwrap(),
            Decision::Prepare
        ));
        let j = invite(2);
        reserve(&mut q, &policy, &config, j.clone(), &[], |_| true).unwrap();
        let c = &q.claims[&i.request_id];
        assert_eq!(c.profile.uid, 40500);
        assert_eq!(q.claims[&j.request_id].profile.uid, 40501);
        let m = member(&i, 40500, false).unwrap();
        c.verify(&config, &i, &m).unwrap();
        let mut wrong = i.clone();
        wrong.volume_id = j.volume_id.clone();
        assert!(c.verify(&config, &wrong, &m).is_err());
        let mut changed = config;
        changed.image = format!("sha256:{}", "b".repeat(64));
        assert!(c.verify(&changed, &i, &m).is_err());
        q.check_installation(&changed).unwrap();
        changed.registry_directory = "/srv/other-registry".into();
        assert!(q.check_installation(&changed).is_err());
        assert_eq!(q.claims.len(), 2);
    }

    #[test]
    fn host_users_storage_claims_and_manual_enrollments_preserved_with_bounded_capacity() {
        let policy = Policy {
            uid_pool: vec![40100, 40101, 40102, 40103, 40104],
            max_total_volume_bytes: 64 << 20,
            enable_text_preview: false,
        };
        let mut config = config();
        let manual = invite(1);
        config.members.push(member(&manual, 40102, false).unwrap());
        let partial = invite(2);
        let records = [(partial.request_id, 40101)];
        let mut q = Queue::default();
        for i in [partial, manual] {
            assert!(matches!(
                reserve(&mut q, &policy, &config, i, &records, |_| true).unwrap(),
                Decision::Manual
            ));
        }
        let i = invite(3);
        reserve(&mut q, &policy, &config, i.clone(), &records, |uid| {
            uid != 40100
        })
        .unwrap();
        assert_eq!(q.claims[&i.request_id].profile.uid, 40103);
        assert!(matches!(
            reserve(&mut q, &policy, &config, invite(4), &records, |_| true).unwrap(),
            Decision::Capacity
        ));
        assert_eq!(q.claims.len(), 1);
        let mut invalid = policy;
        invalid.uid_pool = vec![40100, 40100];
        assert!(invalid.validate(&config).is_err());
        invalid.uid_pool = vec![0];
        assert!(invalid.validate(&config).is_err());
    }

    #[test]
    fn exhausted_pool_leaves_new_invitation_unclaimed_even_with_disk_budget() {
        let config = config();
        let mut q = Queue::default();
        let policy = Policy {
            uid_pool: vec![40500],
            max_total_volume_bytes: 128 << 20,
            enable_text_preview: false,
        };
        reserve(&mut q, &policy, &config, invite(1), &[], |_| true).unwrap();
        assert!(matches!(
            reserve(&mut q, &policy, &config, invite(2), &[], |_| true).unwrap(),
            Decision::Capacity
        ));
        assert_eq!(q.claims.len(), 1);
    }
}
