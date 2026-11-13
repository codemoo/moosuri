//! Immutable ciphertext history, exact operation receipts, durable nonce ledger.
use crate::*;
use fs2::FileExt;
use moosuri_vault_core::{inventory_digest, storage_commitment};
use protocol::{Binding, EncryptedItem, Header, Operation, Owner, Receipt, VaultState};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

const JOURNAL_LIMIT: u64 = 1024 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"MVSJ0001";
pub struct Database {
    conn: Connection,
    journal: File,
    head: [u8; 32],
    sequence: i64,
    _lock: File,
    poisoned: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Intent {
    Prepare {
        owner: Owner,
        operation: Operation,
    },
    Adopt {
        owner: Owner,
        operation: uuid::Uuid,
        digest: String,
    },
    Cancel {
        owner: Owner,
        operation: Operation,
        digest: String,
    },
    Close {
        session: String,
    },
}
fn private_file(path: &Path, create: bool) -> Result<File> {
    let mut opts = OpenOptions::new();
    opts.read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    if create {
        opts.create_new(true);
    }
    let f = opts.open(path)?;
    if !f.metadata()?.is_file() || f.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(Error::Unavailable);
    }
    Ok(f)
}
fn json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let b = serde_json::to_vec(value).map_err(|_| Error::Invalid)?;
    if b.len() > MAX_BODY {
        return Err(Error::Limit);
    }
    Ok(b)
}
fn owner_key(owner: &Owner) -> (String, String) {
    (owner.tenant.to_string(), owner.account.to_string())
}
fn inventory(items: &[EncryptedItem]) -> Result<String> {
    Ok(inventory_digest(items)
        .map_err(|_| Error::Invalid)?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}
impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let fresh = !path.exists();
        if fresh {
            fs::DirBuilder::new().mode(0o700).create(path)?
        }
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_dir() || meta.file_type().is_symlink() || meta.permissions().mode() & 0o077 != 0
        {
            return Err(Error::Unavailable);
        }
        let lock = private_file(&path.join("vault.lock"), !path.join("vault.lock").exists())?;
        lock.try_lock_exclusive().map_err(|_| Error::Unavailable)?;
        let db_exists = path.join("vault.sqlite").exists();
        let journal_exists = path.join("vault.journal").exists();
        if db_exists != journal_exists {
            return Err(Error::Unavailable);
        }
        if !db_exists
            && fs::read_dir(path)?
                .filter_map(std::result::Result::ok)
                .any(|e| e.file_name() != "vault.lock")
        {
            return Err(Error::Unavailable);
        }
        if !db_exists {
            private_file(&path.join("vault.sqlite"), true)?;
        }
        for name in ["vault.sqlite", "vault.sqlite-wal", "vault.sqlite-shm"] {
            if path.join(name).exists() {
                private_file(&path.join(name), false)?;
            }
        }
        let mut journal = private_file(&path.join("vault.journal"), !journal_exists)?;
        let conn = Connection::open(path.join("vault.sqlite"))?;
        conn.execute_batch("PRAGMA journal_mode=WAL;PRAGMA synchronous=FULL;PRAGMA foreign_keys=ON;PRAGMA trusted_schema=OFF;")?;
        let mut head = [0; 32];
        let mut sequence = 0i64;
        if !db_exists {
            let id = uuid::Uuid::new_v4();
            journal.write_all(MAGIC)?;
            journal.write_all(id.as_bytes())?;
            journal.sync_all()?;
            File::open(path)?.sync_all()?;
            conn.execute_batch(include_str!("schema.sql"))?;
            conn.execute(
                "INSERT INTO meta VALUES(1,?1,0,?2)",
                params![id.to_string(), head.as_slice()],
            )?;
        } else {
            if journal.metadata()?.len() > JOURNAL_LIMIT {
                return Err(Error::Limit);
            }
            let mut prefix = [0; 24];
            journal.read_exact(&mut prefix)?;
            if &prefix[..8] != MAGIC {
                return Err(Error::Unavailable);
            }
            let id = uuid::Uuid::from_slice(&prefix[8..]).map_err(|_| Error::Unavailable)?;
            let (db_id, db_seq, db_head): (String, i64, Vec<u8>) = conn.query_row(
                "SELECT installation,sequence,head FROM meta WHERE singleton=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            if db_id != id.to_string() {
                return Err(Error::Unavailable);
            }
            loop {
                let mut len = [0; 4];
                let n = journal.read(&mut len)?;
                if n == 0 {
                    break;
                }
                if n != 4 {
                    return Err(Error::Unavailable);
                }
                let len = u32::from_be_bytes(len) as usize;
                if len > MAX_BODY + 4096 {
                    return Err(Error::Limit);
                }
                let mut bytes = vec![0; len];
                journal.read_exact(&mut bytes)?;
                let mut want = [0; 32];
                journal.read_exact(&mut want)?;
                let mut h = Sha256::new();
                h.update(head);
                h.update(&bytes);
                let got: [u8; 32] = h.finalize().into();
                if got != want {
                    return Err(Error::Unavailable);
                }
                let _: Intent = serde_json::from_slice(&bytes).map_err(|_| Error::Unavailable)?;
                head = got;
                sequence = sequence.checked_add(1).ok_or(Error::Limit)?;
            }
            if db_seq != sequence || db_head != head {
                return Err(Error::Unavailable);
            }
            let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
            if version != 1 {
                return Err(Error::Unavailable);
            }
        }
        Ok(Self {
            conn,
            journal,
            head,
            sequence,
            _lock: lock,
            poisoned: false,
        })
    }
    fn ready(&self) -> Result<()> {
        if self.poisoned {
            Err(Error::Unavailable)
        } else {
            Ok(())
        }
    }
    fn append(&mut self, intent: &Intent) -> Result<(i64, [u8; 32])> {
        self.ready()?;
        let bytes = json(intent)?;
        let needed = bytes.len() as u64 + 36;
        if self
            .journal
            .metadata()?
            .len()
            .checked_add(needed)
            .is_none_or(|v| v > JOURNAL_LIMIT)
        {
            return Err(Error::Limit);
        }
        self.poisoned = true;
        let mut hash = Sha256::new();
        hash.update(self.head);
        hash.update(&bytes);
        let head: [u8; 32] = hash.finalize().into();
        self.journal
            .write_all(&(bytes.len() as u32).to_be_bytes())?;
        self.journal.write_all(&bytes)?;
        self.journal.write_all(&head)?;
        self.journal.sync_all()?;
        Ok((self.sequence.checked_add(1).ok_or(Error::Limit)?, head))
    }
    fn adopt_head(&mut self, next: (i64, [u8; 32])) {
        self.sequence = next.0;
        self.head = next.1;
        self.poisoned = false;
    }
    pub fn create_session(&mut self, cookie: &str, owner: &Owner) -> Result<()> {
        self.ready()?;
        self.conn
            .execute("DELETE FROM sessions WHERE expires_at<?1", [now()?])?;
        self.conn.execute(
            "INSERT OR IGNORE INTO owners VALUES(?1,?2)",
            params![owner.tenant.to_string(), owner.account.to_string()],
        )?;
        let count: i64 = self.conn.query_row(
            "SELECT count(*) FROM sessions WHERE owner=?1",
            [owner.account.to_string()],
            |r| r.get(0),
        )?;
        if count >= 64 {
            return Err(Error::Limit);
        }
        self.conn.execute(
            "INSERT INTO sessions VALUES(?1,?2,?3,?4,?5,?6,0)",
            params![
                hash(cookie.as_bytes()),
                owner.account.to_string(),
                owner.tenant.to_string(),
                owner.installation.to_string(),
                owner.epoch,
                now()?.checked_add(86400).ok_or(Error::Unavailable)?
            ],
        )?;
        Ok(())
    }
    pub fn session(&self, cookie: &str) -> Result<Owner> {
        self.ready()?;
        self.conn.query_row("SELECT owner,tenant,installation,epoch FROM sessions WHERE digest=?1 AND closed=0 AND expires_at>?2",params![hash(cookie.as_bytes()),now()?],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?))).optional()?.ok_or(Error::Unauthorized).and_then(|(a,t,i,e)|Ok(Owner{account:a.parse().map_err(|_|Error::Unavailable)?,tenant:t.parse().map_err(|_|Error::Unavailable)?,installation:i.parse().map_err(|_|Error::Unavailable)?,epoch:e}))
    }
    pub fn close_session(&mut self, cookie: &str) -> Result<()> {
        self.ready()?;
        let id = hash(cookie.as_bytes());
        let next = self.append(&Intent::Close {
            session: id.clone(),
        })?;
        let tx = self.conn.transaction()?;
        tx.execute("UPDATE sessions SET closed=1 WHERE digest=?1", [id])?;
        tx.execute(
            "UPDATE meta SET sequence=?1,head=?2",
            params![next.0, next.1.as_slice()],
        )?;
        tx.commit()?;
        self.adopt_head(next);
        Ok(())
    }
    /// Read the admitted key lifetime without loading the encrypted inventory.
    pub fn head(&self, owner: &Owner) -> Result<protocol::VaultHead> {
        self.ready()?;
        let (tenant, account) = owner_key(owner);
        let row: Option<(String, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT vault,header FROM vaults WHERE tenant=?1 AND owner=?2",
                params![tenant, account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match row {
            None => Ok(protocol::VaultHead {
                header_sha256: String::new(),
                revision: 0,
                envelope_epoch: 0,
                generation: 0,
            }),
            Some((vault, bytes)) => {
                let binding = Binding {
                    owner_id: owner.account,
                    tenant_id: owner.tenant,
                    vault_id: vault.parse().map_err(|_| Error::Unavailable)?,
                };
                let header = Header::from_json(&bytes, binding).map_err(|_| Error::Unavailable)?;
                Ok(protocol::VaultHead {
                    header_sha256: hash(&bytes),
                    revision: header.revision,
                    envelope_epoch: header.envelope_epoch,
                    generation: header.generation,
                })
            }
        }
    }
    pub fn state(&self, owner: &Owner) -> Result<VaultState> {
        self.ready()?;
        let (tenant, account) = owner_key(owner);
        let row: Option<(String, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT vault,header FROM vaults WHERE tenant=?1 AND owner=?2",
                params![tenant, account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let mut items = vec![];
        let (binding, header, digest) = if let Some((vault, bytes)) = row {
            let binding = Binding {
                owner_id: owner.account,
                tenant_id: owner.tenant,
                vault_id: vault.parse().map_err(|_| Error::Unavailable)?,
            };
            let header = Header::from_json(&bytes, binding).map_err(|_| Error::Unavailable)?;
            let mut stmt=self.conn.prepare("SELECT ciphertext FROM items WHERE tenant=?1 AND owner=?2 AND deleted=0 ORDER BY item")?;
            for row in stmt.query_map(params![tenant, account], |r| r.get::<_, Vec<u8>>(0))? {
                items.push(EncryptedItem::from_json(&row?).map_err(|_| Error::Unavailable)?)
            }
            (Some(binding), Some(header), hash(&bytes))
        } else {
            (None, None, String::new())
        };
        let mut stmt=self.conn.prepare("SELECT receipt FROM operations WHERE tenant=?1 AND owner=?2 AND status='pending' ORDER BY id LIMIT 33")?;
        let pending = stmt
            .query_map(params![tenant, account], |r| r.get::<_, Vec<u8>>(0))?
            .map(|r| serde_json::from_slice(&r?).map_err(|_| Error::Unavailable))
            .collect::<Result<Vec<Receipt>>>()?;
        if pending.len() > 32 {
            return Err(Error::Unavailable);
        }
        let inventory_sha256 = inventory(&items)?;
        Ok(VaultState {
            owner: owner.clone(),
            binding,
            header,
            header_sha256: digest,
            items,
            inventory_sha256,
            pending,
        })
    }
    pub fn operation(&self, owner: &Owner, id: uuid::Uuid) -> Result<(Operation, Receipt)> {
        self.ready()?;
        let (t, a) = owner_key(owner);
        let (body, receipt): (Vec<u8>, Vec<u8>) = self
            .conn
            .query_row(
                "SELECT body,receipt FROM operations WHERE tenant=?1 AND owner=?2 AND id=?3",
                params![t, a, id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(Error::NotFound)?;
        Ok((
            serde_json::from_slice(&body).map_err(|_| Error::Unavailable)?,
            serde_json::from_slice(&receipt).map_err(|_| Error::Unavailable)?,
        ))
    }
    pub fn prepare(&mut self, owner: &Owner, operation: Operation) -> Result<Receipt> {
        self.ready()?;
        let body = json(&operation)?;
        let digest = hash(&body);
        let id = operation.id();
        if id.is_nil() {
            return Err(Error::Invalid);
        }
        match self.operation(owner, id) {
            Ok((_, r)) => {
                if r.payload_sha256 != digest {
                    return Err(Error::Conflict);
                }
                return Ok(r);
            }
            Err(Error::NotFound) => {}
            Err(e) => return Err(e),
        }
        let current = self.state(owner)?;
        validate_operation(owner, &current, &operation)?;
        let (t, a) = owner_key(owner);
        let(count,used):(i64,i64)=self.conn.query_row("SELECT count(*),coalesce(sum(length(body)),0) FROM operations WHERE tenant=?1 AND owner=?2",params![t,a],|r|Ok((r.get(0)?,r.get(1)?)))?;
        if count >= 100000
            || used
                .checked_add(body.len() as i64)
                .is_none_or(|n| n > MAX_RETAINED as i64)
        {
            return Err(Error::Limit);
        }
        if current.pending.len() >= 32 {
            return Err(Error::Limit);
        }
        // Validate nonce reservations in the DB transaction before adopting intent.
        let next_seq = self.sequence.checked_add(1).ok_or(Error::Limit)?;
        {
            let tx = self.conn.unchecked_transaction()?;
            reserve(&tx, owner, &operation)?;
            tx.rollback()?;
        }
        let next = self.append(&Intent::Prepare {
            owner: owner.clone(),
            operation: operation.clone(),
        })?;
        assert_eq!(next.0, next_seq);
        let tx = self.conn.transaction()?;
        reserve(&tx, owner, &operation)?;
        let receipt = Receipt {
            operation: id,
            payload_sha256: digest,
            state: "pending".into(),
            header_sha256: current.header_sha256,
            inventory_sha256: current.inventory_sha256,
            commitment: match &operation {
                Operation::Key { commitment, .. } => Some(commitment.clone()),
                _ => None,
            },
        };
        tx.execute(
            "INSERT INTO operations VALUES(?1,?2,?3,?4,?5,?6,'pending')",
            params![t, a, id.to_string(), owner.epoch, body, json(&receipt)?],
        )?;
        tx.execute(
            "UPDATE meta SET sequence=?1,head=?2",
            params![next.0, next.1.as_slice()],
        )?;
        tx.commit()?;
        self.adopt_head(next);
        Ok(receipt)
    }
    /// Call only after exact online assistant commitment, matching owner/epoch.
    pub fn commit(&mut self, owner: &Owner, id: uuid::Uuid, digest: &str) -> Result<Receipt> {
        self.ready()?;
        let (op, mut receipt) = self.operation(owner, id)?;
        if receipt.payload_sha256 != digest {
            return Err(Error::Conflict);
        }
        if receipt.state != "pending" {
            return Ok(receipt);
        }
        let (t, a) = owner_key(owner);
        let epoch: i64 = self.conn.query_row(
            "SELECT epoch FROM operations WHERE tenant=?1 AND owner=?2 AND id=?3",
            params![t, a, id.to_string()],
            |r| r.get(0),
        )?;
        if epoch != owner.epoch {
            return Err(Error::Unauthorized);
        }
        let current = self.state(owner)?;
        if validate_operation(owner, &current, &op).is_err() {
            receipt.state = "conflict".into();
        }
        let next = self.append(&Intent::Adopt {
            owner: owner.clone(),
            operation: id,
            digest: digest.into(),
        })?;
        let tx = self.conn.transaction()?;
        if receipt.state == "pending" {
            match &op {
                Operation::Key { header, items, .. } => {
                    let bytes = header.to_json().map_err(|_| Error::Invalid)?;
                    tx.execute(
                        "INSERT INTO headers VALUES(?1,?2,?3,?4,?5)",
                        params![t, a, header.revision as i64, hash(&bytes), bytes],
                    )?;
                    tx.execute("INSERT INTO vaults VALUES(?1,?2,?3,?4) ON CONFLICT(tenant,owner) DO UPDATE SET header=excluded.header",params![t,a,header.binding.vault_id.to_string(),bytes])?;
                    for item in items {
                        save_item(&tx, owner, item)?;
                    }
                }
                Operation::Put { item, .. } => save_item(&tx, owner, item)?,
                Operation::Delete { item, .. } => {
                    tx.execute(
                        "UPDATE items SET deleted=1 WHERE tenant=?1 AND owner=?2 AND item=?3",
                        params![t, a, item.to_string()],
                    )?;
                }
            }
            receipt.state = "committed".into();
        }
        // Compute exact post-adoption receipts inside the same transaction.
        let state = state_in(&tx, owner)?;
        receipt.header_sha256 = state.0;
        receipt.inventory_sha256 = state.1;
        tx.execute(
            "UPDATE operations SET receipt=?4,status=?5 WHERE tenant=?1 AND owner=?2 AND id=?3",
            params![t, a, id.to_string(), json(&receipt)?, receipt.state],
        )?;
        tx.execute(
            "UPDATE meta SET sequence=?1,head=?2",
            params![next.0, next.1.as_slice()],
        )?;
        tx.commit()?;
        self.adopt_head(next);
        Ok(receipt)
    }
    pub fn cancel(
        &mut self,
        owner: &Owner,
        id: uuid::Uuid,
        candidate: Operation,
    ) -> Result<Receipt> {
        self.ready()?;
        if candidate.id() != id || id.is_nil() {
            return Err(Error::Invalid);
        }
        let body = json(&candidate)?;
        let digest = hash(&body);
        let (existing, mut receipt) = match self.operation(owner, id) {
            Ok((op, r)) => {
                if r.payload_sha256 != digest {
                    return Err(Error::Conflict);
                }
                (Some(op), r)
            }
            Err(Error::NotFound) => {
                // A cancel may win before a held prepare request reaches this DB.
                // Retain its exact immutable body/nonce reservations and tombstone.
                match &candidate {
                    Operation::Key { header, items, .. } => {
                        if header.binding.owner_id != owner.account
                            || header.binding.tenant_id != owner.tenant
                            || items.len() > MAX_ITEMS
                        {
                            return Err(Error::Invalid);
                        }
                        header.to_json().map_err(|_| Error::Invalid)?;
                        for i in items {
                            i.to_json().map_err(|_| Error::Invalid)?;
                        }
                    }
                    Operation::Put { item, .. } => {
                        item.to_json().map_err(|_| Error::Invalid)?;
                    }
                    Operation::Delete { .. } => {}
                }
                let state = self.state(owner)?;
                let (t, a) = owner_key(owner);
                let(count,used):(i64,i64)=self.conn.query_row("SELECT count(*),coalesce(sum(length(body)),0) FROM operations WHERE tenant=?1 AND owner=?2",params![t,a],|r|Ok((r.get(0)?,r.get(1)?)))?;
                if count >= 100000
                    || used
                        .checked_add(body.len() as i64)
                        .is_none_or(|n| n > MAX_RETAINED as i64)
                {
                    return Err(Error::Limit);
                }
                (
                    None,
                    Receipt {
                        operation: id,
                        payload_sha256: digest.clone(),
                        state: "pending".into(),
                        header_sha256: state.header_sha256,
                        inventory_sha256: state.inventory_sha256,
                        commitment: None,
                    },
                )
            }
            Err(e) => return Err(e),
        };
        if receipt.state != "pending" {
            return Ok(receipt);
        }
        if existing.is_none() {
            let tx = self.conn.unchecked_transaction()?;
            reserve(&tx, owner, &candidate)?;
            tx.rollback()?;
        }
        let next = self.append(&Intent::Cancel {
            owner: owner.clone(),
            operation: candidate.clone(),
            digest,
        })?;
        receipt.state = "aborted".into();
        let (t, a) = owner_key(owner);
        let tx = self.conn.transaction()?;
        if existing.is_none() {
            reserve(&tx, owner, &candidate)?;
            tx.execute(
                "INSERT INTO operations VALUES(?1,?2,?3,?4,?5,?6,'aborted')",
                params![t, a, id.to_string(), owner.epoch, body, json(&receipt)?],
            )?;
        } else {
            tx.execute("UPDATE operations SET receipt=?4,status='aborted' WHERE tenant=?1 AND owner=?2 AND id=?3",params![t,a,id.to_string(),json(&receipt)?])?;
        }
        tx.execute(
            "UPDATE meta SET sequence=?1,head=?2",
            params![next.0, next.1.as_slice()],
        )?;
        tx.commit()?;
        self.adopt_head(next);
        Ok(receipt)
    }
}
fn state_in(conn: &Connection, owner: &Owner) -> Result<(String, String)> {
    let (t, a) = owner_key(owner);
    let header: Option<Vec<u8>> = conn
        .query_row(
            "SELECT header FROM vaults WHERE tenant=?1 AND owner=?2",
            params![t, a],
            |r| r.get(0),
        )
        .optional()?;
    let mut stmt = conn.prepare(
        "SELECT ciphertext FROM items WHERE tenant=?1 AND owner=?2 AND deleted=0 ORDER BY item",
    )?;
    let items = stmt
        .query_map(params![t, a], |r| r.get::<_, Vec<u8>>(0))?
        .map(|r| EncryptedItem::from_json(&r?).map_err(|_| Error::Unavailable))
        .collect::<Result<Vec<_>>>()?;
    Ok((
        header.map(|h| hash(&h)).unwrap_or_default(),
        inventory(&items)?,
    ))
}
fn save_item(conn: &Connection, owner: &Owner, item: &EncryptedItem) -> Result<()> {
    let (t, a) = owner_key(owner);
    let b = item.to_json().map_err(|_| Error::Invalid)?;
    conn.execute(
        "INSERT INTO item_versions VALUES(?1,?2,?3,?4,?5)",
        params![t, a, item.id.to_string(), item.revision as i64, b],
    )?;
    conn.execute("INSERT INTO items VALUES(?1,?2,?3,?4,?5,0) ON CONFLICT(tenant,owner,item) DO UPDATE SET revision=excluded.revision,ciphertext=excluded.ciphertext",params![t,a,item.id.to_string(),item.revision as i64,b])?;
    Ok(())
}
fn validate_operation(owner: &Owner, current: &VaultState, op: &Operation) -> Result<()> {
    match op {
        Operation::Key {
            id,
            expected_header,
            expected_inventory,
            header,
            items,
            commitment,
        } => {
            if header.binding.owner_id != owner.account
                || header.binding.tenant_id != owner.tenant
                || items.len() > MAX_ITEMS
            {
                return Err(Error::Invalid);
            }
            header.to_json().map_err(|_| Error::Invalid)?;
            if expected_header.as_deref()
                != current
                    .header
                    .as_ref()
                    .map(|_| current.header_sha256.as_str())
                || expected_inventory != &current.inventory_sha256
            {
                return Err(Error::Conflict);
            }
            let parent = if let Some(h) = &current.header {
                if header.binding != h.binding
                    || header.revision != h.revision.checked_add(1).ok_or(Error::Limit)?
                    || header.envelope_epoch
                        != h.envelope_epoch.checked_add(1).ok_or(Error::Limit)?
                {
                    return Err(Error::Conflict);
                }
                if header.generation == h.generation {
                    if !items.is_empty() {
                        return Err(Error::Invalid);
                    }
                } else if header.generation == h.generation.checked_add(1).ok_or(Error::Limit)? {
                    if items.len() != current.items.len() {
                        return Err(Error::Conflict);
                    }
                    for (old, new) in current.items.iter().zip(items) {
                        if old.id != new.id
                            || new.revision != old.revision.checked_add(1).ok_or(Error::Limit)?
                            || new.generation != header.generation
                        {
                            return Err(Error::Conflict);
                        }
                    }
                } else {
                    return Err(Error::Conflict);
                }
                unhex(&current.header_sha256)?
            } else {
                if header.revision != 1
                    || header.generation != 1
                    || header.envelope_epoch != 1
                    || !items.is_empty()
                {
                    return Err(Error::Invalid);
                }
                [0; 32]
            };
            for item in items {
                item.to_json().map_err(|_| Error::Invalid)?;
            }
            let wanted =
                storage_commitment(*id, header, items, parent).map_err(|_| Error::Invalid)?;
            if unhex(commitment)? != wanted {
                return Err(Error::Conflict);
            }
        }
        Operation::Put {
            expected_header,
            expected_revision,
            item,
            ..
        } => {
            let h = current.header.as_ref().ok_or(Error::Conflict)?;
            item.to_json().map_err(|_| Error::Invalid)?;
            if expected_header != &current.header_sha256
                || item.generation != h.generation
                || item.revision != expected_revision.checked_add(1).ok_or(Error::Limit)?
            {
                return Err(Error::Conflict);
            }
            let old = current.items.iter().find(|i| i.id == item.id);
            if old.map_or(0, |i| i.revision) != *expected_revision {
                return Err(Error::Conflict);
            }
            if old.is_none() && current.items.len() >= MAX_ITEMS {
                return Err(Error::Limit);
            }
        }
        Operation::Delete {
            expected_header,
            item,
            expected_revision,
            ..
        } => {
            if expected_header != &current.header_sha256
                || !current
                    .items
                    .iter()
                    .any(|i| i.id == *item && i.revision == *expected_revision)
            {
                return Err(Error::Conflict);
            }
        }
    }
    Ok(())
}
fn reserve(conn: &Connection, owner: &Owner, op: &Operation) -> Result<()> {
    let (t, a) = owner_key(owner);
    let mut values = vec![];
    match op {
        Operation::Key { header, items, .. } => {
            let v: serde_json::Value =
                serde_json::from_slice(&header.to_json().map_err(|_| Error::Invalid)?)
                    .map_err(|_| Error::Invalid)?;
            for env in v["envelopes"].as_array().ok_or(Error::Invalid)? {
                values.push((
                    0,
                    "envelope",
                    env["nonce"].as_str().ok_or(Error::Invalid)?.to_owned(),
                    hash(&json(env)?),
                ));
            }
            for i in items {
                item_nonces(i, &mut values)?;
            }
        }
        Operation::Put { item, .. } => {
            let deleted:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM items WHERE tenant=?1 AND owner=?2 AND item=?3 AND deleted=1)",params![t,a,item.id.to_string()],|r|r.get(0))?;
            if deleted {
                return Err(Error::Conflict);
            }
            item_nonces(item, &mut values)?;
        }
        Operation::Delete { .. } => {}
    }
    for (g, domain, nonce, digest) in values {
        let old:Option<String>=conn.query_row("SELECT digest FROM nonce_ledger WHERE tenant=?1 AND owner=?2 AND generation=?3 AND domain=?4 AND nonce=?5",params![t,a,g as i64,domain,nonce],|r|r.get(0)).optional()?;
        if old.as_ref().is_some_and(|v| v != &digest) {
            return Err(Error::Conflict);
        }
        conn.execute(
            "INSERT OR IGNORE INTO nonce_ledger VALUES(?1,?2,?3,?4,?5,?6)",
            params![t, a, g as i64, domain, nonce, digest],
        )?;
    }
    Ok(())
}
fn item_nonces(
    item: &EncryptedItem,
    out: &mut Vec<(u64, &'static str, String, String)>,
) -> Result<()> {
    let identity = json(&(item.id, item.revision, item.generation, &item.wrapped_key))?;
    out.push((
        item.generation,
        "root_wrap",
        item.key_nonce.clone(),
        hash(&identity),
    ));
    out.push((
        item.generation,
        "payload",
        item.nonce.clone(),
        hash(&item.to_json().map_err(|_| Error::Invalid)?),
    ));
    Ok(())
}
