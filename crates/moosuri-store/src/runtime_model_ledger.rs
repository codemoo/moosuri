//! Installation-owned, privacy-minimal model dispatch ledger. No prompts,
//! responses, provider response IDs, credentials or unkeyed content hashes.
use crate::{Result, StoreError, files};
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::{fs, os::unix::fs::MetadataExt, path::Path, time::Duration};
use uuid::Uuid;

const APP_ID: i64 = 0x4d534d4c;
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub max_calls: u64,
    pub max_tokens: u64,
    /// Held until exact provider usage is known; unknown/missing usage retains it.
    pub reservation_tokens: u64,
}
impl Budget {
    pub fn validate(self) -> Result<()> {
        if self.max_calls == 0
            || self.max_calls > 1_000_000
            || self.max_tokens == 0
            || self.max_tokens > 1_000_000_000_000
            || self.reservation_tokens == 0
            || self.reservation_tokens > self.max_tokens
        {
            return Err(StoreError::InvalidInput);
        }
        Ok(())
    }
}
// Deliberately no Debug on keyed request fingerprints or authority carriers.
pub struct CallKey {
    pub call_id: Uuid,
    pub account_id: Uuid,
    pub tenant_id: Uuid,
    pub generation: i64,
    pub run_id: Uuid,
    pub ordinal: u32,
    pub connection_revision: Uuid,
    pub fingerprint: [u8; 32],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Reserved,
    Dispatched,
    Settled,
    Unknown,
}
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    pub state: State,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub charged_tokens: u64,
}
pub struct Permit {
    call_id: String,
    nonce: String,
}
pub enum Admission {
    Fresh(Permit),
    Existing(Receipt),
}
pub struct Ledger {
    _directory: fs::File,
    db: Connection,
}

