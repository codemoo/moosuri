//! Optional component delivery. No installation, execution, pairing or service mutation.
use super::*;
#[path = "../../../../tools/release/components.rs"]
mod contract;

#[derive(Clone, ValueEnum)]
pub enum Kind {
    CredentialBroker,
    VaultService,
}
impl Kind {
    fn name(&self) -> &'static str {
        match self {
            Self::CredentialBroker => "credential-broker",
            Self::VaultService => "vault-service",
        }
    }
}
#[derive(Subcommand)]
pub enum Command {
    /// Create an unsigned exact component manifest from a prebuilt native executable; sign externally.
    Manifest {
        #[arg(long, value_enum)]
        component: Kind,
        #[arg(long)]
        target: String,
        #[arg(long)]
        version: String,
        #[arg(long)]
        sequence: u64,
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    /// Authenticate a separate local component package without executing it.
    Verify {
        #[arg(long, value_enum)]
        component: Kind,
        #[arg(long)]
        target: String,
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        trust_key: PathBuf,
    },
    /// Fetch a signed component from an explicit version/component/target HTTPS directory.
    Download {
        #[arg(long, value_enum)]
        component: Kind,
        #[arg(long)]
        target: String,
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        trust_key: PathBuf,
        #[arg(long)]
        destination: PathBuf,
        #[arg(long)]
        tls_ca_file: Option<PathBuf>,
    },
}
fn pin(path: &Path) -> Result<Vec<u8>> {
    let bytes = read(path, 128, true)?;
    let key = decode_hex(std::str::from_utf8(&bytes).map_err(error)?.trim())?;
    if key.len() != 32 {
        return Err("installation_public_key_length".into());
    }
    Ok(key)
}
fn authenticate(
    bytes: &[u8],
    sig: &[u8],
    key: &[u8],
    kind: &Kind,
    target: &str,
) -> Result<serde_json::Value> {
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(bytes, sig)
        .map_err(|_| "installation_signature_invalid")?;
    let manifest: serde_json::Value = parse(bytes)?;
    contract::validate(&manifest, kind.name(), target).map_err(str::to_owned)?;
    Ok(manifest)
}
fn local(root: &Path, key: &Path, kind: &Kind, target: &str) -> Result<()> {
    private_dir(root)?;
    let bytes = read(&root.join("manifest.json"), 16384, true)?;
    let sig = read(&root.join("manifest.ed25519"), 64, true)?;
    let m = authenticate(&bytes, &sig, &pin(key)?, kind, target)?;
    let name = contract::binary(kind.name()).map_err(str::to_owned)?;
    // No unrecognized files, including unsigned claims/configuration/hooks, are
    // admitted to the authenticated package. Symlinks fail the normal read checks.
    let mut allowed = vec!["manifest.json", "manifest.ed25519", name];
    if m.get("provenance").is_some() {
        allowed.extend([release_evidence::PROVENANCE, release_evidence::INVENTORY]);
    }
    for entry in fs::read_dir(root).map_err(error)? {
        let name = entry.map_err(error)?.file_name();
        if !allowed.iter().any(|expected| name == *expected) {
            return Err("component_bundle_unrecognized_entry".into());
        }
    }
    let binary = read(&root.join(name), contract::BINARY_CAP, true)?;
    contract::verify_binary(&m, &binary).map_err(str::to_owned)?;
    if let Some(reference) = m.get("provenance") {
        let p = read(
            &root.join(release_evidence::PROVENANCE),
            release_evidence::PROVENANCE_CAP,
            true,
        )?;
        let i = read(
            &root.join(release_evidence::INVENTORY),
            release_evidence::INVENTORY_CAP,
            true,
        )?;
        release_evidence::verify(reference, &p, &i, &m).map_err(str::to_owned)?;
    }
    Ok(())
}
async fn download(
    base: &url::Url,
    key: &Path,
    destination: &Path,
    ca: Option<&Path>,
    kind: &Kind,
    target: &str,
) -> Result<()> {
    if !contract::TARGETS.contains(&target) {
        return Err("component_bundle_target_refused".into());
    }
    path_checked(destination)?;
    let parent = destination.parent().ok_or("installation_parent_required")?;
    private_dir(parent)?;
    match fs::symlink_metadata(destination) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        _ => return Err("installation_download_new_destination_required".into()),
    }
    // Component and target are explicit caller intent, not inferred from host or URL.
    // Require exactly the final version/component/target directory layout, so a
    // differently named/versioned mirror path cannot silently serve another package.
    let parts: Vec<_> = base
        .path_segments()
        .ok_or("component_bundle_url_mismatch")?
        .filter(|s| !s.is_empty())
        .collect();
    if parts.len() < 3 || parts[parts.len() - 1] != target || parts[parts.len() - 2] != kind.name()
    {
        return Err("component_bundle_url_mismatch".into());
    }
    let key_bytes = pin(key)?;
    let client = super::download::client(ca)?;
    let mut staging = super::download::Staging::new(parent)?;
    let bytes = super::download::fetch(&client, base, "manifest.json", 16384, None).await?;
    let sig = super::download::fetch(&client, base, "manifest.ed25519", 64, Some(64)).await?;
    let m = authenticate(&bytes, &sig, &key_bytes, kind, target)?;
    let version = m["version"]
        .as_str()
        .ok_or("component_bundle_manifest_invalid")?;
    if parts[parts.len() - 3] != version && parts[parts.len() - 3] != format!("v{version}") {
        return Err("component_bundle_url_mismatch".into());
    }
    staging.put("manifest.json", &bytes)?;
    staging.put("manifest.ed25519", &sig)?;
    if let Some(reference) = m.get("provenance") {
        use release_evidence::{INVENTORY, INVENTORY_CAP, PROVENANCE, PROVENANCE_CAP};
        let size = reference["size"]
            .as_u64()
            .ok_or("release_provenance_invalid")?;
        let p = super::download::fetch(&client, base, PROVENANCE, size, Some(size)).await?;
        release_evidence::check_reference(reference, &p, PROVENANCE, PROVENANCE_CAP)
            .map_err(str::to_owned)?;
        let value: serde_json::Value = parse(&p)?;
        let inventory = &value["inventory"];
        release_evidence::validate_reference(inventory, INVENTORY, INVENTORY_CAP)
            .map_err(str::to_owned)?;
        let size = inventory["size"]
            .as_u64()
            .ok_or("release_provenance_invalid")?;
        let i = super::download::fetch(&client, base, INVENTORY, size, Some(size)).await?;
        release_evidence::verify(reference, &p, &i, &m).map_err(str::to_owned)?;
        staging.put(PROVENANCE, &p)?;
        staging.put(INVENTORY, &i)?;
    }
    let name = contract::binary(kind.name()).map_err(str::to_owned)?;
    let size = m["size"]
        .as_u64()
        .ok_or("component_bundle_manifest_invalid")?;
    let binary = super::download::fetch(&client, base, name, size, Some(size)).await?;
    contract::verify_binary(&m, &binary).map_err(str::to_owned)?;
    staging.put(name, &binary)?;
    drop(binary);
    local(&staging.path, key, kind, target)?;
    sync(&staging.path)?;
    super::download::publish(&staging.path, destination)?;
    staging.published = true;
    sync(parent)
}
pub(super) fn run(command: &Command) -> Result<()> {
    match command {
        Command::Manifest {
            component,
            target,
            version,
            sequence,
            binary,
            output,
        } => {
            path_checked(output)?;
            private_dir(output.parent().ok_or("installation_parent_required")?)?;
            let bytes = read(binary, contract::BINARY_CAP, true)?;
            let manifest = contract::make(component.name(), target, version, *sequence, &bytes)
                .map_err(str::to_owned)?;
            create(output, &json(&manifest)?, false)?;
            sync(output.parent().ok_or("installation_parent_required")?)?;
            println!(
                "Unsigned component manifest created. Component/version/ABI are packager claims; sign exact final bytes externally."
            );
        }
        Command::Verify {
            component,
            target,
            bundle,
            trust_key,
        } => {
            local(bundle, trust_key, component, target)?;
            println!(
                "Pinned component bytes verified. No code executed, deployment performed or device trust established."
            );
        }
        Command::Download {
            component,
            target,
            base_url,
            trust_key,
            destination,
            tls_ca_file,
        } => {
            let base = super::download::base_url(base_url)?;
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(error)?;
                        runtime.block_on(async {
                            tokio::time::timeout(
                                super::download::TOTAL_TIMEOUT,
                                download(
                                    &base,
                                    trust_key,
                                    destination,
                                    tls_ca_file.as_deref(),
                                    component,
                                    target,
                                ),
                            )
                            .await
                            .map_err(|_| "installation_download_timeout".to_owned())?
                        })
                    })
                    .join()
                    .map_err(|_| "installation_download_failed".to_owned())?
            })?;
            println!(
                "Authenticated component package downloaded. Run/configure separately on its intended host; no device trust established."
            );
        }
    }
    Ok(())
}
