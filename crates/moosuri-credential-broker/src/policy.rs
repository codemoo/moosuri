//! Device-owned immutable grant. Server metadata cannot create a local selection.
use crate::{Error, Result, auth::hash};
use moosuri_vault_protocol::broker::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalGrant {
    pub device: Device,
    pub binding: Binding,
    // Opaque local item reference; never uploaded to the assistant.
    item: Uuid,
    service: String,
    adapter_version: u32,
    origin: String,
    resource: Uuid,
    revoked: bool,
}
impl LocalGrant {
    /// Actual local connection/selection step. Foundation supports only a fixed,
    /// non-network synthetic operation; no model/server-facing grant constructor.
    pub fn select_fixed_probe(
        device: Device,
        item: Uuid,
        resource: Uuid,
        item_epoch: u64,
        key_epoch: u64,
        expires_at: i64,
    ) -> Result<Self> {
        if item.is_nil()
            || resource.is_nil()
            || item_epoch == 0
            || key_epoch == 0
            || device.generation == 0
        {
            return Err(Error::Policy);
        }
        let mut g = Self {
            device,
            binding: Binding {
                id: Uuid::new_v4(),
                epoch: 1,
                grant_version: 1,
                grant_hash: [0; 32],
                item_epoch,
                key_epoch,
                expires_at,
            },
            item,
            service: "foundation-fixed-probe".into(),
            adapter_version: 1,
            origin: "https://fixed-probe.invalid".into(),
            resource,
            revoked: false,
        };
        // Fixed-order local commitment includes the otherwise hidden grant ceiling.
        let mut data = Vec::from(b"moosuri/local-fixed-probe/v1".as_slice());
        data.extend_from_slice(g.item.as_bytes());
        data.extend_from_slice(g.resource.as_bytes());
        data.extend_from_slice(&request_bytes(&Request {
            device: g.device.clone(),
            invocation: Invocation {
                run: Uuid::nil(),
                step: 0,
                call: Uuid::nil(),
                nonce: Uuid::nil(),
            },
            binding: g.binding.clone(),
            operation: Operation::FixedProbe,
            digest: [0; 32],
            expires_at,
        }));
        g.binding.grant_hash = hash(&data);
        Ok(g)
    }
    pub fn revoke(&mut self) {
        self.revoked = true;
    }
    pub fn check(
        &self,
        request: &Request,
        now: i64,
        item_epoch: u64,
        key_epoch: u64,
    ) -> Result<()> {
        if self.revoked
            || request.device != self.device
            || request.binding != self.binding
            || request.expires_at <= now
            || request.expires_at > self.binding.expires_at
            || self.binding.expires_at <= now
            || item_epoch != self.binding.item_epoch
            || key_epoch != self.binding.key_epoch
            || self.service != "foundation-fixed-probe"
            || self.adapter_version != 1
            || self.origin != "https://fixed-probe.invalid"
        {
            return Err(Error::Policy);
        }
        // Digest is derived locally from a typed fixed resource, not a URL supplied by server.
        if request.digest != self.probe_digest() {
            return Err(Error::Policy);
        }
        Ok(())
    }
    pub fn probe_digest(&self) -> [u8; 32] {
        let mut b = Vec::from(b"moosuri/fixed-probe/v1".as_slice());
        b.extend_from_slice(self.resource.as_bytes());
        hash(&b)
    }
}
