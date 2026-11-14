//! Optional unmodified official CLI. No subscription bearer token adapter exists.
pub mod protocol;
#[cfg(unix)]
mod sandbox;
#[cfg(unix)]
pub use sandbox::{Profile, helper_main};

#[cfg(any(test, feature = "test-fixtures"))]
pub mod fixture;
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        CancellationToken,
        protocol::{ModelEvent, TextMessage, TextRequest, TextRole},
    };
    fn request() -> TextRequest {
        TextRequest {
            model: "claude-fixture-v1".into(),
            effort: None,
            max_output_tokens: 2048,
            messages: vec![TextMessage {
                role: TextRole::User,
                text: "/model no @/private/canary".into(),
                phase: None,
            }],
        }
    }
    #[tokio::test]
    async fn actual_child_supervisor_requires_clean_protocol_and_os_exit() {
        let f = fixture::Fixture::new();
        for behavior in ["ok", "truncated", "suffix", "nonzero"] {
            f.behavior(behavior);
            let (tx, mut rx) = tokio::sync::mpsc::channel(16);
            let r = f
                .profile
                .stream_text(
                    "00000000-0000-4000-8000-000000000001",
                    &request(),
                    &tx,
                    &CancellationToken::new(),
                )
                .await;
            assert_eq!(r.is_ok(), behavior == "ok", "{behavior}");
            let mut complete = false;
            while let Ok(e) = rx.try_recv() {
                complete |= matches!(e, ModelEvent::Finished { .. });
            }
            assert_eq!(complete, behavior == "ok");
        }
        assert_eq!(f.launches(), 4);
    }
    #[tokio::test]
    async fn cancellation_bounds_real_child_and_container_cleanup() {
        let f = fixture::Fixture::new();
        f.behavior("hold");
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let token = CancellationToken::new();
        let request = request();
        let operation = f.profile.stream_text(
            "00000000-0000-4000-8000-000000000001",
            &request,
            &tx,
            &token,
        );
        tokio::pin!(operation);
        tokio::time::timeout(std::time::Duration::from_secs(5),async{loop{tokio::select!{r=&mut operation=>panic!("premature {r:?}"),_=tokio::time::sleep(std::time::Duration::from_millis(20))=>{if f.directory.path().join("entered").exists(){break}}}}}).await.unwrap();
        token.cancel();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(3), operation)
                .await
                .unwrap()
                .is_err()
        );
        assert!(f.directory.path().join("removed").exists());
        assert_eq!(f.launches(), 1);
    }
    #[tokio::test]
    async fn restart_reaps_full_labelled_container_identity() {
        let f = fixture::Fixture::new();
        std::fs::write(f.directory.path().join("stale"), b"stale").unwrap();
        f.profile.cleanup_stale().await.unwrap();
        assert!(f.directory.path().join("removed").exists());
        assert_eq!(f.launches(), 0);
    }
    #[tokio::test]
    async fn byte_pin_and_symlink_fail_before_spawn() {
        let f = fixture::Fixture::new();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                f.directory.path().join("official"),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
        }
        std::fs::write(f.directory.path().join("official"), b"changed").unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        assert!(
            f.profile
                .stream_text(
                    "00000000-0000-4000-8000-000000000001",
                    &request(),
                    &tx,
                    &CancellationToken::new()
                )
                .await
                .is_err()
        );
        assert_eq!(f.launches(), 0);
    }
}
#[cfg(test)]
mod retirement_tests {
    use super::*;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    #[tokio::test]
    async fn retired_auth_environment_is_erased_and_stale_native_reference_cannot_recreate_it() {
        let f = fixture::Fixture::new();
        let id = "00000000-0000-4000-8000-000000000001";
        let path = f.directory.path().join("auth").join(id);
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(
            path.join(".credentials.json"),
            b"SYNTHETIC_PRIVATE_AUTH_CANARY",
        )
        .unwrap();
        f.profile.retire_environment(id).await.unwrap();
        assert!(!path.exists());
        f.profile.retire_environment(id).await.unwrap();
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let request = crate::model::protocol::TextRequest {
            model: "claude-fixture-v1".into(),
            effort: None,
            max_output_tokens: 2048,
            messages: vec![crate::model::protocol::TextMessage {
                role: crate::model::protocol::TextRole::User,
                text: "Hello".into(),
                phase: None,
            }],
        };
        assert!(
            f.profile
                .stream_text(id, &request, &tx, &crate::model::CancellationToken::new())
                .await
                .is_err()
        );
        assert!(!path.exists());
        assert_eq!(f.launches(), 0);
    }
    #[tokio::test]
    async fn cancellation_interrupts_cross_process_auth_lock_wait() {
        use std::os::fd::AsRawFd;
        let f = fixture::Fixture::new();
        let id = "00000000-0000-4000-8000-000000000001";
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(f.directory.path().join("auth").join(format!("{id}.lock")))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        let token = crate::model::CancellationToken::new();
        let operation = f.profile.login_with_cancel(id, &token);
        tokio::pin!(operation);
        tokio::select! {r=&mut operation=>panic!("unexpected {r:?}"),_=tokio::time::sleep(std::time::Duration::from_millis(50))=>token.cancel()};
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), operation)
                .await
                .unwrap()
                .is_err()
        );
        assert_eq!(f.launches(), 0);
    }
}
#[cfg(test)]
mod login_cleanup_tests {
    use super::*;
    use crate::model::CancellationToken;
    use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
    #[tokio::test]
    async fn held_login_cancellation_keeps_auth_lock_until_container_remove_ack() {
        let f = fixture::Fixture::new();
        f.behavior("login-hold");
        let id = "00000000-0000-4000-8000-000000000001";
        let cancel = CancellationToken::new();
        let operation = f.profile.login_with_cancel(id, &cancel);
        tokio::pin!(operation);
        for marker in ["entered", "remove-entered"] {
            tokio::time::timeout(std::time::Duration::from_secs(5),async{loop{
                tokio::select!{r=&mut operation=>panic!("login completed before {marker}: {r:?}"),_=tokio::time::sleep(std::time::Duration::from_millis(10))=>{if f.directory.path().join(marker).exists(){break}}}
            }}).await.unwrap();
            if marker == "entered" {
                cancel.cancel();
            }
        }
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .mode(0o600)
            .open(f.directory.path().join("auth").join(format!("{id}.lock")))
            .unwrap();
        assert_ne!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        std::fs::write(f.directory.path().join("allow-remove"), b"allow").unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(2), operation)
                .await
                .unwrap()
                .is_err()
        );
        assert!(f.directory.path().join("removed").exists());
        assert_eq!(
            unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        assert_eq!(f.launches(), 1);
    }
}
