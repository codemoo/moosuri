//! Fixed-file HTTPS transport. The local bundle verifier remains the trust contract.
use super::*;
use std::time::Duration;
use url::Url;

const MANIFEST_CAP: u64 = 16384;
const TOTAL_TIMEOUT: Duration = Duration::from_secs(300);
const FILE_TIMEOUT: Duration = Duration::from_secs(120);
const FILES: [&str; 5] = [
    "manifest.json",
    "manifest.ed25519",
    "moosuri",
    release_evidence::PROVENANCE,
    release_evidence::INVENTORY,
];

// Reject normalization, userinfo and encoded separators before URL parsing. Only
// literal public path segments are supported; secrets must never be URL inputs.
fn base_url(input: &str) -> Result<Url> {
    if input.len() > 2048
        || !input.is_ascii()
        || input
            .bytes()
            .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        || input.contains(['%', '\\', '@', '?', '#'])
        || !input.starts_with("https://")
        || !input.ends_with('/')
        || input.split('/').any(|s| matches!(s, "." | ".."))
    {
        return Err("installation_download_https_base_required".into());
    }
    let url = Url::parse(input).map_err(|_| "installation_download_https_base_required")?;
    if url.host_str().is_none()
        || url.path() == "/"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path_segments().is_none_or(|mut parts| {
            parts.any(|s| {
                !s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-~+".contains(&b))
            })
        })
    {
        return Err("installation_download_https_base_required".into());
    }
    Ok(url)
}
fn version_matches(url: &Url, manifest: &Manifest) -> Result<()> {
    let tagged = format!("v{}", manifest.version);
    if !url
        .path_segments()
        .is_some_and(|mut parts| parts.any(|part| part == manifest.version || part == tagged))
    {
        return Err("installation_download_version_path_mismatch".into());
    }
    Ok(())
}

// A new staging directory is the only cleanup authority. Never recurse, remove
// a replaced inode or clean an operator-selected existing destination.
struct Staging {
    path: PathBuf,
    identity: (u64, u64),
    files: Vec<(String, u64, u64)>,
    published: bool,
}
impl Staging {
    fn new(parent: &Path) -> Result<Self> {
        let path = parent.join(format!(".moosuri-download-{}", uuid::Uuid::new_v4()));
        path_checked(&path)?;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(error)?;
        let m = fs::symlink_metadata(&path).map_err(error)?;
        let staging = Self {
            path,
            identity: (m.dev(), m.ino()),
            files: vec![],
            published: false,
        };
        // Establish cleanup ownership before fallible ACL/durability checks.
        private_dir(&staging.path)?;
        sync(parent)?;
        Ok(staging)
    }
    fn put(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
        if !FILES.contains(&name) {
            return Err("installation_download_filename_refused".into());
        }
        let path = self.path.join(name);
        // Register ownership before writing so a failed/partial write can be cleaned.
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(if name == "moosuri" { 0o700 } else { 0o600 })
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(error)?;
        let m = file.metadata().map_err(error)?;
        self.files.push((name.to_owned(), m.dev(), m.ino()));
        acl_supported(&file, false)?;
        file.write_all(bytes)
            .and_then(|()| file.sync_all())
            .map_err(error)
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        if !fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.is_dir() && (m.dev(), m.ino()) == self.identity)
        {
            return;
        }
        for (name, dev, ino) in &self.files {
            let path = self.path.join(name);
            if fs::symlink_metadata(&path)
                .is_ok_and(|m| m.is_file() && m.dev() == *dev && m.ino() == *ino && m.nlink() == 1)
            {
                let _ = fs::remove_file(path);
            }
        }
        let _ = fs::remove_dir(&self.path); // Fails closed on any unrelated entry.
    }
}

// Plain rename may overwrite an empty destination directory. Both supported
// platforms provide an atomic no-replace publication primitive instead.
fn publish(source: &Path, destination: &Path) -> Result<()> {
    use std::ffi::CString;
    let source = CString::new(source.as_os_str().as_encoded_bytes()).map_err(error)?;
    let destination = CString::new(destination.as_os_str().as_encoded_bytes()).map_err(error)?;
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let result = -1;
    if result != 0 {
        return Err("installation_download_publish_refused".into());
    }
    Ok(())
}

