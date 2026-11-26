//! Installation-only startup acceptance. No account credentials or remote pages.
use super::*;

impl CapsuleReader {
    /// Return a usable config only after the pinned worker actually launches its
    /// browser under kernel confinement and its owned container is removed.
    pub async fn verify_installation(
        mut config: CapsuleConfig,
        google: bool,
        cancel: &CancellationToken,
    ) -> Result<(CapsuleConfig, Vec<u8>), BrowserError> {
        if !cfg!(target_os = "linux") || config.engine != Engine::Kiri {
            return Err(BrowserError::IsolationRequired);
        }
        // Provisional admission is confined to this function and never persisted.
        config.verified = true;
        config.smoke_evidence_sha256 = format!("{:x}", Sha256::digest(b"installation-probe-pending"));
        let reader = Self::new(config.clone())?;
        let local = cancel.child_token();
        let _drop = CancelOnDrop(local.clone());
        let task = tokio::spawn(async move {
            reader.validate_resources(&local).await?;
            empty_docker_config(Path::new(DOCKER_CONFIG))?;
            let identity = Uuid::new_v4();
            let tenant = Uuid::new_v4();
            let name = format!("moosuri-install-probe-{}", identity.simple());
            let mut command = reader.worker_command_mode(tenant, identity, &name, None, google);
            if google { command.arg("--google-installation-probe"); }
            let mut child = command.stdin(Stdio::piped()).stdout(Stdio::piped())
                .stderr(Stdio::null()).kill_on_drop(true).spawn().map_err(|_| BrowserError::Setup)?;
            let mut observed = false;
            let operation = async {
                let mut stdout = BufReader::new(child.stdout.take().ok_or(BrowserError::Setup)?);
                let ready = read_line(&mut stdout, 2048).await?;
                if ready["ready"] != true || ready["tenant"] != tenant.to_string()
                    || ready["javascript_enabled"] != config.javascript_enabled
                    || (google && ready["chrome"] != "158.0.8127.88") {
                    return Err(BrowserError::Setup);
                }
                observed = true;
                Ok(())
            };
            let result = tokio::select! { biased;
                _ = local.cancelled() => Err(BrowserError::Cancelled),
                result = tokio::time::timeout(Duration::from_secs(50), operation) => result.unwrap_or(Err(BrowserError::Timeout)),
            };
            reader.cleanup(&mut child, &name, identity, observed, false).await?;
            result?;
            let evidence = serde_json::to_vec_pretty(&json!({
                "format": 1, "probe": "actual-browser-start-and-owned-cleanup-v1",
                "engine": "kiri-0.2.2", "image_id": config.image_id,
                "network_id": config.network_id, "proxy_container_id": config.proxy_container_id,
                "owner": config.owner, "role": if google { "google-auth" } else { "browser" },
                "kernel_confinement": true, "browser_started": true, "owned_cleanup": true,
                "google_chrome_version": if google { Some("158.0.8127.88") } else { None },
            })).map_err(|_| BrowserError::Setup)?;
            config.smoke_evidence_sha256 = format!("{:x}", Sha256::digest(&evidence));
            Ok((config, evidence))
        });
        task.await.map_err(|_| BrowserError::Setup)?
    }
}
