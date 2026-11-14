//! Versioned nonsecret broker metadata. No credential, provider body or arbitrary URL.
use crate::Owner;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEVICE_PATH: &str = "/api/v1/credential-broker/device/command";
pub const MAX_SIGNED_BYTES: usize = 16_384;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub owner: Owner,
    pub id: Uuid,
    pub generation: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairChallenge {
    pub device: Device,
    pub origin: String,
    pub public_key: Vec<u8>,
    pub nonce: Uuid,
    pub expires_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairProof {
    pub challenge: Uuid,
    pub signature: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthChallenge {
    pub device: Device,
    pub origin: String,
    pub nonce: Uuid,
    pub expires_at: i64,
}
/// There is deliberately no generic HTTP, secret-fill or arbitrary write operation.
/// FixedProbe is synthetic foundation evidence, never a registered production capability.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    FixedProbe,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Invocation {
    pub run: Uuid,
    pub step: u32,
    pub call: Uuid,
    pub nonce: Uuid,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub id: Uuid,
    pub epoch: u64,
    pub grant_version: u64,
    pub grant_hash: [u8; 32],
    pub item_epoch: u64,
    pub key_epoch: u64,
    pub expires_at: i64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub device: Device,
    pub invocation: Invocation,
    pub binding: Binding,
    pub operation: Operation,
    pub digest: [u8; 32],
    pub expires_at: i64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Commitment {
    pub request: Request,
    pub nonce: Uuid,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Unknown,
    NotDispatched,
    Rejected,
    ProjectedSuccess,
}
/// Projection is reconstructed from an allowlist; no strings from an adapter.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    None,
    FixedProbeOk,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub commitment: Commitment,
    pub outcome: Outcome,
    pub projection: Projection,
    pub projection_digest: [u8; 32],
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedReceipt {
    pub receipt: Receipt,
    pub signature: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Register { binding: Binding },
    Commit { invocation: Invocation },
    Recover { invocation: Invocation },
    Settle { receipt: Box<SignedReceipt> },
    AckRevoke,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCommand {
    pub challenge: Uuid,
    pub command: Command,
    pub signature: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Reply {
    Registered,
    Commitment { commitment: Box<Commitment> },
    Settled,
    Revoked,
}

/// Explicit field-order, length-prefixed binary encoding. Never JSON/map canonicalization.
struct Bytes(Vec<u8>);
impl Bytes {
    fn new(domain: &[u8]) -> Self {
        let mut b = Self(Vec::new());
        b.blob(b"moosuri/credential-broker/v1");
        b.blob(domain);
        b
    }
    fn blob(&mut self, b: &[u8]) {
        self.0.extend_from_slice(&(b.len() as u32).to_be_bytes());
        self.0.extend_from_slice(b);
    }
    fn id(&mut self, id: Uuid) {
        self.0.extend_from_slice(id.as_bytes());
    }
    fn u(&mut self, n: u64) {
        self.0.extend_from_slice(&n.to_be_bytes());
    }
    fn device(&mut self, d: &Device) {
        self.id(d.owner.tenant);
        self.id(d.owner.account);
        self.id(d.owner.installation);
        self.0.extend_from_slice(&d.owner.epoch.to_be_bytes());
        self.id(d.id);
        self.u(d.generation);
    }
    fn invocation(&mut self, i: &Invocation) {
        self.id(i.run);
        self.0.extend_from_slice(&i.step.to_be_bytes());
        self.id(i.call);
        self.id(i.nonce);
    }
    fn binding(&mut self, b: &Binding) {
        self.id(b.id);
        self.u(b.epoch);
        self.u(b.grant_version);
        self.0.extend_from_slice(&b.grant_hash);
        self.u(b.item_epoch);
        self.u(b.key_epoch);
        self.0.extend_from_slice(&b.expires_at.to_be_bytes());
    }
    fn request(&mut self, r: &Request) {
        self.device(&r.device);
        self.invocation(&r.invocation);
        self.binding(&r.binding);
        self.0.push(1);
        self.0.extend_from_slice(&r.digest);
        self.0.extend_from_slice(&r.expires_at.to_be_bytes());
    }
    fn receipt(&mut self, r: &Receipt) {
        self.request(&r.commitment.request);
        self.id(r.commitment.nonce);
        self.0.push(match r.outcome {
            Outcome::Unknown => 0,
            Outcome::NotDispatched => 1,
            Outcome::Rejected => 2,
            Outcome::ProjectedSuccess => 3,
        });
        self.0.push(match r.projection {
            Projection::None => 0,
            Projection::FixedProbeOk => 1,
        });
        self.0.extend_from_slice(&r.projection_digest);
    }
}
pub fn pairing_bytes(c: &PairChallenge) -> Option<Vec<u8>> {
    if c.origin.len() > 256 || c.public_key.len() != 32 {
        return None;
    }
    let mut b = Bytes::new(b"pair");
    b.device(&c.device);
    b.blob(c.origin.as_bytes());
    b.blob(&c.public_key);
    b.id(c.nonce);
    b.0.extend_from_slice(&c.expires_at.to_be_bytes());
    Some(b.0)
}
pub fn receipt_bytes(r: &Receipt) -> Vec<u8> {
    let mut b = Bytes::new(b"receipt");
    b.receipt(r);
    b.0
}
pub fn request_bytes(r: &Request) -> Vec<u8> {
    let mut b = Bytes::new(b"request");
    b.request(r);
    b.0
}
pub fn command_bytes(
    c: &AuthChallenge,
    method: &str,
    path: &str,
    command: &Command,
) -> Option<Vec<u8>> {
    if c.origin.len() > 256 || method.len() > 8 || path.len() > 128 {
        return None;
    }
    let mut body = Bytes::new(b"command-body");
    match command {
        Command::Register { binding } => {
            body.0.push(0);
            body.binding(binding)
        }
        Command::Commit { invocation } => {
            body.0.push(1);
            body.invocation(invocation)
        }
        Command::Recover { invocation } => {
            body.0.push(2);
            body.invocation(invocation)
        }
        Command::Settle { receipt } => {
            if receipt.signature.len() != 64 {
                return None;
            }
            body.0.push(3);
            body.receipt(&receipt.receipt);
            body.blob(&receipt.signature)
        }
        Command::AckRevoke => body.0.push(4),
    }
    // The body itself is bounded and length-prefixed, thus binds at least as strongly
    // as a digest. Native auth also exposes its SHA256 for transport diagnostics.
    let mut b = Bytes::new(b"authenticated-request");
    b.device(&c.device);
    b.blob(c.origin.as_bytes());
    b.id(c.nonce);
    b.0.extend_from_slice(&c.expires_at.to_be_bytes());
    b.blob(method.as_bytes());
    b.blob(path.as_bytes());
    b.blob(&body.0);
    (b.0.len() <= MAX_SIGNED_BYTES).then_some(b.0)
}
