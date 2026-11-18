//! Operator registration inside one verified member volume. Member consent uses the web UI.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use fs2::FileExt;
use moosuri_connectors::gmail::OAuthClient;
use moosuri_store::personal_volume::GWS_DIRECTORY;
use rand::{RngCore, rngs::OsRng};
use serde::Serialize;
use serde_json::value::RawValue;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Serialize)]
struct Registration<'a> {
    app_revision: Uuid,
    redirect_uri: &'a str,
    web_client: &'a RawValue,
    capability_key: &'a str,
}

pub(super) fn configure(
    directory: &Path,
    redirect_uri: &str,
    input: impl Read,
) -> Result<(), &'static str> {
    let invalid = "invalid_google_web_client";
    let redirect = url::Url::parse(redirect_uri).map_err(|_| invalid)?;
    if redirect.scheme() != "https" || redirect.path() != "/api/v1/mail/oauth/callback" {
        return Err(invalid);
    }
    let mut bytes = Zeroizing::new(String::new());
    input
        .take(16385)
        .read_to_string(&mut bytes)
        .map_err(|_| invalid)?;
    let app = OAuthClient::from_web_client_json(&bytes, redirect_uri).map_err(|_| invalid)?;
    let raw: &RawValue = serde_json::from_str(&bytes).map_err(|_| invalid)?;
    let failed = "member_google_registration_failed";
    let lock = File::open(directory).map_err(|_| failed)?;
    FileExt::try_lock_exclusive(&lock).map_err(|_| "member_google_registration_busy")?;
    let subdir = directory.join(GWS_DIRECTORY);
    match std::fs::DirBuilder::new().mode(0o700).create(&subdir) {
        Ok(()) => lock.sync_all().map_err(|_| failed)?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err(failed),
    }
    let meta = std::fs::symlink_metadata(&subdir).map_err(|_| failed)?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        return Err(failed);
    }
    let path = subdir.join("gws-client.json");
    match crate::personal_google::registered_client(&path) {
        Ok((_, old, _))
            if old.client_id == app.client_id
                && old.redirect_uri == app.redirect_uri
                && old.client_secret.expose() == app.client_secret.expose() =>
        {
            return Ok(());
        }
        Ok(_) | Err("google_setup_required") => (),
        Err(e) => return Err(e),
    }
    let mut key = Zeroizing::new([0u8; 32]);
    OsRng.try_fill_bytes(key.as_mut()).map_err(|_| failed)?;
    let encoded = Zeroizing::new(URL_SAFE_NO_PAD.encode(key.as_slice()));
    let registration = Registration {
        app_revision: Uuid::new_v4(),
        web_client: raw,
        redirect_uri,
        capability_key: &encoded,
    };
    let mut temp = tempfile::NamedTempFile::new_in(&subdir).map_err(|_| failed)?;
    serde_json::to_writer(&mut temp, &registration).map_err(|_| failed)?;
    temp.flush()
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| failed)?;
    temp.persist(path).map_err(|_| failed)?;
    File::open(subdir)
        .and_then(|d| d.sync_all())
        .map_err(|_| failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn registration_while_serving_is_private_to_the_verified_volume() {
        use moosuri_store::personal_volume::VolumeOwner;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
        let owner = VolumeOwner {
            account: Uuid::new_v4(),
            tenant: Uuid::new_v4(),
            volume: Uuid::new_v4(),
            generation: 1,
        };
        owner
            .initialize(
                &root,
                "A".into(),
                "a@example.test".into(),
                "synthetic-password".into(),
            )
            .await
            .unwrap();
        let store = owner.open_verified(&root).await.unwrap();
        let dir = owner.verified_member_directory(&root).unwrap();
        let redirect = "https://assistant.example.test/api/v1/mail/oauth/callback";
        let data=serde_json::json!({"web":{"client_id":"fixture","client_secret":"fixture","redirect_uris":[redirect]}}).to_string();
        configure(&dir, redirect, data.as_bytes()).unwrap();
        let other = VolumeOwner {
            account: Uuid::new_v4(),
            ..owner
        };
        assert!(other.verified_member_directory(&root).is_err());
        drop(store);
        let _restarted = owner.open_verified(&root).await.unwrap();
        assert!(
            crate::personal_google::registered_client(
                &dir.join(GWS_DIRECTORY).join("gws-client.json")
            )
            .is_ok()
        );
    }

    #[test]
    fn repeat_preserves_grants_until_the_registered_client_changes() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().canonicalize().unwrap();
        let redirect = "https://assistant.example.test/api/v1/mail/oauth/callback";
        let input = |id| {
            serde_json::json!({"web":{"client_id":id,"client_secret":"fixture","redirect_uris":[redirect]}}).to_string()
        };
        let path = dir.join(GWS_DIRECTORY).join("gws-client.json");
        configure(&dir, redirect, input("one").as_bytes()).unwrap();
        let original = std::fs::read(&path).unwrap();
        configure(&dir, redirect, input("one").as_bytes()).unwrap();
        assert_eq!(original, std::fs::read(&path).unwrap());
        configure(&dir, redirect, input("two").as_bytes()).unwrap();
        let current = std::fs::read(&path).unwrap();
        let data = |s: &[u8]| serde_json::from_slice::<serde_json::Value>(s).unwrap();
        for field in ["app_revision", "capability_key"] {
            assert_ne!(data(&original)[field], data(&current)[field]);
        }
        assert!(configure(&dir, redirect, vec![b' '; 16385].as_slice()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), current);
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(configure(&dir, redirect, input("two").as_bytes()).is_err());
    }
}
