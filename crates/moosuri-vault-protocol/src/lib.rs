//! Narrow ciphertext/authority protocol. Never accept a master cookie, key or plaintext field.
pub use moosuri_vault_core::{Binding, EncryptedItem, Header};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub account: Uuid,
    pub tenant: Uuid,
    pub installation: Uuid,
    pub epoch: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthorityRequest {
    Exchange {
        handoff: String,
        vault_origin: String,
    },
    Validate {
        grant: String,
    },
    Commit {
        grant: String,
        operation: Uuid,
        digest: String,
    },
    Close {
        grant: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityReply {
    pub owner: Owner,
    pub grant: Option<String>,
    pub operation: Option<Uuid>,
    pub digest: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handoff {
    pub token: String,
    pub vault_origin: String,
    pub expires_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Key {
        id: Uuid,
        expected_header: Option<String>,
        expected_inventory: String,
        header: Header,
        items: Vec<EncryptedItem>,
        commitment: String,
    },
    Put {
        id: Uuid,
        expected_header: String,
        expected_revision: u64,
        item: EncryptedItem,
    },
    Delete {
        id: Uuid,
        expected_header: String,
        item: Uuid,
        expected_revision: u64,
    },
}
impl Operation {
    pub fn id(&self) -> Uuid {
        match self {
            Self::Key { id, .. } | Self::Put { id, .. } | Self::Delete { id, .. } => *id,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub operation: Uuid,
    pub payload_sha256: String,
    pub state: String,
    pub header_sha256: String,
    pub inventory_sha256: String,
    pub commitment: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationRecord {
    pub operation: Operation,
    pub receipt: Receipt,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultState {
    pub owner: Owner,
    pub binding: Option<Binding>,
    pub header: Option<Header>,
    pub header_sha256: String,
    pub items: Vec<EncryptedItem>,
    pub inventory_sha256: String,
    pub pending: Vec<Receipt>,
}

/// Authenticated ciphertext-vault key lifetime commitment; no key material.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VaultHead {
    pub header_sha256: String,
    pub revision: u64,
    pub envelope_epoch: u64,
    pub generation: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentSession {
    pub owner: Owner,
    pub head: VaultHead,
}
