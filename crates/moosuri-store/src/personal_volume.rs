//! First-use and restart boundary for a fresh account volume. The provisioner
//! supplies these identifiers independently of the volume (never from HTTP).
use super::*;
use fs2::FileExt;
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};

#[derive(Clone, Copy)]
pub struct VolumeOwner {
    pub account: Uuid,
    pub tenant: Uuid,
    pub volume: Uuid,
    pub generation: u64,
}

impl VolumeOwner {
    fn directory(self) -> Result<String> {
        if self.account.is_nil()
            || self.tenant.is_nil()
            || self.volume.is_nil()
            || !(1..=i64::MAX as u64).contains(&self.generation)
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(format!(
            "{}-{}-{}-g{}",
            self.volume, self.account, self.tenant, self.generation
        ))
    }

    fn open(self, root: &Path, initialize: bool) -> Result<Store> {
        let meta = fs::symlink_metadata(root).map_err(|_| StoreError::InvalidInput)?;
        if !meta.is_dir()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o077 != 0
            || fs::canonicalize(root).map_err(|_| StoreError::InvalidInput)? != root
        {
            return Err(StoreError::InvalidInput);
        }
        let directory = File::open(root).map_err(|_| StoreError::Unavailable)?;
        FileExt::try_lock_exclusive(&directory).map_err(|_| StoreError::Busy)?;
        let name = self.directory()?;
        let child = root.join(&name);
        let entries = fs::read_dir(root).map_err(|_| StoreError::Unavailable)?;
        let mut present = false;
        for entry in entries {
            let entry = entry.map_err(|_| StoreError::Unavailable)?;
            if entry.file_name() != name.as_str()
                || !entry
                    .file_type()
                    .map_err(|_| StoreError::Unavailable)?
                    .is_dir()
            {
                return Err(StoreError::Conflict);
            }
            present = true;
        }
        if !present {
            if !initialize {
                return Err(StoreError::NotFound);
            }
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&child)
                .map_err(|_| StoreError::Unavailable)?;
        }
        directory.sync_all().map_err(|_| StoreError::Unavailable)?;
        if !initialize && !child.join("assistant.sqlite").is_file() {
            return Err(StoreError::Corrupt);
        }
        // Store retains the exclusive writer lock and performs normal journal
        // admission. This path starts no network service or background runner.
        let admission = cutover::personal_admission(&child, self, initialize)?
            .ok_or(StoreError::Unavailable)?;
        Store::open_admitted(
            &child,
            moosuri_core::WebToolProfile::default(),
            Some(admission),
        )
    }

    /// Installer child only: prepare a fresh owner store or identify a completed
    /// retry under this UID's writer lock, without accepting a new password.
    pub async fn initialization_status(self, root: &Path, email: String) -> Result<bool> {
        let email = email_valid(email)?;
        let store = self.open(root, true)?;
        store.call(move |db| {
            let accounts:i64=db.conn.query_row("SELECT count(*) FROM accounts",[],|r|r.get(0))?;
            let tenants:i64=db.conn.query_row("SELECT count(*) FROM tenants",[],|r|r.get(0))?;
            let identity:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='personal_runtime_identity')",[],|r|r.get(0))?;
            if accounts==0 && tenants==0 && !identity {return Ok(false);}
            self.check_identity(&db.conn,Some(&email))?;Ok(true)
        }).await
    }

    pub async fn initialize(
        self,
        root: &Path,
        name: String,
        email: String,
        temporary_password: String,
    ) -> Result<()> {
        let password = Zeroizing::new(temporary_password);
        let name = name_valid(name, 200)?;
        let email = email_valid(email)?;
        password_valid(&password)?;
        let hash = hash_password(password).await?;
        let store = self.open(root, true)?;
        store
            .call(move |db| {
                let tx = db
                    .conn
                    .transaction_with_behavior(TransactionBehavior::Immediate)?;
                let count: i64 = tx.query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))?;
                if count == 0 {
                    tx.execute_batch("CREATE TABLE personal_runtime_identity(singleton INTEGER PRIMARY KEY CHECK(singleton=1), volume TEXT NOT NULL, generation INTEGER NOT NULL CHECK(generation>0)) STRICT;")?;
                    tx.execute("INSERT INTO personal_runtime_identity VALUES(1,?1,?2)",params![self.volume.to_string(), self.generation as i64])?;
                    insert_account_with_identity(
                        &tx,
                        (self.account, self.tenant),
                        name,
                        email,
                        hash,
                        Role::Member,
                        true,
                    )?;
                } else {
                    self.check_identity(&tx, Some(&email))?;
                    // A previously delivered credential cannot be replaced by replay.
                    return Err(StoreError::AlreadyInitialized);
                }
                tx.commit()?;
                Ok(())
            })
            .await
    }

    pub(crate) fn check_identity(self, conn: &Connection, email: Option<&str>) -> Result<()> {
        let matches: bool = conn.query_row(
            "SELECT (SELECT count(*) FROM accounts)=1 AND (SELECT count(*) FROM tenants)=1 AND EXISTS(
             SELECT 1 FROM accounts JOIN tenants ON tenants.id=accounts.tenant_id
             WHERE EXISTS(SELECT 1 FROM personal_runtime_identity WHERE singleton=1 AND volume=?4 AND generation=?5) AND accounts.id=?1 AND tenants.id=?2 AND accounts.role='member' AND (?3 IS NULL OR accounts.email=?3))",
             params![self.account.to_string(),self.tenant.to_string(),email,self.volume.to_string(),self.generation as i64],|r|r.get(0))?;
        if matches {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        }
    }

    pub async fn open_verified(self, root: &Path) -> Result<Store> {
        let store = self.open(root, false)?;
        store
            .call(move |db| self.check_identity(&db.conn, None))
            .await?;
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    async fn session_survives_restart_only_on_its_owner_volume() {
        let temp = tempfile::tempdir().unwrap();
        let base = temp.path().canonicalize().unwrap();
        let mut users = Vec::new();
        for label in ["a", "b"] {
            let root = base.join(label);
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let owner = VolumeOwner {
                account: Uuid::new_v4(),
                tenant: Uuid::new_v4(),
                volume: Uuid::new_v4(),
                generation: 1,
            };
            let email = format!("{label}@example.test");
            owner
                .initialize(
                    &root,
                    label.into(),
                    email.clone(),
                    "temporary-example".into(),
                )
                .await
                .unwrap();
            assert!(matches!(
                owner
                    .initialize(
                        &root,
                        label.into(),
                        email.clone(),
                        "replacement-example".into()
                    )
                    .await,
                Err(StoreError::AlreadyInitialized)
            ));
            let store = owner.open(&root, false).unwrap();
            let first = store
                .login(email, "temporary-example".into())
                .await
                .unwrap();
            assert!(first.user.password_change_required());
            let changed = store
                .change_password(&first.user, "changed-example".into())
                .await
                .unwrap();
            users.push((root, owner, changed.token));
        }
        let (aroot, a, token) = &users[0];
        let (broot, b, _) = &users[1];
        assert!(matches!(a.open(broot, false), Err(StoreError::Conflict)));
        let stale = VolumeOwner {
            generation: 2,
            ..*a
        };
        assert!(matches!(stale.open(aroot, true), Err(StoreError::Conflict)));
        // A mislabeled recovered DB must fail before Store startup changes even
        // one byte or lets a restored session through.
        let actual = aroot.join(a.directory().unwrap());
        let mislabeled = aroot.join(stale.directory().unwrap());
        fs::rename(&actual, &mislabeled).unwrap();
        let before = fs::read(mislabeled.join("assistant.sqlite")).unwrap();
        assert!(stale.open(aroot, false).is_err());
        assert!(Store::open(&mislabeled).is_err());
        assert_eq!(
            before,
            fs::read(mislabeled.join("assistant.sqlite")).unwrap()
        );
        fs::rename(&mislabeled, &actual).unwrap();
        let reopened = a.open(aroot, false).unwrap();
        let session = reopened.authenticate(token.clone()).await.unwrap();
        assert_eq!(session.account().id, a.account.to_string());
        assert!(!session.password_change_required());
        assert!(matches!(
            b.open(broot, false)
                .unwrap()
                .authenticate(token.clone())
                .await,
            Err(StoreError::Unauthorized)
        ));
    }
}

