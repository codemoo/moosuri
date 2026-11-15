//! Versioned credential-free broker messages. Preview text is private untrusted data.
//! No credentials, raw provider bodies or arbitrary request URLs.
use crate::Owner;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
mod model_mail;
pub use model_mail::*;

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
    GiteaRepositoryRead { site: Uuid },
    GitHubRepositoryRead,
    GoogleInboxRead,
    GoogleInboxPreview,
    GoogleInboxBody { selection: Uuid },
    GoogleMailSend { effect: Uuid, execution: [u8; 32] },
    GoogleMailLookup { effect: Uuid, execution: [u8; 32] },
    GoogleMailModelRead { selection: Uuid },
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
    Gitea(GiteaRepository),
    GoogleInbox(GoogleInboxResource),
    GoogleBody(GoogleBodyResource),
    GoogleModelRead(GoogleModelReadResource),
}
/// Opaque device-local site selection; the origin and account stay on the device.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GiteaRepository {
    pub gitea_site: Uuid,
    pub repository: GitHubRepository,
}
impl GiteaRepository {
    pub fn valid(&self) -> bool {
        !self.gitea_site.is_nil() && self.repository.valid()
    }
    pub fn bytes(&self) -> Vec<u8> {
        let mut b = Bytes::new(b"gitea-browser-issues/v1");
        b.id(self.gitea_site);
        b.blob(&self.repository.bytes());
        b.0
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleModelReadResource {
    pub google_model_read: Uuid,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleBodyResource {
    pub google_body: Uuid,
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
    pub fn google_model_read(selection: Uuid) -> Self {
        Self::GoogleModelRead(GoogleModelReadResource {
            google_model_read: selection,
        })
    }
    pub fn google_body(selection: Uuid) -> Self {
        Self::GoogleBody(GoogleBodyResource {
            google_body: selection,
        })
    }
    pub fn scope(&self) -> Self {
        match self {
            Self::GoogleBody(_) => Self::google_body(Uuid::nil()),
            Self::GoogleModelRead(_) => Self::google_model_read(Uuid::nil()),
            _ => self.clone(),
        }
    }
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
            Self::Gitea(r) => r.valid(),
            Self::GoogleInbox(_) => true,
            Self::GoogleBody(r) => !r.google_body.is_nil(),
            Self::GoogleModelRead(r) => !r.google_model_read.is_nil(),
        }
    }
    pub fn operation(&self) -> Operation {
        match self {
            Self::GitHub(_) => Operation::GitHubRepositoryRead,
            Self::Gitea(r) => Operation::GiteaRepositoryRead { site: r.gitea_site },
            Self::GoogleModelRead(r) => Operation::GoogleMailModelRead {
                selection: r.google_model_read,
            },
            Self::GoogleBody(r) => Operation::GoogleInboxBody {
                selection: r.google_body,
            },
            Self::GoogleInbox(r) => match r.google_inbox {
                GoogleInboxVersion::CountsV1 => Operation::GoogleInboxRead,
                GoogleInboxVersion::PreviewV1 => Operation::GoogleInboxPreview,
            },
        }
    }
    pub fn repository(&self) -> Option<&GitHubRepository> {
        match self {
            Self::GitHub(r) => Some(r),
            Self::Gitea(r) => Some(&r.repository),
            Self::GoogleInbox(_) | Self::GoogleBody(_) | Self::GoogleModelRead(_) => None,
        }
    }
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Self::GitHub(r) => r.bytes(),
            Self::Gitea(r) => r.bytes(),
            Self::GoogleModelRead(_) => b"moosuri/google-mail-model-read/v1".to_vec(),
            Self::GoogleBody(_) => b"moosuri/google-inbox-body/v1".to_vec(),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection: Option<Uuid>,
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
    GiteaRepository {
        open_issues: u64,
    },
    GoogleMailModelRead {
        result: Box<MailModelResult>,
    },
    GoogleMailSent {
        message_id: String,
        thread_id: String,
    },
    GoogleMailObserved {
        status: GoogleSentStatus,
        candidates: u8,
        message_id: Option<String>,
        thread_id: Option<String>,
    },
    GoogleInboxBody {
        text: String,
        truncated: bool,
        unavailable: bool,
    },
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
            Self::GiteaRepository { open_issues } => {
                b.0.push(10);
                b.u(*open_issues);
            }
            Self::GoogleMailModelRead { result } => {
                b.0.push(9);
                b.blob(&result.bytes());
            }
            Self::GoogleMailSent {
                message_id,
                thread_id,
            } => {
                b.0.push(7);
                b.blob(message_id.as_bytes());
                b.blob(thread_id.as_bytes());
            }
            Self::GoogleMailObserved {
                status,
                candidates,
                message_id,
                thread_id,
            } => {
                b.0.push(8);
                b.0.push(status.code());
                b.0.push(*candidates);
                b.blob(message_id.as_deref().unwrap_or("").as_bytes());
                b.blob(thread_id.as_deref().unwrap_or("").as_bytes());
            }
            Self::GoogleInboxBody {
                text,
                truncated,
                unavailable,
            } => {
                b.0.push(5);
                b.blob(text.as_bytes());
                b.0.push(*truncated as u8);
                b.0.push(*unavailable as u8);
            }
            Self::GoogleInboxPreview {
                messages,
                unavailable,
                more_available,
            } => {
                let selected = messages.iter().any(|m| m.selection.is_some());
                b.0.push(if selected { 6 } else { 4 });
                b.u(messages.len() as u64);
                for m in messages {
                    for s in [&m.subject, &m.from, &m.date, &m.snippet] {
                        b.blob(s.as_bytes());
                    }
                    b.0.push(m.truncated as u8);
                    if selected {
                        b.0.push(m.selection.is_some() as u8);
                        if let Some(id) = m.selection {
                            b.id(id);
                        }
                    }
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
    RegisterGitea {
        binding: Binding,
        resource: GiteaRepository,
    },
    RegisterGoogleModelRead {
        binding: Binding,
        background: bool,
    },
    RefreshGoogleModelRead {
        expected: Binding,
        binding: Binding,
        background: bool,
    },
    ModelMailPayload {
        invocation: Invocation,
        request_digest: [u8; 32],
    },
    RegisterGoogleSend {
        binding: Binding,
        email: String,
    },
    RefreshGoogleSend {
        expected: Binding,
        binding: Binding,
        email: String,
    },
    PollMail,
    CheckMail {
        invocation: Invocation,
        request_digest: [u8; 32],
    },
    MailPayload {
        invocation: Invocation,
        request_digest: [u8; 32],
    },
    RegisterGoogleBody {
        binding: Binding,
    },
    RefreshGoogleBody {
        expected: Binding,
        binding: Binding,
    },
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
    ModelMailPayload {
        payload: Box<MailModelRequest>,
    },
    MailCurrent,
    MailPayload {
        payload: Box<GoogleMailPayload>,
    },
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
            Operation::GiteaRepositoryRead { site } => {
                self.0.push(9);
                self.id(site);
                if let Some(repo) = &r.repository {
                    self.blob(&repo.bytes());
                } else {
                    self.blob(&[]);
                }
            }
            Operation::GoogleMailModelRead { selection } => {
                self.0.push(8);
                self.id(selection);
                self.0.push(r.repository.is_some() as u8);
                if let Some(repo) = &r.repository {
                    self.blob(repo.owner.as_bytes());
                    self.blob(repo.repository.as_bytes());
                }
            }
            Operation::GoogleMailSend { effect, execution }
            | Operation::GoogleMailLookup { effect, execution } => {
                self.0
                    .push(if matches!(r.operation, Operation::GoogleMailSend { .. }) {
                        6
                    } else {
                        7
                    });
                self.id(effect);
                self.0.extend_from_slice(&execution);
                self.0.push(r.repository.is_some() as u8);
                if let Some(repo) = &r.repository {
                    self.blob(repo.owner.as_bytes());
                    self.blob(repo.repository.as_bytes());
                }
            }
            Operation::GoogleInboxBody { selection } => {
                self.0.push(5);
                self.id(selection);
                self.0.push(r.repository.is_some() as u8);
                if let Some(repo) = &r.repository {
                    self.blob(repo.owner.as_bytes());
                    self.blob(repo.repository.as_bytes());
                }
            }
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
            Projection::GiteaRepository { .. } => 10,
            Projection::GoogleMailModelRead { .. } => 9,
            Projection::GoogleMailSent { .. } => 7,
            Projection::GoogleMailObserved { .. } => 8,
            Projection::GoogleInboxBody { .. } => 5,
            Projection::GoogleInboxPreview { ref messages, .. } => {
                if messages.iter().any(|m| m.selection.is_some()) {
                    6
                } else {
                    4
                }
            }
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
        match &r.projection {
            Projection::GoogleMailSent {
                message_id,
                thread_id,
            } => {
                self.blob(message_id.as_bytes());
                self.blob(thread_id.as_bytes());
            }
            Projection::GoogleMailObserved {
                status,
                candidates,
                message_id,
                thread_id,
            } => {
                self.0.push(status.code());
                self.0.push(*candidates);
                self.blob(message_id.as_deref().unwrap_or("").as_bytes());
                self.blob(thread_id.as_deref().unwrap_or("").as_bytes());
            }
            _ => {}
        }
        if let Projection::GoogleInboxBody {
            text,
            truncated,
            unavailable,
        } = &r.projection
        {
            self.blob(text.as_bytes());
            self.0.push(*truncated as u8);
            self.0.push(*unavailable as u8);
        }
        if let Projection::GoogleInboxPreview {
            messages,
            unavailable,
            more_available,
        } = &r.projection
        {
            let selected = messages.iter().any(|m| m.selection.is_some());
            self.u(messages.len() as u64);
            for m in messages {
                for s in [&m.subject, &m.from, &m.date, &m.snippet] {
                    self.blob(s.as_bytes());
                }
                self.0.push(m.truncated as u8);
                if selected {
                    self.0.push(m.selection.is_some() as u8);
                    if let Some(id) = m.selection {
                        self.id(id);
                    }
                }
            }
            self.0.push(*unavailable);
            self.0.push(*more_available as u8);
        }
        if let Projection::GiteaRepository { open_issues } = &r.projection {
            self.u(*open_issues);
        }
        if let Projection::GoogleMailModelRead { result } = &r.projection {
            self.blob(&result.bytes());
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
        Command::RegisterGitea { binding, resource } => {
            body.0.push(24);
            body.binding(binding);
            body.blob(&resource.bytes());
        }
        Command::RegisterGoogleModelRead {
            binding,
            background,
        } => {
            body.0.push(21);
            body.binding(binding);
            body.0.push(*background as u8);
        }
        Command::RefreshGoogleModelRead {
            expected,
            binding,
            background,
        } => {
            body.0.push(22);
            body.binding(expected);
            body.binding(binding);
            body.0.push(*background as u8);
        }
        Command::ModelMailPayload {
            invocation,
            request_digest,
        } => {
            body.0.push(23);
            body.invocation(invocation);
            body.blob(request_digest);
        }
        Command::RegisterGoogleSend { binding, email } => {
            body.0.push(16);
            body.binding(binding);
            body.blob(email.as_bytes());
        }
        Command::RefreshGoogleSend {
            expected,
            binding,
            email,
        } => {
            body.0.push(17);
            body.binding(expected);
            body.binding(binding);
            body.blob(email.as_bytes());
        }
        Command::CheckMail {
            invocation,
            request_digest,
        } => {
            body.0.push(20);
            body.invocation(invocation);
            body.0.extend_from_slice(request_digest);
        }
        Command::PollMail => body.0.push(18),
        Command::MailPayload {
            invocation,
            request_digest,
        } => {
            body.0.push(19);
            body.invocation(invocation);
            body.0.extend_from_slice(request_digest);
        }
        Command::RegisterGoogleBody { binding } => {
            body.0.push(14);
            body.binding(binding);
        }
        Command::RefreshGoogleBody { expected, binding } => {
            body.0.push(15);
            body.binding(expected);
            body.binding(binding);
        }
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

/// An approved action's exact immutable execution envelope. `raw` is optional
/// base64url private payload, excluded from the digest because mime_digest binds it.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleMailPayload {
    pub owner: crate::Owner,
    pub action: Uuid,
    pub revision: u64,
    pub connection: Uuid,
    pub connection_epoch: u64,
    pub auth_epoch: i64,
    pub mime_digest: [u8; 32],
    pub message_id: String,
    pub not_before: i64,
    pub timezone: Option<String>,
    pub expires_at: i64,
    pub fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
}
impl GoogleMailPayload {
    pub fn execution_bytes(&self) -> Vec<u8> {
        let mut b = Bytes::new(b"approved-google-mail/v1");
        b.id(self.owner.tenant);
        b.id(self.owner.account);
        b.id(self.owner.installation);
        b.0.extend_from_slice(&self.owner.epoch.to_be_bytes());
        b.id(self.action);
        b.u(self.revision);
        b.id(self.connection);
        b.u(self.connection_epoch);
        b.0.extend_from_slice(&self.auth_epoch.to_be_bytes());
        b.0.extend_from_slice(&self.mime_digest);
        b.blob(self.message_id.as_bytes());
        b.0.extend_from_slice(&self.not_before.to_be_bytes());
        b.blob(self.timezone.as_deref().unwrap_or("").as_bytes());
        b.0.extend_from_slice(&self.expires_at.to_be_bytes());
        b.blob(self.fingerprint.as_bytes());
        b.0
    }
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoogleSentStatus {
    Matching,
    NoCandidates,
    MultipleCandidates,
    IncompleteSearch,
    CandidateMismatch,
    UnsupportedCandidate,
    LegacyEvidenceUnavailable,
    LookupUnavailable,
}
impl GoogleSentStatus {
    pub fn code(self) -> u8 {
        match self {
            Self::Matching => 0,
            Self::NoCandidates => 1,
            Self::MultipleCandidates => 2,
            Self::IncompleteSearch => 3,
            Self::CandidateMismatch => 4,
            Self::UnsupportedCandidate => 5,
            Self::LegacyEvidenceUnavailable => 6,
            Self::LookupUnavailable => 7,
        }
    }
}
pub fn valid_google_mail_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
