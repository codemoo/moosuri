//! Native enforcement foundation. No network adapter or credential unlocking is shipped.
pub mod auth;
mod journal;
pub mod policy;
use moosuri_vault_protocol::broker::*;
use serde::{Deserialize, Serialize};
use std::path::Path;
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("broker authentication failed")]
    Authentication,
    #[error("broker local policy denied")]
    Policy,
    #[error("broker private state unavailable")]
    PrivateState,
    #[error("broker is already owned")]
    Locked,
    #[error("broker operation cannot be retried")]
    Unknown,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Phase {
    Prepared,
    RetainedPrepared,
    Committed(Commitment),
    RetainedCommitted(Commitment),
    Entered(Commitment),
    Settled(SignedReceipt),
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    request: Request,
    phase: Phase,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    device: Device,
    origin: String,
    public_key: Vec<u8>,
    grants: Vec<policy::LocalGrant>,
    entries: Vec<Entry>,
    revoked: bool,
}
pub struct Engine {
    state: State,
    journal: journal::Journal,
    key: auth::SigningKey,
    poisoned: bool,
}
impl Engine {
    /// New pairing only. Existing/missing/damaged state must never call this as recovery.
    pub fn enroll(path: &Path, device: Device, origin: String, pkcs8: &[u8]) -> Result<Self> {
        if device.generation == 0 || device.id.is_nil() || !auth::valid_origin(&origin) {
            return Err(Error::Policy);
        }
        let key = auth::SigningKey::from_pkcs8(pkcs8)?;
        let state = State {
            device,
            origin,
            public_key: key.public_key(),
            grants: vec![],
            entries: vec![],
            revoked: false,
        };
        let journal = journal::Journal::create(path, pkcs8, &state)?;
        Ok(Self {
            state,
            journal,
            key,
            poisoned: false,
        })
    }
    /// Caller pins enrollment identity independently from the retained local journal.
    /// Startup never dispatches retained work; it must reconcile original commitments.
    pub fn reopen(path: &Path, device: &Device, origin: &str, public_key: &[u8]) -> Result<Self> {
        let (journal, bytes, state): (journal::Journal, zeroize::Zeroizing<Vec<u8>>, State) =
            journal::Journal::reopen(path)?;
        let key = auth::SigningKey::from_pkcs8(&bytes)?;
        if &state.device != device
            || state.origin != origin
            || state.public_key != public_key
            || key.public_key() != public_key
        {
            return Err(Error::PrivateState);
        }
        let mut engine = Self {
            state,
            journal,
            key,
            poisoned: false,
        };
        // Retained work can only reconcile. Restart is never adapter permission.
        for entry in &mut engine.state.entries {
            entry.phase = match &entry.phase {
                Phase::Prepared => Phase::RetainedPrepared,
                Phase::Committed(c) => Phase::RetainedCommitted(c.clone()),
                other => other.clone(),
            };
        }
        engine.save()?;
        Ok(engine)
    }
    fn save(&mut self) -> Result<()> {
        if self.poisoned {
            return Err(Error::PrivateState);
        }
        if self.journal.append(&self.state).is_err() {
            self.poisoned = true;
            return Err(Error::PrivateState);
        }
        Ok(())
    }
    pub fn select(&mut self, grant: policy::LocalGrant) -> Result<Binding> {
        if self.poisoned
            || self.state.revoked
            || grant.device != self.state.device
            || self.state.grants.len() >= 256
            || self
                .state
                .grants
                .iter()
                .any(|g| g.binding.id == grant.binding.id)
        {
            return Err(Error::Policy);
        }
        let b = grant.binding.clone();
        self.state.grants.push(grant);
        self.save()?;
        Ok(b)
    }
    pub fn prepare(
        &mut self,
        request: Request,
        now: i64,
        item_epoch: u64,
        key_epoch: u64,
    ) -> Result<()> {
        if request.invocation.run.is_nil()
            || request.invocation.call.is_nil()
            || request.invocation.nonce.is_nil()
            || request.invocation.step == 0
        {
            return Err(Error::Policy);
        }
        if self.poisoned || self.state.revoked || self.state.entries.len() >= 1024 {
            return Err(Error::Policy);
        }
        let grant = self
            .state
            .grants
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?;
        grant.check(&request, now, item_epoch, key_epoch)?;
        if let Some(e) = self
            .state
            .entries
            .iter()
            .find(|e| e.request.invocation == request.invocation)
        {
            return if e.request == request {
                Ok(())
            } else {
                Err(Error::Policy)
            };
        }
        self.state.entries.push(Entry {
            request,
            phase: Phase::Prepared,
        });
        self.save()
    }
    pub fn accept_commitment(&mut self, c: Commitment) -> Result<()> {
        if self.poisoned || self.state.revoked || c.nonce.is_nil() {
            return Err(Error::Policy);
        }
        let e = self
            .state
            .entries
            .iter_mut()
            .find(|e| e.request.invocation == c.request.invocation)
            .ok_or(Error::Policy)?;
        if e.request != c.request {
            return Err(Error::Policy);
        }
        match &e.phase {
            Phase::Prepared => e.phase = Phase::Committed(c),
            Phase::RetainedPrepared => e.phase = Phase::RetainedCommitted(c),
            Phase::RetainedCommitted(old) if old == &c => return Ok(()),
            Phase::Committed(old) if old == &c => return Ok(()),
            _ => return Err(Error::Unknown),
        }
        self.save()
    }
    pub fn sign_command(
        &self,
        c: &AuthChallenge,
        method: &str,
        path: &str,
        command: Command,
        now: i64,
    ) -> Result<SignedCommand> {
        if self.poisoned
            || c.device != self.state.device
            || c.origin != self.state.origin
            || c.expires_at <= now
            || c.expires_at > now + 60
        {
            return Err(Error::Authentication);
        }
        match &command {
            Command::Register { binding } => {
                if self.state.revoked || !self.state.grants.iter().any(|g| &g.binding == binding) {
                    return Err(Error::Policy);
                }
            }
            Command::Commit { invocation } => {
                if self.state.revoked {
                    return Err(Error::Policy);
                }
                let entry = self
                    .state
                    .entries
                    .iter()
                    .find(|e| &e.request.invocation == invocation)
                    .ok_or(Error::Policy)?;
                if !matches!(entry.phase, Phase::Prepared | Phase::Committed(_)) {
                    return Err(Error::Unknown);
                }
                self.state
                    .grants
                    .iter()
                    .find(|g| g.binding.id == entry.request.binding.id)
                    .ok_or(Error::Policy)?
                    .check(
                        &entry.request,
                        now,
                        entry.request.binding.item_epoch,
                        entry.request.binding.key_epoch,
                    )?;
            }
            Command::Recover { invocation } => {
                if !self
                    .state
                    .entries
                    .iter()
                    .any(|e| &e.request.invocation == invocation)
                {
                    return Err(Error::Policy);
                }
            }
            Command::Settle { receipt } => {
                let own = self
                    .receipt(&receipt.receipt.commitment.request.invocation)?
                    .ok_or(Error::Policy)?;
                if own.receipt != receipt.receipt || own.signature != receipt.signature {
                    return Err(Error::Policy);
                }
            }
            Command::AckRevoke => {
                if !self.state.revoked {
                    return Err(Error::Policy);
                }
            }
        }
        Ok(SignedCommand {
            challenge: c.nonce,
            signature: self.key.sign(
                &auth::authenticated_bytes(c, method, path, &command)
                    .ok_or(Error::Authentication)?,
            ),
            command,
        })
    }
    /// Immediately fences local entry. This is not upstream provider revocation.
    pub fn revoke(&mut self) -> Result<()> {
        self.state.revoked = true;
        for g in &mut self.state.grants {
            g.revoke()
        }
        self.save()
    }
    pub fn receipt(&self, invocation: &Invocation) -> Result<Option<SignedReceipt>> {
        if self.poisoned {
            return Err(Error::PrivateState);
        }
        let e = self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?;
        Ok(match &e.phase {
            Phase::Settled(r) => Some(r.clone()),
            _ => None,
        })
    }
    /// Positively known not entered, or unknown after entry. Never converts a
    /// disconnect/timeout into evidence that bytes did not leave the device.
    pub fn settle_without_retry(&mut self, invocation: &Invocation) -> Result<SignedReceipt> {
        if self.poisoned {
            return Err(Error::PrivateState);
        }
        let e = self
            .state
            .entries
            .iter_mut()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?;
        let (c, outcome) = match &e.phase {
            Phase::Committed(c) | Phase::RetainedCommitted(c) => {
                (c.clone(), Outcome::NotDispatched)
            }
            Phase::Entered(c) => (c.clone(), Outcome::Unknown),
            Phase::Settled(r) => return Ok(r.clone()),
            Phase::Prepared | Phase::RetainedPrepared => return Err(Error::Unknown),
        };
        let r = Receipt {
            commitment: c,
            outcome,
            projection: Projection::None,
            projection_digest: auth::projection_digest(Projection::None),
        };
        let r = SignedReceipt {
            signature: self.key.sign(&receipt_bytes(&r)),
            receipt: r,
        };
        e.phase = Phase::Settled(r.clone());
        self.save()?;
        Ok(r)
    }
    #[cfg(test)]
    fn begin_dispatch(
        &mut self,
        invocation: &Invocation,
        now: i64,
        item_epoch: u64,
        key_epoch: u64,
    ) -> Result<Commitment> {
        if self.poisoned || self.state.revoked {
            return Err(Error::Policy);
        }
        let e = self
            .state
            .entries
            .iter_mut()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?;
        self.state
            .grants
            .iter()
            .find(|g| g.binding.id == e.request.binding.id)
            .ok_or(Error::Policy)?
            .check(&e.request, now, item_epoch, key_epoch)?;
        let c = match &e.phase {
            Phase::Committed(c) => c.clone(),
            _ => return Err(Error::Unknown),
        };
        // fsync occurs before adapter entry. A crash here is conservatively unknown.
        e.phase = Phase::Entered(c.clone());
        self.save()?;
        Ok(c)
    }
}
#[cfg(test)]
mod tests;
