//! Versioned credential-free broker messages. Preview text is private untrusted data.
//! No credentials, raw provider bodies or arbitrary request URLs.
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
    GitHubRepositoryRead,
    GoogleInboxRead,
    GoogleInboxPreview,
}
/// Exact, canonical repository selection. Never a URL or HTTP header.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubRepository {
    pub owner: String,
    pub repository: String,
}
impl GitHubRepository {
    pub fn valid(&self) -> bool {
        !self.owner.is_empty()
            && self.owner.len() <= 39
            && !self.owner.starts_with('-')
            && !self.owner.ends_with('-')
            && self
                .owner
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !self.repository.is_empty()
            && self.repository.len() <= 100
            && !matches!(self.repository.as_str(), "." | "..")
            && self
                .repository
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    }
    pub fn bytes(&self) -> Vec<u8> {
        let mut b = Bytes::new(b"github-repository-metadata/v1");
        b.blob(self.owner.as_bytes());
        b.blob(self.repository.as_bytes());
        b.0
    }
}
/// Resource JSON preserves the original untagged GitHub object exactly.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Resource {
    GitHub(GitHubRepository),
    GoogleInbox(GoogleInboxResource),
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleInboxResource {
    pub google_inbox: GoogleInboxVersion,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoogleInboxVersion {
    CountsV1,
    PreviewV1,
}
impl Resource {
    pub fn google_inbox() -> Self {
        Self::GoogleInbox(GoogleInboxResource {
            google_inbox: GoogleInboxVersion::CountsV1,
        })
    }
    pub fn google_preview() -> Self {
        Self::GoogleInbox(GoogleInboxResource {
            google_inbox: GoogleInboxVersion::PreviewV1,
        })
    }
    pub fn valid(&self) -> bool {
        match self {
            Self::GitHub(r) => r.valid(),
            Self::GoogleInbox(_) => true,
        }
    }
    pub fn operation(&self) -> Operation {
        match self {
            Self::GitHub(_) => Operation::GitHubRepositoryRead,
            Self::GoogleInbox(r) => match r.google_inbox {
                GoogleInboxVersion::CountsV1 => Operation::GoogleInboxRead,
                GoogleInboxVersion::PreviewV1 => Operation::GoogleInboxPreview,
            },
        }
    }
    pub fn repository(&self) -> Option<&GitHubRepository> {
        match self {
            Self::GitHub(r) => Some(r),
            Self::GoogleInbox(_) => None,
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Self::GitHub(r) => r.bytes(),
            Self::GoogleInbox(r) => match r.google_inbox {
                GoogleInboxVersion::CountsV1 => b"moosuri/google-inbox-counts/v1".to_vec(),
                GoogleInboxVersion::PreviewV1 => b"moosuri/google-inbox-preview/v1".to_vec(),
            },
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<GitHubRepository>,
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
/// Only bounded untrusted mail display fields; no IDs, account, URLs or MIME.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleMessagePreview {
    pub subject: String,
    pub from: String,
    pub date: String,
    pub snippet: String,
    pub truncated: bool,
}
impl GoogleMessagePreview {
    pub fn valid(&self) -> bool {
        [
            (&self.subject, 256),
            (&self.from, 256),
            (&self.date, 128),
            (&self.snippet, 512),
        ]
        .iter()
        .all(|(s, max)| s.len() <= *max && !s.chars().any(char::is_control))
    }
}
/// Projection is reconstructed from a typed allowlist. Preview text is untrusted data.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Projection {
    None,
    FixedProbeOk,
    GoogleInbox {
        messages_total: u32,
        messages_unread: u32,
        threads_total: u32,
        threads_unread: u32,
    },
    GoogleInboxPreview {
        messages: Vec<GoogleMessagePreview>,
        unavailable: u8,
        more_available: bool,
    },
    GitHubRepository {
        id: u64,
        private: bool,
        archived: bool,
        stars: u64,
        open_issues: u64,
    },
}
impl Projection {
    pub fn bytes(&self) -> Vec<u8> {
        let mut b = Bytes::new(b"projection");
        match self {
            Self::GoogleInboxPreview {
                messages,
                unavailable,
                more_available,
            } => {
                b.0.push(4);
                b.u(messages.len() as u64);
                for m in messages {
                    for s in [&m.subject, &m.from, &m.date, &m.snippet] {
                        b.blob(s.as_bytes());
                    }
                    b.0.push(m.truncated as u8);
                }
                b.0.push(*unavailable);
                b.0.push(*more_available as u8);
            }
            Self::None => b.0.push(0),
            Self::FixedProbeOk => b.0.push(1),
            Self::GoogleInbox {
                messages_total,
                messages_unread,
                threads_total,
                threads_unread,
            } => {
                b.0.push(3);
                for value in [
                    messages_total,
                    messages_unread,
                    threads_total,
                    threads_unread,
                ] {
                    b.u(*value as u64);
                }
            }
            Self::GitHubRepository {
                id,
                private,
                archived,
                stars,
                open_issues,
            } => {
                b.0.push(2);
                b.u(*id);
                b.0.push(*private as u8);
                b.0.push(*archived as u8);
                b.u(*stars);
                b.u(*open_issues);
            }
        }
        b.0
    }
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
    Register {
        binding: Binding,
    },
    RegisterGooglePreview {
        binding: Binding,
    },
    RefreshGooglePreview {
        expected: Binding,
        binding: Binding,
    },
    RegisterGoogleInbox {
        binding: Binding,
    },
    RefreshGoogleInbox {
        expected: Binding,
        binding: Binding,
    },
    RegisterRepository {
        binding: Binding,
        repository: GitHubRepository,
    },
    /// Same local scope, exact old→new version CAS; no item identity is uploaded.
    RefreshRepository {
        expected: Binding,
        binding: Binding,
        repository: GitHubRepository,
    },
    /// Fresh live-vault dispatch binds the exact original/current request bytes.
    CommitCurrent {
        invocation: Invocation,
        request_digest: [u8; 32],
    },
    Poll,
    Abandon {
        invocation: Invocation,
    },
    Commit {
        invocation: Invocation,
    },
    Recover {
        invocation: Invocation,
    },
    Settle {
        receipt: Box<SignedReceipt>,
    },
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
    Waiting {
        requests: Vec<Request>,
        revocation_pending: bool,
    },
    Commitment {
        commitment: Box<Commitment>,
    },
    Settled,
    Abandoned,
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
        match r.operation {
            Operation::FixedProbe => self.0.push(1),
            Operation::GoogleInboxRead | Operation::GoogleInboxPreview => {
                self.0.push(if r.operation == Operation::GoogleInboxRead {
                    3
                } else {
                    4
                });
                match &r.repository {
                    None => self.0.push(0),
                    Some(repo) => {
                        self.0.push(1);
                        self.blob(repo.owner.as_bytes());
                        self.blob(repo.repository.as_bytes());
                    }
                }
            }
            Operation::GitHubRepositoryRead => {
                self.0.push(2);
                if let Some(repo) = &r.repository {
                    self.blob(repo.owner.as_bytes());
                    self.blob(repo.repository.as_bytes());
                } else {
                    self.blob(&[]);
                    self.blob(&[]);
                }
            }
        }
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
            Projection::GoogleInboxPreview { .. } => 4,
            Projection::None => 0,
            Projection::FixedProbeOk => 1,
            Projection::GoogleInbox { .. } => 3,
            Projection::GitHubRepository { .. } => 2,
        });
        if let Projection::GitHubRepository {
            id,
            private,
            archived,
            stars,
            open_issues,
        } = &r.projection
        {
            self.u(*id);
            self.0.push(*private as u8);
            self.0.push(*archived as u8);
            self.u(*stars);
            self.u(*open_issues);
        }
        if let Projection::GoogleInbox {
            messages_total,
            messages_unread,
            threads_total,
            threads_unread,
        } = &r.projection
        {
            for value in [
                messages_total,
                messages_unread,
                threads_total,
                threads_unread,
            ] {
                self.u(*value as u64);
            }
        }
        if let Projection::GoogleInboxPreview {
            messages,
            unavailable,
            more_available,
        } = &r.projection
        {
            self.u(messages.len() as u64);
            for m in messages {
                for s in [&m.subject, &m.from, &m.date, &m.snippet] {
                    self.blob(s.as_bytes());
                }
                self.0.push(m.truncated as u8);
            }
            self.0.push(*unavailable);
            self.0.push(*more_available as u8);
        }
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
        Command::RegisterGooglePreview { binding } => {
            body.0.push(12);
            body.binding(binding);
        }
        Command::RefreshGooglePreview { expected, binding } => {
            body.0.push(13);
            body.binding(expected);
            body.binding(binding);
        }
        Command::RegisterGoogleInbox { binding } => {
            body.0.push(10);
            body.binding(binding);
        }
        Command::RefreshGoogleInbox { expected, binding } => {
            body.0.push(11);
            body.binding(expected);
            body.binding(binding);
        }
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
        Command::RegisterRepository {
            binding,
            repository,
        } => {
            body.0.push(5);
            body.binding(binding);
            body.blob(repository.owner.as_bytes());
            body.blob(repository.repository.as_bytes());
        }
        Command::RefreshRepository {
            expected,
            binding,
            repository,
        } => {
            body.0.push(8);
            body.binding(expected);
            body.binding(binding);
            body.blob(repository.owner.as_bytes());
            body.blob(repository.repository.as_bytes());
        }
        Command::CommitCurrent {
            invocation,
            request_digest,
        } => {
            body.0.push(9);
            body.invocation(invocation);
            body.blob(request_digest);
        }
        Command::Poll => body.0.push(6),
        Command::Abandon { invocation } => {
            body.0.push(7);
            body.invocation(invocation)
        }
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