fn state(s: String) -> rusqlite::Result<State> {
    match s.as_str() {
        "reserved" => Ok(State::Reserved),
        "dispatched" => Ok(State::Dispatched),
        "settled" => Ok(State::Settled),
        "unknown" => Ok(State::Unknown),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}
fn unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(row.get::<_, i64>(column)?).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn optional_unsigned(row: &rusqlite::Row<'_>, column: usize) -> rusqlite::Result<Option<u64>> {
    row.get::<_, Option<i64>>(column)?
        .map(|v| u64::try_from(v).map_err(|_| rusqlite::Error::InvalidQuery))
        .transpose()
}
fn receipt(row: &rusqlite::Row<'_>) -> rusqlite::Result<Receipt> {
    Ok(Receipt {
        state: state(row.get(0)?)?,
        input_tokens: optional_unsigned(row, 1)?,
        output_tokens: optional_unsigned(row, 2)?,
        charged_tokens: unsigned(row, 3)?,
    })
}
impl Ledger {
    /// Dedicated pre-created private directory, separate from the account registry
    /// and all personal volumes. One broker holds its directory lease until exit.
    pub fn open(root: &Path) -> Result<Self> {
        let m = fs::symlink_metadata(root).map_err(|_| StoreError::Unavailable)?;
        if !root.is_absolute()
            || !m.is_dir()
            || m.uid() != unsafe { libc::geteuid() }
            || m.mode() & 0o077 != 0
            || fs::canonicalize(root).ok().as_deref() != Some(root)
        {
            return Err(StoreError::InvalidInput);
        }
        let directory = fs::File::open(root).map_err(|_| StoreError::Unavailable)?;
        directory
            .try_lock_exclusive()
            .map_err(|_| StoreError::Busy)?;
        for e in fs::read_dir(root).map_err(|_| StoreError::Unavailable)? {
            let e = e.map_err(|_| StoreError::Unavailable)?;
            let m = fs::symlink_metadata(e.path()).map_err(|_| StoreError::InvalidInput)?;
            if !matches!(
                e.file_name().to_str(),
                Some("model-calls.sqlite" | "model-calls.sqlite-journal")
            ) || !m.is_file()
                || m.nlink() != 1
                || m.uid() != unsafe { libc::geteuid() }
                || m.mode() & 0o077 != 0
            {
                return Err(StoreError::InvalidInput);
            }
        }
        let path = root.join("model-calls.sqlite");
        files::private_file(&path)?;
        let mut db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.execute_batch(
            "PRAGMA trusted_schema=OFF; PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;",
        )?;
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id: i64 = tx.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        if id == 0 {
            let existing: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema)", [], |r| {
                    r.get(0)
                })?;
            if existing {
                return Err(StoreError::InvalidInput);
            }
            tx.execute_batch("CREATE TABLE calls (
                call_id TEXT PRIMARY KEY NOT NULL, account_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
                generation INTEGER NOT NULL CHECK(generation>0), run_id TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK(ordinal>=0),
                connection_revision TEXT NOT NULL, fingerprint BLOB NOT NULL CHECK(length(fingerprint)=32), nonce TEXT NOT NULL,
                state TEXT NOT NULL CHECK(state IN ('reserved','dispatched','settled','unknown')),
                reservation INTEGER NOT NULL CHECK(reservation>0), charged_tokens INTEGER NOT NULL CHECK(charged_tokens>=0),
                input_tokens INTEGER CHECK(input_tokens>=0), output_tokens INTEGER CHECK(output_tokens>=0),
                UNIQUE(account_id,generation,run_id,ordinal)
            ) STRICT; CREATE INDEX calls_account ON calls(account_id); PRAGMA user_version=1;")?;
            tx.pragma_update(None, "application_id", APP_ID)?;
        } else if id != APP_ID {
            return Err(StoreError::InvalidInput);
        }
        let version: i64 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != 1 {
            return Err(StoreError::InvalidInput);
        }
        // A prior process may have handed the request to a provider. Neither a
        // restart nor a missing receipt is permission to dispatch it again.
        tx.execute(
            "UPDATE calls SET state='unknown' WHERE state IN ('reserved','dispatched')",
            [],
        )?;
        tx.commit()?;
        directory.sync_all().map_err(|_| StoreError::Unavailable)?;
        Ok(Self {
            _directory: directory,
            db,
        })
    }
    pub fn admit(&mut self, key: &CallKey, budget: Budget) -> Result<Admission> {
        self.admit_if_ready(key, budget, true)
    }
    /// An unavailable credential can still retrieve an existing receipt. It
    /// cannot create a new reservation or consume the account's budget.
    pub fn admit_if_ready(
        &mut self,
        key: &CallKey,
        budget: Budget,
        provider_ready: bool,
    ) -> Result<Admission> {
        budget.validate()?;
        if key.generation <= 0 || key.ordinal > 1_000_000 {
            return Err(StoreError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing=tx.query_row("SELECT account_id,tenant_id,generation,run_id,ordinal,connection_revision,fingerprint,state,input_tokens,output_tokens,charged_tokens FROM calls WHERE call_id=?1",[key.call_id.to_string()],|r| {
            Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,u32>(4)?,r.get::<_,String>(5)?,r.get::<_,Vec<u8>>(6)?,Receipt{state:state(r.get(7)?)?,input_tokens:optional_unsigned(r,8)?,output_tokens:optional_unsigned(r,9)?,charged_tokens:unsigned(r,10)?}))
        }).optional()?;
        if let Some((a, t, g, r, o, v, f, receipt)) = existing {
            if a != key.account_id.to_string()
                || t != key.tenant_id.to_string()
                || g != key.generation
                || r != key.run_id.to_string()
                || o != key.ordinal
                || v != key.connection_revision.to_string()
                || f != key.fingerprint
            {
                return Err(StoreError::Conflict);
            }
            return Ok(Admission::Existing(receipt));
        }
        let collision:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM calls WHERE account_id=?1 AND generation=?2 AND run_id=?3 AND ordinal=?4)",params![key.account_id.to_string(),key.generation,key.run_id.to_string(),key.ordinal],|r|r.get(0))?;
        if collision {
            return Err(StoreError::Conflict);
        }
        if !provider_ready {
            return Err(StoreError::ModelRequired);
        }
        let (calls, tokens): (u64, u64) = tx.query_row(
            "SELECT count(*),coalesce(sum(charged_tokens),0) FROM calls WHERE account_id=?1",
            [key.account_id.to_string()],
            |r| Ok((unsigned(r, 0)?, unsigned(r, 1)?)),
        )?;
        if calls >= budget.max_calls
            || tokens
                .checked_add(budget.reservation_tokens)
                .is_none_or(|n| n > budget.max_tokens)
        {
            return Err(StoreError::BudgetExhausted);
        }
        let permit = Permit {
            call_id: key.call_id.to_string(),
            nonce: Uuid::new_v4().to_string(),
        };
        tx.execute(
            "INSERT INTO calls VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'reserved',?10,?10,NULL,NULL)",
            params![
                permit.call_id,
                key.account_id.to_string(),
                key.tenant_id.to_string(),
                key.generation,
                key.run_id.to_string(),
                key.ordinal,
                key.connection_revision.to_string(),
                key.fingerprint.as_slice(),
                permit.nonce,
                budget.reservation_tokens as i64
            ],
        )?;
        tx.commit()?;
        Ok(Admission::Fresh(permit))
    }
    /// Commit dispatch intent before even beginning HTTP/DNS. The permit exists
    /// only in the reserving process and can transition this call once.
    pub fn dispatch(&mut self, permit: &Permit) -> Result<()> {
        if self.db.execute("UPDATE calls SET state='dispatched' WHERE call_id=?1 AND nonce=?2 AND state='reserved'",params![permit.call_id,permit.nonce])?!=1 {return Err(StoreError::Conflict);}
        Ok(())
    }
    pub fn unknown(&mut self, permit: &Permit) -> Result<Receipt> {
        self.uncertain(permit, None, None)
    }
    pub fn uncertain(
        &mut self,
        permit: &Permit,
        input: Option<u64>,
        output: Option<u64>,
    ) -> Result<Receipt> {
        if input.is_some_and(|n| n > 1_000_000_000_000)
            || output.is_some_and(|n| n > 1_000_000_000_000)
        {
            return Err(StoreError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old=tx.query_row("SELECT state,input_tokens,output_tokens,charged_tokens FROM calls WHERE call_id=?1 AND nonce=?2",params![permit.call_id,permit.nonce],receipt)?;
        if old.state == State::Settled {
            return Ok(old);
        }
        let input = match (old.input_tokens, input) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        let output = match (old.output_tokens, output) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        let charged = old
            .charged_tokens
            .max(input.unwrap_or(0) + output.unwrap_or(0));
        tx.execute("UPDATE calls SET state='unknown',input_tokens=?3,output_tokens=?4,charged_tokens=?5 WHERE call_id=?1 AND nonce=?2",params![permit.call_id,permit.nonce,input.map(|n|n as i64),output.map(|n|n as i64),charged as i64])?;
        tx.commit()?;
        Ok(Receipt {
            state: State::Unknown,
            input_tokens: input,
            output_tokens: output,
            charged_tokens: charged,
        })
    }
    /// Missing input/output usage keeps the full reservation. An anomalous usage
    /// overrun is still recorded in full and blocks subsequent quota admission;
    /// a provider token count is evidence, not a promise of tokenization bounds.
    pub fn settle(
        &mut self,
        permit: &Permit,
        input: Option<u64>,
        output: Option<u64>,
    ) -> Result<Receipt> {
        if input.is_some_and(|n| n > 1_000_000_000_000)
            || output.is_some_and(|n| n > 1_000_000_000_000)
        {
            return Err(StoreError::InvalidInput);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let previous=tx.query_row("SELECT state,input_tokens,output_tokens,charged_tokens FROM calls WHERE call_id=?1 AND nonce=?2",params![permit.call_id,permit.nonce],receipt)?;
        if previous.state == State::Settled {
            if previous.input_tokens == input && previous.output_tokens == output {
                return Ok(previous);
            }
            return Err(StoreError::Conflict);
        }
        if previous.state != State::Dispatched {
            return Err(StoreError::Conflict);
        }
        let charged = match (input, output) {
            (Some(i), Some(o)) => i.checked_add(o).ok_or(StoreError::InvalidInput)?,
            _ => previous
                .charged_tokens
                .max(input.unwrap_or(0) + output.unwrap_or(0)),
        };
        tx.execute("UPDATE calls SET state='settled',input_tokens=?3,output_tokens=?4,charged_tokens=?5 WHERE call_id=?1 AND nonce=?2 AND state='dispatched'",params![permit.call_id,permit.nonce,input.map(|n|n as i64),output.map(|n|n as i64),charged as i64])?;
        tx.commit()?;
        Ok(Receipt {
            state: State::Settled,
            input_tokens: input,
            output_tokens: output,
            charged_tokens: charged,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn root() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
        d
    }
    fn key() -> CallKey {
        CallKey {
            call_id: Uuid::new_v4(),
            account_id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            generation: 1,
            run_id: Uuid::new_v4(),
            ordinal: 0,
            connection_revision: Uuid::new_v4(),
            fingerprint: [7; 32],
        }
    }
    fn budget() -> Budget {
        Budget {
            max_calls: 3,
            max_tokens: 100,
            reservation_tokens: 40,
        }
    }
    #[test]
    fn identities_content_and_ordinals_cannot_be_rebound_and_settlement_is_once_only() {
        let d = root();
        let mut l = Ledger::open(d.path()).unwrap();
        let mut k = key();
        let p = match l.admit(&k, budget()).unwrap() {
            Admission::Fresh(p) => p,
            _ => panic!("new"),
        };
        assert!(matches!(
            l.admit(&k, budget()).unwrap(),
            Admission::Existing(Receipt {
                state: State::Reserved,
                ..
            })
        ));
        let account = k.account_id;
        k.account_id = Uuid::new_v4();
        assert!(matches!(l.admit(&k, budget()), Err(StoreError::Conflict)));
        k.account_id = account;
        k.fingerprint = [8; 32];
        assert!(matches!(l.admit(&k, budget()), Err(StoreError::Conflict)));
        k.fingerprint = [7; 32];
        let id = k.call_id;
        k.call_id = Uuid::new_v4();
        assert!(matches!(l.admit(&k, budget()), Err(StoreError::Conflict)));
        k.call_id = id;
        l.dispatch(&p).unwrap();
        assert!(l.dispatch(&p).is_err());
        assert_eq!(l.settle(&p, Some(7), Some(3)).unwrap().charged_tokens, 10);
        assert_eq!(l.settle(&p, Some(7), Some(3)).unwrap().charged_tokens, 10);
        assert!(l.settle(&p, Some(9), Some(3)).is_err());
    }
    #[test]
    fn restart_preserves_unknown_spend_and_quota_across_generations() {
        let d = root();
        let mut l = Ledger::open(d.path()).unwrap();
        let mut k = key();
        let p = match l.admit(&k, budget()).unwrap() {
            Admission::Fresh(p) => p,
            _ => panic!("new"),
        };
        l.dispatch(&p).unwrap();
        drop(l);
        let mut l = Ledger::open(d.path()).unwrap();
        assert!(matches!(
            l.admit(&k, budget()).unwrap(),
            Admission::Existing(Receipt {
                state: State::Unknown,
                charged_tokens: 40,
                ..
            })
        ));
        k.call_id = Uuid::new_v4();
        k.generation = 2;
        let p = match l.admit(&k, budget()).unwrap() {
            Admission::Fresh(p) => p,
            _ => panic!("new"),
        };
        l.dispatch(&p).unwrap();
        l.settle(&p, None, Some(1)).unwrap();
        k.call_id = Uuid::new_v4();
        k.ordinal += 1;
        assert!(matches!(
            l.admit(&k, budget()),
            Err(StoreError::BudgetExhausted)
        ));
    }
    #[test]
    fn measured_overrun_and_partial_unknown_usage_are_never_discounted() {
        let d = root();
        let mut l = Ledger::open(d.path()).unwrap();
        let mut k = key();
        let p = match l.admit(&k, budget()).unwrap() {
            Admission::Fresh(p) => p,
            _ => panic!("new"),
        };
        l.dispatch(&p).unwrap();
        assert_eq!(
            l.uncertain(&p, Some(90), Some(20)).unwrap().charged_tokens,
            110
        );
        assert_eq!(l.uncertain(&p, None, Some(1)).unwrap().charged_tokens, 110);
        k.call_id = Uuid::new_v4();
        k.ordinal += 1;
        assert!(matches!(
            l.admit(&k, budget()),
            Err(StoreError::BudgetExhausted)
        ));
        assert!(l.settle(&p, Some(90), Some(20)).is_err());
    }
    #[test]
    fn foreign_roots_symlinks_and_parallel_brokers_fail_closed() {
        let d = root();
        let l = Ledger::open(d.path()).unwrap();
        assert!(matches!(Ledger::open(d.path()), Err(StoreError::Busy)));
        drop(l);
        fs::write(d.path().join("private-prompt"), b"not adopted").unwrap();
        assert!(Ledger::open(d.path()).is_err());
        let target = root();
        let link = d.path().join("link");
        std::os::unix::fs::symlink(target.path(), link.clone()).unwrap();
        assert!(Ledger::open(&link).is_err());
    }
}