async fn fetch(
    client: &reqwest::Client,
    base: &Url,
    name: &str,
    cap: u64,
    exact: Option<u64>,
) -> Result<Vec<u8>> {
    if !FILES.contains(&name) {
        return Err("installation_download_filename_refused".into());
    }
    let url = base
        .join(name)
        .map_err(|_| "installation_download_https_base_required")?;
    let mut response = client
        .get(url)
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .send()
        .await
        .map_err(|_| "installation_download_transport_failed")?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("installation_download_http_status_refused".into());
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .is_some_and(|v| v.as_bytes() != b"identity")
    {
        return Err("installation_download_encoding_refused".into());
    }
    if let Some(size) = response.content_length()
        && (size > cap || exact.is_some_and(|n| n != size))
    {
        return Err("installation_download_size_mismatch".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "installation_download_transport_failed")?
    {
        if chunk.len() as u64 > cap.saturating_sub(bytes.len() as u64) {
            return Err("installation_download_size_mismatch".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    if exact.is_some_and(|n| n != bytes.len() as u64) {
        return Err("installation_download_size_mismatch".into());
    }
    Ok(bytes)
}

async fn download(
    base: &Url,
    key_path: &Path,
    destination: &Path,
    ca: Option<&Path>,
) -> Result<()> {
    // Complete local admission before making requests or creating staging.
    path_checked(destination)?;
    let parent = destination.parent().ok_or("installation_parent_required")?;
    private_dir(parent)?;
    match fs::symlink_metadata(destination) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        _ => return Err("installation_download_new_destination_required".into()),
    }
    let keytext = read(key_path, 128, true)?;
    let key = decode_hex(std::str::from_utf8(&keytext).map_err(error)?.trim())?;
    if key.len() != 32 {
        return Err("installation_public_key_length".into());
    }
    let mut builder = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .connect_timeout(Duration::from_secs(15))
        .timeout(FILE_TIMEOUT);
    if let Some(path) = ca {
        let pem = read(path, 65536, true)?;
        let cert =
            reqwest::Certificate::from_pem(&pem).map_err(|_| "installation_download_ca_invalid")?;
        builder = builder.tls_certs_merge([cert]);
    }
    let client = builder
        .build()
        .map_err(|_| "installation_download_transport_failed")?;
    let mut staging = Staging::new(parent)?;
    let bytes = fetch(&client, base, "manifest.json", MANIFEST_CAP, None).await?;
    let sig = fetch(&client, base, "manifest.ed25519", 64, Some(64)).await?;
    signature::UnparsedPublicKey::new(&signature::ED25519, &key)
        .verify(&bytes, &sig)
        .map_err(|_| "installation_signature_invalid")?;
    let manifest: Manifest = parse(&bytes)?;
    validate_manifest(&manifest)?;
    version_matches(base, &manifest)?;
    staging.put("manifest.json", &bytes)?;
    staging.put("manifest.ed25519", &sig)?;
    if let Some(reference) = &manifest.provenance {
        use release_evidence::{INVENTORY, INVENTORY_CAP, PROVENANCE, PROVENANCE_CAP};
        let provenance_size = reference["size"]
            .as_u64()
            .ok_or("release_provenance_invalid")?;
        let p = fetch(
            &client,
            base,
            PROVENANCE,
            provenance_size,
            Some(provenance_size),
        )
        .await?;
        release_evidence::check_reference(reference, &p, PROVENANCE, PROVENANCE_CAP)
            .map_err(str::to_owned)?;
        let parsed: serde_json::Value = parse(&p)?;
        let inventory = &parsed["inventory"];
        release_evidence::validate_reference(inventory, INVENTORY, INVENTORY_CAP)
            .map_err(str::to_owned)?;
        let inventory_size = inventory["size"]
            .as_u64()
            .ok_or("release_provenance_invalid")?;
        let i = fetch(
            &client,
            base,
            INVENTORY,
            inventory_size,
            Some(inventory_size),
        )
        .await?;
        release_evidence::verify(
            reference,
            &p,
            &i,
            &serde_json::to_value(&manifest).map_err(error)?,
        )
        .map_err(str::to_owned)?;
        staging.put(PROVENANCE, &p)?;
        staging.put(INVENTORY, &i)?;
    }
    let binary = fetch(&client, base, "moosuri", manifest.size, Some(manifest.size)).await?;
    if hash(&binary) != manifest.sha256 {
        return Err("installation_binary_hash_mismatch".into());
    }
    staging.put("moosuri", &binary)?;
    drop(binary);
    // Reuse the same complete signed bundle/provenance contract used by verify,
    // install and update, without stage()/probe() or executing downloaded bytes.
    bundle(&staging.path, key_path)?;
    sync(&staging.path)?;
    publish(&staging.path, destination)?;
    staging.published = true;
    sync(parent)
}

pub(super) fn run(base: &str, key: &Path, destination: &Path, ca: Option<&Path>) -> Result<()> {
    let base = base_url(base)?;
    // Installation dispatch already runs inside Tokio. Isolate a current-thread
    // runtime without changing the existing synchronous local installer API.
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(error)?;
                runtime.block_on(async {
                    tokio::time::timeout(TOTAL_TIMEOUT, download(&base, key, destination, ca))
                        .await
                        .map_err(|_| "installation_download_timeout".to_owned())?
                })
            })
            .join()
            .map_err(|_| "installation_download_failed".to_owned())?
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> tempfile::TempDir {
        tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
    }
    #[test]
    fn signed_download_cleanup_is_owned_and_nonrecursive_publication_never_replaces() {
        let root = root();
        let mut staging = Staging::new(root.path()).unwrap();
        staging.put("manifest.json", b"partial").unwrap();
        let path = staging.path.clone();
        drop(staging);
        assert!(!path.exists());

        let mut staging = Staging::new(root.path()).unwrap();
        staging.put("manifest.json", b"original").unwrap();
        let path = staging.path.clone();
        // Retain the old inode elsewhere; the replacement is outside cleanup authority.
        fs::rename(
            path.join("manifest.json"),
            root.path().join("retained-original"),
        )
        .unwrap();
        create(&path.join("manifest.json"), b"replacement", false).unwrap();
        create(&path.join("unrelated"), b"retained", false).unwrap();
        drop(staging);
        assert_eq!(
            fs::read(path.join("manifest.json")).unwrap(),
            b"replacement"
        );
        assert_eq!(fs::read(path.join("unrelated")).unwrap(), b"retained");
        assert_eq!(
            fs::read(root.path().join("retained-original")).unwrap(),
            b"original"
        );

        let staging = Staging::new(root.path()).unwrap();
        let destination = root.path().join("existing-empty");
        new_dir(&destination).unwrap();
        let m = fs::metadata(&destination).unwrap();
        assert!(publish(&staging.path, &destination).is_err());
        assert_eq!(fs::metadata(&destination).unwrap().ino(), m.ino());
    }
    #[test]
    fn signed_download_url_policy_does_not_normalize_or_accept_authority_secrets() {
        for input in [
            "http://mirror.example.test/v1.2.3/",
            "https://user:secret@mirror.example.test/v1.2.3/",
            "https://mirror.example.test/v1.2.3/?token=secret",
            "https://mirror.example.test/v1.2.3/#fragment",
            "https://mirror.example.test/v1.2.3/../",
            "https://mirror.example.test/v1.2.3/%2e%2e/",
            "https://mirror.example.test/v1.2.3/\\escape/",
            " https://mirror.example.test/v1.2.3/",
            "https://mirror.example.test/",
        ] {
            assert!(base_url(input).is_err(), "accepted {input}");
        }
        assert!(base_url("https://mirror.example.test/releases/v1.2.3/linux-aarch64/").is_ok());
    }
}
