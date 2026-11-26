//! Existing-member container rollout. Replaces only the exact inspected, owned member
//! container of a member that already exists, keeping identity, generation, UID, data,
//! IPC, scratch and limits byte-identical. The old and the target profile are both
//! built through the normal provisioner validation; only image, broker socket mounts,
//! vault origin, local-GWS and text-preview flags may differ.
//!
//! Safety contract: the allocation lock is held for the whole operation (so the
//! lifecycle worker's reconcile cannot interleave), the lifecycle state directory lock
//! must be free (worker stopped), the container is addressed by its inspected ID, every
//! destructive step is preceded by a fresh inspection, and removal never removes
//! volumes (`v=false`, no force). Host data/volumes/registry are never touched.
//! After a successful create the target is never deleted; failures are reported as
//! the true state and nothing is rolled back automatically. Retrying the same
//! command is safe: a container that already matches the target is only reconciled.
use super::{
    API, Engine, Expected, PROFILE, expected_member, reconcile, reserve_expected, trusted_dir,
    verify_inspect,
};
use clap::Args;
use fs2::FileExt;
use serde_json::{Value, json};
use std::{fs, fs::File, path::PathBuf, time::Duration};

#[derive(Args, Clone, Debug)]
pub struct Options {
    /// Explicitly opt in to the experimental profile.
    #[arg(long)]
    pub experimental: bool,
    #[arg(long)]
    pub registry_directory: PathBuf,
    #[arg(long)]
    pub email: String,
    #[arg(long)]
    pub mounts_root: PathBuf,
    #[arg(long)]
    pub ipc_root: PathBuf,
    #[arg(long)]
    pub allocations_root: PathBuf,
    /// Lifecycle worker state directory. Its lock must be free: stop the worker first.
    #[arg(long)]
    pub lifecycle_state_directory: PathBuf,
    /// Exact full ID of the currently installed (old) container, from a prior inspect.
    #[arg(long)]
    pub expected_container_id: String,
    #[arg(long)]
    pub uid: u32,
    #[arg(long, default_value = "/run/kiri/docker.sock")]
    pub engine_socket: PathBuf,
    #[arg(long, default_value_t = 536870912)]
    pub memory_bytes: u64,
    #[arg(long, default_value_t = 1000000000)]
    pub nano_cpus: u64,
    #[arg(long, default_value_t = 128)]
    pub pids: u32,
    #[arg(long, default_value_t = 10737418240)]
    pub max_volume_bytes: u64,
    /// Currently installed local sha256 image config ID.
    #[arg(long)]
    pub old_image: String,
    /// Target local sha256 image config ID.
    #[arg(long)]
    pub new_image: String,
    #[arg(long)]
    pub old_model_sockets_root: Option<PathBuf>,
    #[arg(long)]
    pub new_model_sockets_root: Option<PathBuf>,
    #[arg(long)]
    pub old_google_sockets_root: Option<PathBuf>,
    #[arg(long)]
    pub new_google_sockets_root: Option<PathBuf>,
    #[arg(long)]
    pub old_browser_sockets_root: Option<PathBuf>,
    #[arg(long)]
    pub new_browser_sockets_root: Option<PathBuf>,
    #[arg(long)]
    pub old_vault_origin: Option<String>,
    #[arg(long)]
    pub new_vault_origin: Option<String>,
    #[arg(long)]
    pub old_enable_local_gws: bool,
    #[arg(long)]
    pub new_enable_local_gws: bool,
    #[arg(long)]
    pub old_enable_text_preview: bool,
    #[arg(long)]
    pub new_enable_text_preview: bool,
    /// Start the target and require owner-identified readiness. Without it the
    /// target is left created/stopped and readiness is reported as not checked.
    #[arg(long)]
    pub start: bool,
}