#[cfg(test)]
mod invite_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    async fn installer_status_recovers_initialization_without_replacing_a_password() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = VolumeOwner {
            account: Uuid::new_v4(),
            tenant: Uuid::new_v4(),
            volume: Uuid::new_v4(),
            generation: 1,
        };
        assert!(
            !owner
                .initialization_status(&root, "new@example.invalid".into())
                .await
                .unwrap()
        );
        owner
            .initialize(
                &root,
                "Synthetic member".into(),
                "new@example.invalid".into(),
                "Original-synthetic-password-191!".into(),
            )
            .await
            .unwrap();
        assert!(
            owner
                .initialization_status(&root, "new@example.invalid".into())
                .await
                .unwrap()
        );
        assert!(
            owner
                .initialization_status(&root, "other@example.invalid".into())
                .await
                .is_err()
        );
        assert!(
            owner
                .initialize(
                    &root,
                    "Synthetic member".into(),
                    "new@example.invalid".into(),
                    "Replacement-synthetic-password-191!".into()
                )
                .await
                .is_err()
        );
        let store = owner.open_verified(&root).await.unwrap();
        assert!(
            store
                .login(
                    "new@example.invalid".into(),
                    "Original-synthetic-password-191!".into()
                )
                .await
                .is_ok()
        );
        assert!(
            store
                .login(
                    "new@example.invalid".into(),
                    "Replacement-synthetic-password-191!".into()
                )
                .await
                .is_err()
        );
    }
}
