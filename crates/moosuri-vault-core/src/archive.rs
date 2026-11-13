//! Independent ciphertext-only archive. Source root and fields never leave core.
use crate::{
    ActiveVault, Cancellation, EncryptedItem, Error, Header, ImportStatus, ItemFields, Result,
    SecretText, TrustedEntropy,
    apple::{account_fingerprint, fingerprint},
    crypto::random_id,
    inventory_digest,
    wire::{b64, json, unb64},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use uuid::Uuid;

pub const ARCHIVE_LIMIT: usize = 26 * 1024 * 1024;
pub const ARCHIVE_ITEMS: usize = 256;
const FORMAT: &str = "moosuri-encrypted-vault-archive";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    format: String,
    version: u8,
    count: u32,
    header: Header,
    items: Vec<EncryptedItem>,
    authentication: String,
}
impl Archive {
    /// Structure, exact policies and all item sizes are checked before any KDF.
    fn transcript(&self, cancel: &impl Cancellation) -> Result<[u8; 32]> {
        if self.format != FORMAT || self.version != 1 {
            return Err(Error::Unsupported);
        }
        if self.items.len() > ARCHIVE_ITEMS || self.count as usize != self.items.len() {
            return Err(Error::Limit);
        }
        let header = self.header.to_json()?;
        let mut hash = Sha256::new();
        hash.update(b"moosuri-vault/archive/v1/manifest\0");
        hash.update([self.version]);
        hash.update((header.len() as u64).to_be_bytes());
        hash.update(header);
        hash.update((self.items.len() as u64).to_be_bytes());
        let mut ids = HashSet::new();
        for item in &self.items {
            if cancel.cancelled() {
                return Err(Error::Cancelled);
            }
            if item.generation != self.header.generation || !ids.insert(item.id) {
                return Err(Error::Conflict);
            }
            let bytes = item.to_json()?;
            hash.update((bytes.len() as u64).to_be_bytes());
            hash.update(bytes);
        }
        Ok(hash.finalize().into())
    }
}
impl ActiveVault {
    /// Export the client's complete pinned inventory, never plaintext fields.
    /// Caller may write these ciphertext bytes to a local user-selected file.
    pub fn export_archive(
        &mut self,
        items: &[EncryptedItem],
        expected_inventory: [u8; 32],
        cancel: &impl Cancellation,
    ) -> Result<Vec<u8>> {
        if cancel.cancelled() {
            return Err(Error::Cancelled);
        }
        if items.len() > ARCHIVE_ITEMS {
            return Err(Error::Limit);
        }
        if inventory_digest(items)? != expected_inventory {
            return Err(Error::Conflict);
        }
        // Authenticate every payload even when an item was never displayed.
        for item in items {
            if cancel.cancelled() {
                return Err(Error::Cancelled);
            }
            self.open_item(item)?;
        }
        let mut archive = Archive {
            format: FORMAT.into(),
            version: 1,
            count: items.len() as u32,
            header: self.header().clone(),
            items: items.to_vec(),
            authentication: String::new(),
        };
        archive.authentication = b64(&self.archive_auth(&archive.transcript(cancel)?)?);
        json(&archive, ARCHIVE_LIMIT)
    }
}
pub struct ArchiveRow {
    pub row: u32,
    pub status: ImportStatus,
}
pub struct ArchivePreview {
    pub rows: Vec<ArchiveRow>,
    pub eligible: u32,
    pub duplicates: u32,
}
struct Row {
    number: u32,
    fields: ItemFields,
}
pub struct ArchiveImport {
    header: [u8; 32],
    preview: ArchivePreview,
    rows: Vec<Row>,
    forbidden_ids: HashSet<Uuid>,
    existing_count: usize,
    batch: Option<(Vec<u32>, Vec<EncryptedItem>)>,
}
impl ArchiveImport {
    /// Entire archive is authenticated before any source plaintext is staged or
    /// preview metadata returned. No original server, owner session or receipt.
    pub fn preview(
        bytes: &[u8],
        secret: &SecretText,
        recovery: bool,
        current: &mut ActiveVault,
        existing: &[EncryptedItem],
        cancel: &impl Cancellation,
    ) -> Result<Self> {
        if cancel.cancelled() {
            return Err(Error::Cancelled);
        }
        if bytes.len() > ARCHIVE_LIMIT || existing.len() > ARCHIVE_ITEMS {
            return Err(Error::Limit);
        }
        let archive: Archive = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        let transcript = archive.transcript(cancel)?;
        let authentication = unb64(&archive.authentication, 32, 32)?;
        if cancel.cancelled() {
            return Err(Error::Cancelled);
        }
        let mut source = ActiveVault::archive_open(
            archive.header,
            secret,
            recovery,
            &transcript,
            &authentication,
        )?;
        // Only after manifest verification, authenticate and decode ALL items.
        // An error on the last item drops all prior secret-owning temporary rows.
        let mut authenticated = Vec::with_capacity(archive.items.len());
        for item in &archive.items {
            if cancel.cancelled() {
                return Err(Error::Cancelled);
            }
            authenticated.push(source.open_item(item)?);
        }
        drop(source);
        inventory_digest(existing)?; // Reject duplicate current IDs as well.
        let mut contents = HashSet::new();
        let mut accounts = HashSet::new();
        let mut forbidden_ids: HashSet<_> = archive.items.iter().map(|i| i.id).collect();
        for item in existing {
            if cancel.cancelled() {
                return Err(Error::Cancelled);
            }
            let fields = current.open_item(item)?;
            contents.insert(fingerprint(&fields)?);
            accounts.insert(account_fingerprint(&fields)?);
            forbidden_ids.insert(item.id);
        }
        let mut preview = ArchivePreview {
            rows: vec![],
            eligible: 0,
            duplicates: 0,
        };
        let mut rows = vec![];
        for (index, fields) in authenticated.into_iter().enumerate() {
            if cancel.cancelled() {
                return Err(Error::Cancelled);
            }
            let number = index as u32 + 1;
            let content = fingerprint(&fields)?;
            let account = account_fingerprint(&fields)?;
            let status = if !contents.insert(content) {
                preview.duplicates += 1;
                ImportStatus::Duplicate
            } else {
                preview.eligible += 1;
                if accounts.contains(&account) {
                    ImportStatus::ChangedAccount
                } else {
                    ImportStatus::New
                }
            };
            accounts.insert(account);
            preview.rows.push(ArchiveRow {
                row: number,
                status,
            });
            if status != ImportStatus::Duplicate {
                rows.push(Row { number, fields });
            }
        }
        Ok(Self {
            header: current.header_digest()?,
            preview,
            rows,
            forbidden_ids,
            existing_count: existing.len(),
            batch: None,
        })
    }
    pub fn counts(&self) -> &ArchivePreview {
        &self.preview
    }
    /// Immutable reencrypted batch; only existing exact-operation machinery may
    /// publish/acknowledge it. Repeated selection returns identical ciphertexts.
    pub fn seal_selected<'a>(
        &'a mut self,
        current: &mut ActiveVault,
        selected: &[u32],
        entropy: &mut impl TrustedEntropy,
        cancel: &impl Cancellation,
    ) -> Result<&'a [EncryptedItem]> {
        if current.header_digest()? != self.header {
            return Err(Error::Conflict);
        }
        if selected.is_empty() || selected.len() > ARCHIVE_ITEMS.saturating_sub(self.existing_count)
        {
            return Err(Error::Limit);
        }
        if cancel.cancelled() {
            return Err(Error::Cancelled);
        }
        if self
            .batch
            .as_ref()
            .is_some_and(|(prior, _)| prior != selected)
        {
            return Err(Error::Conflict);
        }
        if self.batch.is_none() {
            let mut unique = HashSet::new();
            for n in selected {
                if !unique.insert(*n) || !self.rows.iter().any(|r| r.number == *n) {
                    return Err(Error::Invalid);
                }
            }
            let mut batch = Vec::with_capacity(selected.len());
            let mut ids = self.forbidden_ids.clone();
            for number in selected {
                if cancel.cancelled() {
                    return Err(Error::Cancelled);
                }
                let row = self
                    .rows
                    .iter()
                    .find(|r| r.number == *number)
                    .ok_or(Error::Invalid)?;
                let mut chosen = None;
                for _ in 0..16 {
                    let id = random_id(entropy)?;
                    if ids.insert(id) {
                        chosen = Some(id);
                        break;
                    }
                }
                batch.push(current.seal_item(
                    chosen.ok_or(Error::Entropy)?,
                    1,
                    &row.fields,
                    entropy,
                )?);
            }
            self.batch = Some((selected.to_vec(), batch));
            self.rows.clear();
        }
        Ok(&self.batch.as_ref().ok_or(Error::State)?.1)
    }
}