impl Options {
    fn profile(&self, target: bool) -> Result<super::Options, String> {
        let pick = |old: &Option<PathBuf>, new: &Option<PathBuf>| -> Option<PathBuf> {
            let chosen = if target { new } else { old };
            chosen.clone()
        };
        let text_preview = if target {
            self.new_enable_text_preview
        } else {
            self.old_enable_text_preview
        };
        let model = pick(&self.old_model_sockets_root, &self.new_model_sockets_root);
        if text_preview && model.is_none() {
            return Err("kiri_rollout_text_preview_requires_model_sockets_root".into());
        }
        Ok(super::Options {
            experimental: self.experimental,
            registry_directory: self.registry_directory.clone(),
            email: self.email.clone(),
            mounts_root: self.mounts_root.clone(),
            ipc_root: self.ipc_root.clone(),
            allocations_root: self.allocations_root.clone(),
            model_sockets_root: model,
            google_sockets_root: pick(&self.old_google_sockets_root, &self.new_google_sockets_root),
            browser_sockets_root: pick(
                &self.old_browser_sockets_root,
                &self.new_browser_sockets_root,
            ),
            vault_origin: if target {
                self.new_vault_origin.clone()
            } else {
                self.old_vault_origin.clone()
            },
            enable_local_gws: if target {
                self.new_enable_local_gws
            } else {
                self.old_enable_local_gws
            },
            enable_text_preview: text_preview,
            uid: self.uid,
            image: if target {
                self.new_image.clone()
            } else {
                self.old_image.clone()
            },
            engine_socket: self.engine_socket.clone(),
            memory_bytes: self.memory_bytes,
            nano_cpus: self.nano_cpus,
            pids: self.pids,
            max_volume_bytes: self.max_volume_bytes,
            start: self.start,
        })
    }
}

pub async fn run(args: &Options) -> Result<(), String> {
    let a = args.clone();
    let mut out = tokio::task::spawn_blocking(move || blocking(&a))
        .await
        .map_err(|_| "kiri_rollout_worker_failed".to_owned())??;
    if args.start {
        let a = args.clone();
        let allocation = tokio::task::spawn_blocking(move || {
            moosuri_store::runtime_registry::lookup(&a.registry_directory, a.email.clone())
                .map_err(|_| "kiri_rollout_registry_lookup_failed".to_owned())
        })
        .await
        .map_err(|_| "kiri_rollout_worker_failed".to_owned())??;
        let socket = args
            .ipc_root
            .join(&allocation.account_id)
            .join("personal.sock");
        let mut last = "member_not_ready";
        for attempt in 0..30 {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            match crate::kiri_lifecycle::health(&socket, &allocation, args.uid).await {
                Ok(()) => {
                    last = "";
                    break;
                }
                Err(reason) => last = reason,
            }
        }
        if !last.is_empty() {
            // The target is deliberately retained, with all data, for inspection.
            return Err(format!(
                "kiri_rollout_target_running_not_ready:{last}:container={}",
                out["container_id"].as_str().unwrap_or("unknown")
            ));
        }
        out["readiness"] = json!("owner-identified-health-ready");
    }
    println!("{out}");
    Ok(())
}