#[cfg(all(test, feature = "native-entropy"))]
mod tests {
    use super::*;
    use crate::{
        Binding, Continue, ItemKind, OsEntropy, PendingVault, RecoveryKey, StorageReceipt,
    };
    fn text(v: &str) -> SecretText {
        SecretText::new(v.into()).unwrap()
    }
    fn vault(n: u128) -> (ActiveVault, SecretText) {
        let b = Binding {
            owner_id: Uuid::from_u128(n),
            tenant_id: Uuid::from_u128(n + 1),
            vault_id: Uuid::from_u128(n + 2),
        };
        let mut p = PendingVault::prepare(
            b,
            &text("Synthetic archive fixture password"),
            &mut OsEntropy,
        )
        .unwrap();
        let recovery = p.recovery().expose_encoded();
        p.verify_recovery(&RecoveryKey::parse(&recovery).unwrap())
            .unwrap();
        let (id, d) = p.candidate().unwrap();
        p.confirm_commit(StorageReceipt::from_verified_storage(id, d))
            .unwrap();
        (p.activate().unwrap(), recovery)
    }
    fn fields(n: &str) -> ItemFields {
        ItemFields {
            kind: ItemKind::Login,
            title: text(n),
            username: text("synthetic-user"),
            url: text("https://example.test"),
            notes: text("synthetic notes"),
            favorite: true,
            expires_at: text("2030-01-01"),
            secret: text(n),
            totp: text("JBSWY3DPEHPK3PXP"),
        }
    }
    #[test]
    fn independent_roundtrip_new_owner_ids_duplicates_selection_and_totp() {
        let (mut source, recovery) = vault(10);
        let first = source
            .seal_item(
                Uuid::from_u128(90),
                1,
                &fields("Synthetic one"),
                &mut OsEntropy,
            )
            .unwrap();
        let second = source
            .seal_item(
                Uuid::from_u128(91),
                1,
                &fields("Synthetic two"),
                &mut OsEntropy,
            )
            .unwrap();
        let items = vec![first, second];
        let archive = source
            .export_archive(&items, inventory_digest(&items).unwrap(), &Continue)
            .unwrap();
        assert!(!String::from_utf8_lossy(&archive).contains("Synthetic one"));
        let password = text("Synthetic archive fixture password");
        let (mut target, _) = vault(20);
        let duplicate = target
            .seal_item(
                Uuid::from_u128(92),
                1,
                &fields("Synthetic one"),
                &mut OsEntropy,
            )
            .unwrap();
        let mut import = ArchiveImport::preview(
            &archive,
            &password,
            false,
            &mut target,
            std::slice::from_ref(&duplicate),
            &Continue,
        )
        .unwrap();
        assert_eq!(import.counts().duplicates, 1);
        assert_eq!(import.counts().eligible, 1);
        assert!(
            import
                .seal_selected(&mut target, &[1], &mut OsEntropy, &Continue)
                .is_err()
        );
        let sealed = import
            .seal_selected(&mut target, &[2], &mut OsEntropy, &Continue)
            .unwrap()
            .to_vec();
        assert!(!items.iter().any(|i| i.id == sealed[0].id));
        let original = sealed[0].to_json().unwrap();
        assert_eq!(
            import
                .seal_selected(&mut target, &[2], &mut OsEntropy, &Continue)
                .unwrap()[0]
                .to_json()
                .unwrap(),
            original
        );
        assert!(
            import
                .seal_selected(&mut target, &[1, 2], &mut OsEntropy, &Continue)
                .is_err()
        );
        let opened = target.open_item(&sealed[0]).unwrap();
        assert_eq!(opened.secret.expose(), "Synthetic two");
        assert_eq!(opened.totp.expose(), "JBSWY3DPEHPK3PXP");
        assert!(opened.favorite);
        assert_eq!(opened.expires_at.expose(), "2030-01-01");
        assert!(source.open_item(&sealed[0]).is_err());
        drop(source); // Original vault/server is no longer available.
        assert!(
            ArchiveImport::preview(&archive, &recovery, true, &mut target, &[], &Continue).is_ok()
        );
        assert!(
            ArchiveImport::preview(
                &archive,
                &text("wrong synthetic password"),
                false,
                &mut target,
                &[],
                &Continue
            )
            .is_err()
        );
        let (_, wrong_recovery) = vault(30);
        assert!(
            ArchiveImport::preview(&archive, &wrong_recovery, true, &mut target, &[], &Continue)
                .is_err()
        );
        let mut stale =
            ArchiveImport::preview(&archive, &recovery, true, &mut target, &[], &Continue).unwrap();
        let mut change = target.prepare_recovery_change(&mut OsEntropy).unwrap();
        let next = RecoveryKey::parse(&change.recovery().unwrap().expose_encoded()).unwrap();
        change.verify_recovery(&next).unwrap();
        let (id, digest) = change.candidate().unwrap();
        change
            .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
            .unwrap();
        change.activate().unwrap();
        assert!(matches!(
            stale.seal_selected(&mut target, &[1], &mut OsEntropy, &Continue),
            Err(Error::Conflict)
        ));
    }
    #[test]
    fn tamper_order_truncation_duplicates_missing_payload_and_pre_kdf_bounds_fail_closed() {
        let (mut source, recovery) = vault(100);
        let items = vec![
            source
                .seal_item(
                    Uuid::from_u128(190),
                    1,
                    &fields("Synthetic first"),
                    &mut OsEntropy,
                )
                .unwrap(),
            source
                .seal_item(
                    Uuid::from_u128(191),
                    1,
                    &fields("Synthetic last"),
                    &mut OsEntropy,
                )
                .unwrap(),
        ];
        let archive = source
            .export_archive(&items, inventory_digest(&items).unwrap(), &Continue)
            .unwrap();
        let (mut target, _) = vault(200);
        let value: serde_json::Value = serde_json::from_slice(&archive).unwrap();
        for mode in 0..8 {
            let mut v = value.clone();
            match mode {
                0 => v["items"].as_array_mut().unwrap().swap(0, 1),
                1 => {
                    v["items"].as_array_mut().unwrap().pop();
                }
                2 => v["items"][1] = v["items"][0].clone(),
                3 => v["items"][1]["ciphertext"] = v["items"][0]["ciphertext"].clone(),
                4 => v["header"]["binding"]["owner_id"] = serde_json::json!(Uuid::from_u128(999)),
                5 => v["count"] = serde_json::json!(1),
                6 => {
                    v["header"]["envelopes"][0]["kdf"]["params"]["memory_kib"] =
                        serde_json::json!(u32::MAX)
                }
                _ => v["unknown"] = serde_json::json!(true),
            }
            assert!(
                ArchiveImport::preview(
                    &serde_json::to_vec(&v).unwrap(),
                    &recovery,
                    true,
                    &mut target,
                    &[],
                    &Continue
                )
                .is_err(),
                "mode {mode}"
            );
        }
        let mut trailing = archive.clone();
        trailing.extend_from_slice(b" null");
        assert!(
            ArchiveImport::preview(&trailing, &recovery, true, &mut target, &[], &Continue)
                .is_err()
        );
        assert!(
            ArchiveImport::preview(
                &archive[..archive.len() - 1],
                &recovery,
                true,
                &mut target,
                &[],
                &Continue
            )
            .is_err()
        );
        assert!(
            ArchiveImport::preview(
                &vec![b' '; ARCHIVE_LIMIT + 1],
                &recovery,
                true,
                &mut target,
                &[],
                &Continue
            )
            .is_err()
        );
        // A valid root-authenticated manifest still cannot admit a bad final payload.
        let mut a: Archive = serde_json::from_slice(&archive).unwrap();
        a.items[1].ciphertext = a.items[0].ciphertext.clone();
        a.authentication = b64(&source
            .archive_auth(&a.transcript(&Continue).unwrap())
            .unwrap());
        assert!(
            ArchiveImport::preview(
                &serde_json::to_vec(&a).unwrap(),
                &recovery,
                true,
                &mut target,
                &[],
                &Continue
            )
            .is_err()
        );
        // An excessive item count and disallowed KDF fail before secret parsing/KDF.
        let mut a: Archive = serde_json::from_slice(&archive).unwrap();
        a.items = vec![items[0].clone(); 257];
        a.count = 257;
        assert!(matches!(
            ArchiveImport::preview(
                &serde_json::to_vec(&a).unwrap(),
                &text("not a recovery key"),
                true,
                &mut target,
                &[],
                &Continue
            ),
            Err(Error::Limit)
        ));
        let mut v = value;
        v["header"]["envelopes"][0]["kdf"]["params"]["memory_kib"] = serde_json::json!(u32::MAX);
        assert!(matches!(
            ArchiveImport::preview(
                &serde_json::to_vec(&v).unwrap(),
                &text("not a recovery key"),
                true,
                &mut target,
                &[],
                &Continue
            ),
            Err(Error::KdfPolicy)
        ));
        struct Stop;
        impl Cancellation for Stop {
            fn cancelled(&self) -> bool {
                true
            }
        }
        assert!(matches!(
            ArchiveImport::preview(&archive, &recovery, true, &mut target, &[], &Stop),
            Err(Error::Cancelled)
        ));
        assert!(source.export_archive(&items, [0; 32], &Continue).is_err());
    }
}