fn container_id(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn blocking(args: &Options) -> Result<Value, String> {
    if !container_id(&args.expected_container_id) {
        return Err("kiri_rollout_invalid_container_id".into());
    }
    let old_options = args.profile(false)?;
    let new_options = args.profile(true)?;
    // Held (non-blocking) so a running lifecycle worker is refused, not raced.
    trusted_dir(&args.lifecycle_state_directory, 0, true)?;
    let state = File::open(&args.lifecycle_state_directory)
        .map_err(|_| "kiri_rollout_lifecycle_state_unavailable")?;
    state
        .try_lock_exclusive()
        .map_err(|_| "kiri_rollout_lifecycle_worker_running")?;
    let (new, engine) = expected_member(&new_options, true)?;
    // Rollout never creates a UID reservation: the member must already own one.
    let reservation = args.allocations_root.join(format!("uid-{}.json", new.uid));
    if !fs::symlink_metadata(&reservation).is_ok_and(|m| m.is_file()) {
        return Err("kiri_rollout_existing_reservation_required".into());
    }
    let _lock = reserve_expected(&new_options, &new)?;
    let load_old = || expected_member(&old_options, true).map(|(e, _)| e);
    let load_new = || expected_member(&new_options, true).map(|(e, _)| e);
    let result = rollout(
        &engine,
        &new,
        &load_old,
        &load_new,
        &args.expected_container_id,
        args.start,
    );
    drop(state);
    result
}

/// Everything except image/socket/vault/GWS/preview features must be identical.
fn same_identity(a: &Expected, b: &Expected) -> bool {
    a.name == b.name
        && a.account == b.account
        && a.tenant == b.tenant
        && a.volume == b.volume
        && a.generation == b.generation
        && a.uid == b.uid
        && a.data == b.data
        && a.ipc == b.ipc
        && a.scratch == b.scratch
        && a.memory == b.memory
        && a.nano_cpus == b.nano_cpus
        && a.pids == b.pids
        && a.env == b.env
}

fn rollout(
    engine: &Engine,
    new: &Expected,
    load_old: &dyn Fn() -> Result<Expected, String>,
    load_new: &dyn Fn() -> Result<Expected, String>,
    expected_id: &str,
    start: bool,
) -> Result<Value, String> {
    let by_name = format!("{API}/containers/{}/json", new.name);
    let (status, inspected) = engine.request("GET", &by_name, None)?;
    match status {
        200 => {}
        404 => return Err("kiri_rollout_member_container_missing_use_kiri_provision".into()),
        n => return Err(format!("kiri_provision_engine_http_{n}")),
    }
    let id = inspected["Id"]
        .as_str()
        .filter(|id| container_id(id))
        .ok_or("kiri_rollout_container_id_missing")?
        .to_owned();
    let replaced = if verify_inspect(&inspected, new).is_ok() {
        // Same-target retry (or response loss after a completed replacement).
        false
    } else {
        // The old profile is only evaluated when the target is not already installed.
        let old = load_old()?;
        if !same_identity(&old, new) {
            return Err("kiri_rollout_member_identity_changed".into());
        }
        if old.create() == new.create() {
            return Err("kiri_rollout_no_change_requested".into());
        }
        if verify_inspect(&inspected, &old).is_err() {
            return Err("kiri_rollout_container_not_exact_old_or_target".into());
        }
        if id != expected_id {
            return Err("kiri_rollout_container_id_mismatch".into());
        }
        let by_id = format!("{API}/containers/{id}/json");
        if inspected["State"]["Running"] == true {
            // Even HTTP 500 may follow a completed stop. Every outcome is settled
            // by one fresh inspection, never by repeating the stop blindly.
            let _ = engine.request("POST", &format!("{API}/containers/{id}/stop?t=5"), None);
        }
        let (status, fresh) = engine
            .request("GET", &by_id, None)
            .map_err(|e| format!("kiri_rollout_old_state_unconfirmed:{e}"))?;
        if status != 200
            || fresh["Id"] != json!(id)
            || fresh["State"]["Running"] != false
            || verify_inspect(&fresh, &old).is_err()
        {
            return Err("kiri_rollout_old_not_stopped_or_changed".into());
        }
        // Re-validate host inputs and that the old/new inputs are unchanged.
        let stopped = |e| format!("kiri_rollout_old_retained_stopped:{e}");
        let (old2, new2) = (load_old().map_err(stopped)?, load_new().map_err(stopped)?);
        if old2.create() != old.create() || new2.create() != new.create() {
            return Err("kiri_rollout_old_retained_stopped:kiri_rollout_inputs_changed".into());
        }
        let remove = engine.request(
            "DELETE",
            &format!("{API}/containers/{id}?v=false&force=false&link=false"),
            None,
        );
        if !matches!(remove, Ok((204, _))) {
            // Lost acknowledgement is accepted only if the exact ID is gone.
            let (after, _) = engine
                .request("GET", &by_id, None)
                .map_err(|e| format!("kiri_rollout_remove_outcome_unconfirmed:{e}"))?;
            if after != 404 {
                return Err("kiri_rollout_remove_failed_old_retained_stopped".into());
            }
        }
        true
    };
    let state = reconcile(engine, new, start).map_err(|e| {
        if replaced {
            format!("kiri_rollout_target_incomplete_old_removed:{e}")
        } else {
            format!("kiri_rollout_existing_target_incomplete:{e}")
        }
    })?;
    let finalized = engine.required("GET", &by_name, None)?;
    verify_inspect(&finalized, new)?;
    Ok(
        json!({"profile":PROFILE,"container":new.name,"account":new.account,
        "generation":new.generation,"uid":new.uid,"replaced":replaced,
        "old_container_id":expected_id,"container_id":finalized["Id"],"state":state,
        "data_preserved":true,"readiness":"not_checked","production":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::super::tests::{expected, fake, inspect};
    use super::*;

    const OLD_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const NEW_ID: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn target() -> Expected {
        let mut e = expected();
        e.image = format!("sha256:{}", "b".repeat(64));
        e.vault_origin = Some("https://vault.example".into());
        e
    }
    fn with_id(mut v: Value, id: &str) -> Value {
        v["Id"] = json!(id);
        v
    }
    fn run_rollout(
        engine: &Engine,
        old: &Expected,
        new: &Expected,
        start: bool,
    ) -> Result<Value, String> {
        let (o, n) = (old.clone(), new.clone());
        rollout(
            engine,
            new,
            &move || Ok(o.clone()),
            &move || Ok(n.clone()),
            OLD_ID,
            start,
        )
    }
    fn count(requests: &[(String, Value)], prefix: &str) -> usize {
        requests
            .iter()
            .filter(|(l, _)| l.starts_with(prefix))
            .count()
    }

    #[test]
    fn replaces_only_exact_old_by_id_and_preserves_state_mounts() {
        let (old, new) = (expected(), target());
        let (_d, engine, worker) = fake(vec![
            (200, with_id(inspect(&old, true), OLD_ID)),
            (204, Value::Null),
            (200, with_id(inspect(&old, false), OLD_ID)),
            (204, Value::Null),
            (404, json!({})),
            (201, json!({"Id":NEW_ID,"Warnings":[]})),
            (200, with_id(inspect(&new, false), NEW_ID)),
            (204, Value::Null),
            (200, with_id(inspect(&new, true), NEW_ID)),
            (200, with_id(inspect(&new, true), NEW_ID)),
        ]);
        let out = run_rollout(&engine, &old, &new, true).unwrap();
        assert_eq!(out["replaced"], true);
        assert_eq!(out["container_id"], NEW_ID);
        let r = worker.join().unwrap();
        assert!(r[1].0.contains(&format!("/containers/{OLD_ID}/stop")));
        assert!(r[3].0.starts_with(&format!(
            "DELETE {API}/containers/{OLD_ID}?v=false&force=false"
        )));
        assert_eq!(count(&r, "DELETE "), 1);
        // The new create has the identical data/IPC/scratch binds and only intended changes.
        assert_eq!(
            r[5].1["HostConfig"]["Binds"],
            old.create()["HostConfig"]["Binds"]
        );
        assert_eq!(r[5].1["Image"], json!(new.image));
        assert_eq!(r[5].1["User"], old.create()["User"]);
        assert_eq!(r[5].1["Labels"], old.create()["Labels"]);
    }

    #[test]
    fn refuses_foreign_altered_or_wrong_id_container_without_writes() {
        let (old, new) = (expected(), target());
        let mut fixtures = Vec::new();
        let mut v = with_id(inspect(&old, true), OLD_ID);
        v["Config"]["Labels"]["org.moosuri.account"] = json!("someone-else");
        fixtures.push(v);
        let mut v = with_id(inspect(&old, true), OLD_ID);
        v["Mounts"][0]["Source"] = json!("/srv/foreign");
        fixtures.push(v);
        let mut v = with_id(inspect(&old, true), OLD_ID);
        v["Mounts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"Destination":"/x"}));
        fixtures.push(v);
        fixtures.push(with_id(inspect(&old, true), NEW_ID)); // not the operator-approved ID
        fixtures.push(inspect(&old, true)); // no ID at all
        for v in fixtures {
            let (_d, engine, worker) = fake(vec![(200, v)]);
            assert!(run_rollout(&engine, &old, &new, true).is_err());
            assert_eq!(worker.join().unwrap().len(), 1);
        }
    }

    #[test]
    fn identity_generation_uid_data_or_noop_changes_are_refused_before_stop() {
        let (old, new) = (expected(), target());
        let mut bad = Vec::new();
        for f in 0..4 {
            let mut n = new.clone();
            match f {
                0 => n.generation = 2,
                1 => n.uid = 10002,
                2 => n.data = "/srv/other/data".into(),
                _ => n.memory *= 2,
            }
            bad.push(n);
        }
        for n in bad {
            let (_d, engine, worker) = fake(vec![(200, with_id(inspect(&old, true), OLD_ID))]);
            assert_eq!(
                run_rollout(&engine, &old, &n, true).unwrap_err(),
                "kiri_rollout_member_identity_changed"
            );
            assert_eq!(worker.join().unwrap().len(), 1);
        }
        assert!(same_identity(&old, &new));
    }

    #[test]
    fn same_target_retry_never_stops_deletes_or_recreates() {
        let (old, new) = (expected(), target());
        let (_d, engine, worker) = fake(vec![
            (200, with_id(inspect(&new, true), NEW_ID)),
            (200, with_id(inspect(&new, true), NEW_ID)),
            (200, with_id(inspect(&new, true), NEW_ID)),
        ]);
        // The old profile need not even be loadable once the target is installed.
        let n = new.clone();
        let out = rollout(
            &engine,
            &new,
            &|| panic!("old profile must not be evaluated"),
            &move || Ok(n.clone()),
            OLD_ID,
            true,
        )
        .unwrap();
        assert_eq!(out["replaced"], false);
        assert_eq!(out["container_id"], NEW_ID);
        let r = worker.join().unwrap();
        assert!(r.iter().all(|(l, _)| l.starts_with("GET ")));
        let _ = old;
    }

    #[test]
    fn lost_remove_and_create_acknowledgements_are_reconciled_without_repeat_writes() {
        let (old, new) = (expected(), target());
        let (_d, engine, worker) = fake(vec![
            (200, with_id(inspect(&old, false), OLD_ID)),
            (200, with_id(inspect(&old, false), OLD_ID)),
            (500, json!({"message":"lost delete ack"})),
            (404, json!({})),
            (404, json!({})),
            (500, json!({"message":"lost create ack"})),
            (200, with_id(inspect(&new, false), NEW_ID)),
            (200, with_id(inspect(&new, false), NEW_ID)),
        ]);
        let out = run_rollout(&engine, &old, &new, false).unwrap();
        assert_eq!(out["state"], "stopped");
        assert_eq!(out["readiness"], "not_checked");
        let r = worker.join().unwrap();
        assert_eq!(count(&r, "DELETE "), 1);
        assert_eq!(count(&r, "POST "), 1);
    }

    #[test]
    fn stop_error_is_reconciled_and_post_stop_validation_reports_retained_state() {
        let (old, new) = (expected(), target());
        let (_d, engine, worker) = fake(vec![
            (200, with_id(inspect(&old, true), OLD_ID)),
            (500, json!({})),
            (200, with_id(inspect(&old, false), OLD_ID)),
            (204, Value::Null),
            (404, json!({})),
            (201, json!({"Id":NEW_ID,"Warnings":[]})),
            (200, with_id(inspect(&new, false), NEW_ID)),
            (200, with_id(inspect(&new, false), NEW_ID)),
        ]);
        assert!(run_rollout(&engine, &old, &new, false).unwrap()["replaced"] == true);
        let requests = worker.join().unwrap();
        assert_eq!(count(&requests, "POST "), 2); // one stop, one create
        assert_eq!(count(&requests, "DELETE "), 1);

        for still_running in [false, true] {
            let (_d, engine, worker) = fake(vec![
                (200, with_id(inspect(&old, true), OLD_ID)),
                (500, json!({})),
                (200, with_id(inspect(&old, still_running), OLD_ID)),
            ]);
            let error = rollout(
                &engine,
                &new,
                &|| Ok(old.clone()),
                &|| Err("kiri_provision_registry_lookup_failed".into()),
                OLD_ID,
                true,
            )
            .unwrap_err();
            assert_eq!(
                error,
                if still_running {
                    "kiri_rollout_old_not_stopped_or_changed"
                } else {
                    "kiri_rollout_old_retained_stopped:kiri_provision_registry_lookup_failed"
                }
            );
            assert_eq!(count(&worker.join().unwrap(), "DELETE "), 0);
        }
    }

    #[test]
    fn failed_remove_retains_old_and_failed_target_start_is_truthful_without_rollback() {
        let (old, new) = (expected(), target());
        let (_d, engine, worker) = fake(vec![
            (200, with_id(inspect(&old, false), OLD_ID)),
            (200, with_id(inspect(&old, false), OLD_ID)),
            (500, json!({})),
            (200, with_id(inspect(&old, false), OLD_ID)),
        ]);
        assert_eq!(
            run_rollout(&engine, &old, &new, true).unwrap_err(),
            "kiri_rollout_remove_failed_old_retained_stopped"
        );
        assert_eq!(count(&worker.join().unwrap(), "POST "), 0);
        // Target start that does not reach running: error, one DELETE, no second create.
        let (_d, engine, worker) = fake(vec![
            (200, with_id(inspect(&old, false), OLD_ID)),
            (200, with_id(inspect(&old, false), OLD_ID)),
            (204, Value::Null),
            (404, json!({})),
            (201, json!({"Warnings":[]})),
            (200, with_id(inspect(&new, false), NEW_ID)),
            (500, json!({})),
            (200, with_id(inspect(&new, false), NEW_ID)),
        ]);
        let e = run_rollout(&engine, &old, &new, true).unwrap_err();
        assert!(e.starts_with("kiri_rollout_target_incomplete_old_removed:"));
        let r = worker.join().unwrap();
        assert_eq!((count(&r, "DELETE "), count(&r, "POST ")), (1, 2));
    }
}
