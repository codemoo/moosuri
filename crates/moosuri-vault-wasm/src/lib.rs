//! Rust crypto lives only in the dedicated trusted browser Worker.
#![forbid(unsafe_code)]
use moosuri_vault_core::*;
use moosuri_vault_protocol::{Operation, Owner, Receipt, VaultHead, VaultState};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use wasm_bindgen::prelude::*;
use zeroize::Zeroizing;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "/src/entropy.js")]
extern "C" {
    #[wasm_bindgen(catch,js_name=fillRandom)]
    fn browser_fill(output: &mut [u8]) -> std::result::Result<(), JsValue>;
}
struct BrowserEntropy;
impl TrustedEntropy for BrowserEntropy {
    fn fill(&mut self, out: &mut [u8]) -> Result<()> {
        #[cfg(target_arch = "wasm32")]
        {
            browser_fill(out).map_err(|_| Error::Entropy)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = out;
            Err(Error::Entropy)
        }
    }
}
fn id() -> Result<Uuid> {
    let mut b = [0; 16];
    BrowserEntropy.fill(&mut b)?;
    b[6] = (b[6] & 15) | 64;
    b[8] = (b[8] & 63) | 128;
    Ok(Uuid::from_bytes(b))
}
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|v| format!("{v:02x}")).collect()
}
fn unhex(s: &str) -> Result<[u8; 32]> {
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid);
    }
    let mut b = [0; 32];
    for (i, v) in b.iter_mut().enumerate() {
        *v = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).map_err(|_| Error::Invalid)?;
    }
    Ok(b)
}

struct CsvText(Zeroizing<String>);
impl<'de> Deserialize<'de> for CsvText {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        if value.len() > 10 * 1024 * 1024 {
            return Err(serde::de::Error::custom("vault_limit"));
        }
        Ok(Self(value))
    }
}
fn keep(value: Option<SecretText>, prior: Option<&SecretText>) -> Result<SecretText> {
    match value {
        Some(v) => Ok(v),
        None => SecretText::new(prior.map_or("", |p| p.expose()).into()),
    }
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Load {
        state: VaultState,
    },
    Setup {
        password: SecretText,
    },
    VerifyRecovery {
        recovery: SecretText,
    },
    Unlock {
        secret: SecretText,
        recovery: bool,
    },
    List {
        query: SecretText,
    },
    EditFields {
        item: Uuid,
    },
    Reveal {
        item: Uuid,
        field: Field,
    },
    Put {
        item: Option<Uuid>,
        fields: Draft,
    },
    Delete {
        item: Uuid,
    },
    GeneratePassword {
        length: u8,
    },
    Totp {
        item: Uuid,
        now: u64,
    },
    PreparePassword {
        password: SecretText,
    },
    PrepareRecovery,
    PrepareRotation {
        password: SecretText,
    },
    Pending,
    CheckPendingHead {
        receipt: Receipt,
        head: VaultHead,
    },
    Acknowledge {
        receipt: Receipt,
        state: VaultState,
    },
    Abort {
        receipt: Option<Receipt>,
    },
    ArchiveExport,
    ArchivePreview {
        archive: String,
        secret: SecretText,
        recovery: bool,
    },
    ArchiveSelect {
        rows: Vec<u32>,
    },
    CsvPreview {
        csv: CsvText,
        username: Option<SecretText>,
    },
    CsvSelect {
        rows: Vec<u32>,
    },
    CsvNext,
    CsvStatus,
    CsvCancel,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Field {
    Secret,
    Username,
    Url,
    Notes,
    Totp,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Draft {
    kind: ItemKind,
    title: SecretText,
    username: SecretText,
    url: SecretText,
    notes: SecretText,
    favorite: bool,
    expires_at: SecretText,
    secret: Option<SecretText>,
    totp: Option<SecretText>,
}

#[wasm_bindgen]
pub struct VaultEngine {
    owner: Owner,
    state: Option<VaultState>,
    session: Option<ClientVault>,
    setup: Option<PendingVault>,
    pending: Option<Operation>,
    recovery_verified: bool,
    import: Option<AppleImport>,
    archive_import: Option<ArchiveImport>,
    batch: Vec<EncryptedItem>,
    batch_index: usize,
}
#[wasm_bindgen]
impl VaultEngine {
    #[wasm_bindgen(constructor)]
    pub fn new(owner: &str) -> std::result::Result<VaultEngine, JsValue> {
        if owner.len() > 1024 {
            return Err(JsValue::from_str("vault_limit"));
        }
        let owner: Owner =
            serde_json::from_str(owner).map_err(|_| JsValue::from_str("vault_invalid"))?;
        if owner.account.is_nil()
            || owner.tenant.is_nil()
            || owner.installation.is_nil()
            || owner.epoch < 0
        {
            return Err(JsValue::from_str("vault_invalid"));
        }
        Ok(Self {
            owner,
            state: None,
            session: None,
            setup: None,
            pending: None,
            recovery_verified: false,
            import: None,
            archive_import: None,
            batch: vec![],
            batch_index: 0,
        })
    }
    /// Bounded fixed-error RPC. No general decrypt command or model tool exists.
    pub fn rpc(&mut self, input: String) -> String {
        let input = Zeroizing::new(input);
        let result = if input.len() > 32 * 1024 * 1024 {
            Err(Error::Limit)
        } else {
            serde_json::from_str::<Request>(&input)
                .map_err(|_| Error::Invalid)
                .and_then(|r| self.dispatch(r))
        };
        match result {
            Ok(value) => json!({"ok":true,"value":value}).to_string(),
            Err(error) => json!({"ok":false,"error":error.to_string()}).to_string(),
        }
    }
}
impl VaultEngine {
    fn current(&self) -> Result<&VaultState> {
        self.state.as_ref().ok_or(Error::State)
    }
    fn active(&mut self) -> Result<&mut ActiveVault> {
        self.session.as_mut().ok_or(Error::State)?.active()
    }
    fn mutable(&self) -> Result<()> {
        if self.pending.is_some() || self.setup.is_some() || !self.current()?.pending.is_empty() {
            return Err(Error::State);
        }
        Ok(())
    }
    fn no_import(&self) -> Result<()> {
        if self.import.is_some() || self.archive_import.is_some() || !self.batch.is_empty() {
            return Err(Error::State);
        }
        Ok(())
    }
    fn item(&self, id: Uuid) -> Result<EncryptedItem> {
        self.current()?
            .items
            .iter()
            .find(|v| v.id == id)
            .cloned()
            .ok_or(Error::Invalid)
    }
    fn validate_state(&self, state: &VaultState) -> Result<()> {
        if state.owner != self.owner || state.items.len() > 256 || state.pending.len() > 32 {
            return Err(Error::Authentication);
        }
        if hex(&inventory_digest(&state.items)?) != state.inventory_sha256 {
            return Err(Error::Conflict);
        }
        match (&state.header, state.binding) {
            (Some(h), Some(b)) => {
                if b.owner_id != self.owner.account
                    || b.tenant_id != self.owner.tenant
                    || h.binding != b
                    || hash(&h.to_json()?) != state.header_sha256
                {
                    return Err(Error::Authentication);
                }
                for item in &state.items {
                    item.to_json()?;
                    if item.generation != h.generation {
                        return Err(Error::Conflict);
                    }
                }
            }
            (None, None) if state.header_sha256.is_empty() && state.items.is_empty() => {}
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
    fn key_operation(&self) -> Result<Operation> {
        let state = self.current()?;
        let (header, items, (id, digest)) = if let Some(p) = &self.setup {
            (p.header().clone(), vec![], p.candidate()?)
        } else {
            let p = self.session.as_ref().ok_or(Error::State)?;
            (
                p.pending_header()?.clone(),
                p.pending_items()?.to_vec(),
                p.pending_candidate()?,
            )
        };
        Ok(Operation::Key {
            id,
            expected_header: state.header.as_ref().map(|_| state.header_sha256.clone()),
            expected_inventory: state.inventory_sha256.clone(),
            header,
            items,
            commitment: hex(&digest),
        })
    }
    fn begin_key(&mut self, verified: bool) -> Result<Value> {
        self.pending = Some(self.key_operation()?);
        self.recovery_verified = verified;
        let recovery = if let Some(p) = &self.setup {
            Some(p.recovery().expose_encoded())
        } else {
            self.session
                .as_ref()
                .ok_or(Error::State)?
                .pending_recovery()?
                .map(|r| r.expose_encoded())
        };
        Ok(json!({"recovery":recovery.as_ref().map(|v|v.expose()),"needs_verification":!verified}))
    }
    fn dispatch(&mut self, request: Request) -> Result<Value> {
        match request {
            Request::Load { state } => {
                self.validate_state(&state)?;
                if self.pending.is_some() {
                    return Err(Error::State);
                }
                if self.session.is_some() && self.current()?.header_sha256 != state.header_sha256 {
                    return Err(Error::Conflict);
                }
                let initialized = state.header.is_some();
                self.state = Some(state);
                Ok(json!({"initialized":initialized}))
            }
            Request::Setup { password } => {
                self.mutable()?;
                if password.expose().len() < 12 {
                    return Err(Error::Invalid);
                }
                if self.current()?.header.is_some() || self.session.is_some() {
                    return Err(Error::State);
                }
                let binding = Binding {
                    owner_id: self.owner.account,
                    tenant_id: self.owner.tenant,
                    vault_id: id()?,
                };
                self.setup = Some(PendingVault::prepare(
                    binding,
                    &password,
                    &mut BrowserEntropy,
                )?);
                self.begin_key(false)
            }
            Request::VerifyRecovery { recovery } => {
                let value = RecoveryKey::parse(&recovery)?;
                if let Some(p) = &mut self.setup {
                    p.verify_recovery(&value)?;
                } else {
                    self.session
                        .as_mut()
                        .ok_or(Error::State)?
                        .verify_recovery(&value)?;
                }
                self.recovery_verified = true;
                Ok(json!(true))
            }
            Request::Unlock { secret, recovery } => {
                if self.pending.is_some() || self.setup.is_some() || self.session.is_some() {
                    return Err(Error::State);
                }
                let state = self.current()?;
                let h = state.header.as_ref().ok_or(Error::State)?.clone();
                let b = state.binding.ok_or(Error::State)?;
                let admitted = CommittedHeader::admit(
                    h,
                    b,
                    HeaderReceipt::from_verified_current_storage(b, unhex(&state.header_sha256)?),
                )?;
                let active = if recovery {
                    ActiveVault::unlock_recovery(admitted, b, &RecoveryKey::parse(&secret)?)?
                } else {
                    ActiveVault::unlock_password(admitted, b, &secret)?
                };
                self.session = Some(ClientVault::new(active));
                Ok(json!(true))
            }
            Request::List { query } => {
                let items = self.current()?.items.clone();
                let q = Zeroizing::new(query.expose().to_lowercase());
                let mut result = vec![];
                for item in items {
                    let f = self.active()?.open_item(&item)?;
                    let found = q.is_empty()
                        || [&f.title, &f.username, &f.url, &f.notes].iter().any(|v| {
                            Zeroizing::new(v.expose().to_lowercase()).contains(q.as_str())
                        });
                    if found {
                        result.push(json!({"id":item.id,"revision":item.revision,"title":f.title.expose(),"kind":f.kind,"favorite":f.favorite,"expires_at":f.expires_at.expose(),"has_totp":!f.totp.expose().is_empty()}));
                    }
                }
                Ok(json!(result))
            }
            Request::EditFields { item } => {
                let i = self.item(item)?;
                let f = self.active()?.open_item(&i)?;
                Ok(
                    json!({"kind":f.kind,"title":f.title.expose(),"username":f.username.expose(),"url":f.url.expose(),"notes":f.notes.expose(),"favorite":f.favorite,"expires_at":f.expires_at.expose()}),
                )
            }
            Request::Reveal { item, field } => {
                let i = self.item(item)?;
                let f = self.active()?.open_item(&i)?;
                Ok(json!(match field {
                    Field::Secret => f.secret.expose(),
                    Field::Username => f.username.expose(),
                    Field::Url => f.url.expose(),
                    Field::Notes => f.notes.expose(),
                    Field::Totp => return Err(Error::Unsupported),
                }))
            }
            Request::Put { item, fields } => {
                self.mutable()?;
                self.no_import()?;
                let old = item.map(|i| self.item(i)).transpose()?;
                let prior = old
                    .as_ref()
                    .map(|i| self.active()?.open_item(i))
                    .transpose()?;
                let f = ItemFields {
                    kind: fields.kind,
                    title: fields.title,
                    username: fields.username,
                    url: fields.url,
                    notes: fields.notes,
                    favorite: fields.favorite,
                    expires_at: fields.expires_at,
                    secret: keep(fields.secret, prior.as_ref().map(|p| &p.secret))?,
                    totp: keep(fields.totp, prior.as_ref().map(|p| &p.totp))?,
                };
                let item_id = match old.as_ref() {
                    Some(i) => i.id,
                    None => id()?,
                };
                let revision = old.as_ref().map_or(0, |i| i.revision);
                let encrypted = self.active()?.seal_item(
                    item_id,
                    revision.checked_add(1).ok_or(Error::Limit)?,
                    &f,
                    &mut BrowserEntropy,
                )?;
                self.pending = Some(Operation::Put {
                    id: id()?,
                    expected_header: self.current()?.header_sha256.clone(),
                    expected_revision: revision,
                    item: encrypted,
                });
                self.recovery_verified = true;
                Ok(json!(true))
            }
            Request::Delete { item } => {
                self.mutable()?;
                self.no_import()?;
                let i = self.item(item)?;
                self.active()?;
                self.pending = Some(Operation::Delete {
                    id: id()?,
                    expected_header: self.current()?.header_sha256.clone(),
                    item,
                    expected_revision: i.revision,
                });
                self.recovery_verified = true;
                Ok(json!(true))
            }
            Request::GeneratePassword { length } => {
                if !(16..=64).contains(&length) {
                    return Err(Error::Limit);
                }
                self.active()?;
                let alphabet =
                    b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789!@#$%^&*()-_=+";
                let maximum = 256 - (256 % alphabet.len());
                let mut result = Zeroizing::new(String::new());
                while result.len() < usize::from(length) {
                    let mut b = [0; 32];
                    BrowserEntropy.fill(&mut b)?;
                    for n in b {
                        if usize::from(n) < maximum {
                            result.push(char::from(alphabet[usize::from(n) % alphabet.len()]));
                            if result.len() == usize::from(length) {
                                break;
                            }
                        }
                    }
                }
                Ok(json!(result.as_str()))
            }
            Request::Totp { item, now } => {
                let i = self.item(item)?;
                let f = self.active()?.open_item(&i)?;
                let otp = if f.totp.expose().starts_with("otpauth:") {
                    Totp::parse_uri(&f.totp)?
                } else {
                    Totp::from_base32(&f.totp, TotpAlgorithm::Sha1, 6, 30)?
                };
                let c = otp.code_at(now)?;
                Ok(json!({"code":c.expose(),"valid_from":c.valid_from,"valid_until":c.valid_until}))
            }
            Request::PreparePassword { password } => {
                self.mutable()?;
                self.no_import()?;
                if password.expose().len() < 12 {
                    return Err(Error::Invalid);
                }
                self.session
                    .as_mut()
                    .ok_or(Error::State)?
                    .prepare_password(&password, &mut BrowserEntropy)?;
                self.begin_key(true)
            }
            Request::PrepareRecovery => {
                self.mutable()?;
                self.no_import()?;
                self.session
                    .as_mut()
                    .ok_or(Error::State)?
                    .prepare_recovery(&mut BrowserEntropy)?;
                self.begin_key(false)
            }
            Request::PrepareRotation { password } => {
                self.mutable()?;
                self.no_import()?;
                if password.expose().len() < 12 {
                    return Err(Error::Invalid);
                }
                let items = self.current()?.items.clone();
                let inventory = unhex(&self.current()?.inventory_sha256)?;
                self.session
                    .as_mut()
                    .ok_or(Error::State)?
                    .prepare_rotation(&password, &items, inventory, &mut BrowserEntropy)?;
                self.begin_key(false)
            }
            Request::Pending => {
                if !self.recovery_verified {
                    return Err(Error::State);
                }
                Ok(
                    serde_json::to_value(self.pending.as_ref().ok_or(Error::State)?)
                        .map_err(|_| Error::Invalid)?,
                )
            }
            Request::CheckPendingHead { receipt, head } => {
                self.check_pending_head(&receipt, &head)?;
                Ok(json!(true))
            }
            Request::Acknowledge { receipt, state } => self.acknowledge(receipt, state),
            Request::Abort { receipt } => {
                let op = self.pending.as_ref().ok_or(Error::State)?;
                if let Some(r) = receipt
                    && (r.operation != op.id()
                        || r.payload_sha256
                            != hash(&serde_json::to_vec(op).map_err(|_| Error::Invalid)?)
                        || !matches!(r.state.as_str(), "aborted" | "conflict"))
                {
                    return Err(Error::Conflict);
                }
                self.pending = None;
                self.setup = None;
                if let Some(s) = &mut self.session {
                    s.abort_pending();
                }
                self.recovery_verified = false;
                Ok(json!(true))
            }
            Request::ArchiveExport => {
                self.mutable()?;
                self.no_import()?;
                let items = self.current()?.items.clone();
                let inventory = unhex(&self.current()?.inventory_sha256)?;
                let bytes = self
                    .active()?
                    .export_archive(&items, inventory, &Continue)?;
                Ok(json!(String::from_utf8(bytes).map_err(|_| Error::Invalid)?))
            }
            Request::ArchivePreview {
                archive,
                secret,
                recovery,
            } => {
                self.mutable()?;
                self.no_import()?;
                let items = self.current()?.items.clone();
                let import = ArchiveImport::preview(
                    archive.as_bytes(),
                    &secret,
                    recovery,
                    self.active()?,
                    &items,
                    &Continue,
                )?;
                let p = import.counts();
                let result = json!({"eligible":p.eligible,"duplicates":p.duplicates,"filtered":0,"rows":p.rows.iter().map(|r|json!({"row":r.row,"status":match r.status{ImportStatus::New=>"new",ImportStatus::ChangedAccount=>"changed",ImportStatus::Duplicate=>"duplicate",ImportStatus::Filtered=>"filtered"},"blank_password":false,"unsupported_url":false,"invalid_otp":false})).collect::<Vec<_>>()});
                self.archive_import = Some(import);
                self.batch_index = 0;
                Ok(result)
            }
            Request::ArchiveSelect { rows } => {
                self.mutable()?;
                if self.import.is_some() {
                    return Err(Error::State);
                }
                let mut import = self.archive_import.take().ok_or(Error::State)?;
                let result = import
                    .seal_selected(self.active()?, &rows, &mut BrowserEntropy, &Continue)
                    .map(|v| v.to_vec());
                self.archive_import = Some(import);
                self.batch = result?;
                self.batch_index = 0;
                Ok(json!({"selected":self.batch.len()}))
            }
            Request::CsvPreview { csv, username } => {
                self.mutable()?;
                self.no_import()?;
                if csv.0.len() > 10 * 1024 * 1024 {
                    return Err(Error::Limit);
                }
                let items = self.current()?.items.clone();
                let filter = username.map_or(UsernameFilter::All, UsernameFilter::Exact);
                self.import = Some(AppleImport::preview(
                    csv.0.as_bytes(),
                    self.active()?,
                    &items,
                    filter,
                    &Continue,
                )?);
                self.batch.clear();
                self.batch_index = 0;
                let p = self.import.as_ref().ok_or(Error::State)?.counts();
                Ok(
                    json!({"eligible":p.eligible,"duplicates":p.duplicates,"filtered":p.filtered,"rows":p.rows.iter().map(|r|json!({"row":r.row,"status":match r.status{ImportStatus::New=>"new",ImportStatus::ChangedAccount=>"changed",ImportStatus::Duplicate=>"duplicate",ImportStatus::Filtered=>"filtered"},"blank_password":r.blank_password,"unsupported_url":r.unsupported_url,"invalid_otp":r.invalid_otp})).collect::<Vec<_>>()}),
                )
            }
            Request::CsvSelect { rows } => {
                self.mutable()?;
                let mut import = self.import.take().ok_or(Error::State)?;
                let batch = import
                    .seal_selected(self.active()?, &rows, &mut BrowserEntropy, &Continue)?
                    .to_vec();
                self.batch = batch;
                self.import = Some(import);
                self.batch_index = 0;
                Ok(json!({"selected":self.batch.len()}))
            }
            Request::CsvNext => {
                self.mutable()?;
                if self.batch_index >= self.batch.len() {
                    return Ok(Value::Null);
                }
                self.next_import(id()?)
            }
            Request::CsvStatus => {
                Ok(json!({"committed":self.batch_index,"selected":self.batch.len()}))
            }
            Request::CsvCancel => {
                if self.pending.is_some() {
                    return Err(Error::State);
                }
                self.import = None;
                self.archive_import = None;
                self.batch.clear();
                self.batch_index = 0;
                Ok(json!(true))
            }
        }
    }
    fn next_import(&mut self, operation_id: Uuid) -> Result<Value> {
        self.mutable()?;
        let item = self
            .batch
            .get(self.batch_index)
            .ok_or(Error::State)?
            .clone();
        self.pending = Some(Operation::Put {
            id: operation_id,
            expected_header: self.current()?.header_sha256.clone(),
            expected_revision: 0,
            item,
        });
        self.recovery_verified = true;
        Ok(json!({"remaining":self.batch.len()-self.batch_index}))
    }
    // This permits exact own-commit reconciliation only. It never activates a key
    // or admits a server-provided header, and exposes no plaintext/key material.
    fn check_pending_head(&self, receipt: &Receipt, head: &VaultHead) -> Result<()> {
        let op = self.pending.as_ref().ok_or(Error::State)?;
        let Operation::Key {
            header,
            items,
            commitment,
            ..
        } = op
        else {
            return Err(Error::State);
        };
        let expected_items = if self
            .current()?
            .header
            .as_ref()
            .is_some_and(|h| h.generation != header.generation)
        {
            items
        } else {
            &self.current()?.items
        };
        if receipt.state != "committed"
            || receipt.operation != op.id()
            || receipt.payload_sha256 != hash(&serde_json::to_vec(op).map_err(|_| Error::Invalid)?)
            || receipt.commitment.as_deref() != Some(commitment.as_str())
            || receipt.header_sha256 != hash(&header.to_json()?)
            || receipt.inventory_sha256 != hex(&inventory_digest(expected_items)?)
            || head.header_sha256 != receipt.header_sha256
            || head.revision != header.revision
            || head.envelope_epoch != header.envelope_epoch
            || head.generation != header.generation
        {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    fn acknowledge(&mut self, receipt: Receipt, state: VaultState) -> Result<Value> {
        self.validate_state(&state)?;
        let op = self.pending.as_ref().ok_or(Error::State)?;
        if receipt.state != "committed"
            || receipt.operation != op.id()
            || receipt.payload_sha256 != hash(&serde_json::to_vec(op).map_err(|_| Error::Invalid)?)
        {
            return Err(Error::Conflict);
        }
        let mut expected = self.current()?.items.clone();
        let expected_header = match op {
            Operation::Key { header, .. } => hash(&header.to_json()?),
            _ => self.current()?.header_sha256.clone(),
        };
        match op {
            Operation::Key {
                header,
                items,
                commitment,
                ..
            } => {
                if receipt.commitment.as_deref() != Some(commitment.as_str())
                    || hash(&header.to_json()?) != receipt.header_sha256
                {
                    return Err(Error::Conflict);
                }
                if self
                    .current()?
                    .header
                    .as_ref()
                    .is_some_and(|h| h.generation != header.generation)
                {
                    expected = items.clone();
                }
            }
            Operation::Put { item, .. } => {
                expected.retain(|v| v.id != item.id);
                expected.push(item.clone());
            }
            Operation::Delete { item, .. } => expected.retain(|v| v.id != *item),
        }
        expected.sort_by_key(|v| v.id);
        let expected_inventory = hex(&inventory_digest(&expected)?);
        if expected_header != receipt.header_sha256
            || unhex(&receipt.inventory_sha256).is_err()
            || matches!(op, Operation::Key { .. }) && expected_inventory != receipt.inventory_sha256
        {
            return Err(Error::Conflict);
        }
        // A different tab can commit after this exact receipt. Confirm the saved
        // outcome, then require a fresh unlock instead of admitting a retired head.
        if state.header_sha256 != receipt.header_sha256
            || state.inventory_sha256 != receipt.inventory_sha256
        {
            return Ok(json!({"saved":true,"refresh_unlock":true}));
        }
        // The service permits independent same-header item CAS commits. Its exact
        // receipt can include another tab's item beyond our cached inventory.
        // Verify our effect in that exact current state, then retire cached state
        // instead of trapping the committed operation in endless reconciliation.
        match op {
            Operation::Put { item, .. } => {
                let current = state
                    .items
                    .iter()
                    .find(|v| v.id == item.id)
                    .ok_or(Error::Conflict)?;
                if current.to_json()? != item.to_json()? {
                    return Err(Error::Conflict);
                }
            }
            Operation::Delete { item, .. } if state.items.iter().any(|v| v.id == *item) => {
                return Err(Error::Conflict);
            }
            _ => {}
        }
        if expected_inventory != receipt.inventory_sha256 {
            return Ok(json!({"saved":true,"refresh_unlock":true}));
        }
        if let Operation::Key { id, commitment, .. } = op {
            let receipt = StorageReceipt::from_verified_storage(*id, unhex(commitment)?);
            if let Some(p) = self.setup.as_mut() {
                p.confirm_commit(receipt)?;
                let p = self.setup.take().ok_or(Error::State)?;
                self.session = Some(ClientVault::new(p.activate()?));
            } else {
                let p = self.session.as_mut().ok_or(Error::State)?;
                p.confirm_commit(receipt)?;
                p.activate()?;
            }
        }
        if let Operation::Put { item, .. } = op
            && self
                .batch
                .get(self.batch_index)
                .is_some_and(|v| v.id == item.id)
        {
            self.batch_index += 1;
        }
        self.state = Some(state);
        self.pending = None;
        self.recovery_verified = false;
        Ok(json!(true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_epoch_zero_can_initialize_without_entropy() {
        let owner = Owner {
            account: Uuid::from_u128(1),
            tenant: Uuid::from_u128(2),
            installation: Uuid::from_u128(3),
            epoch: 0,
        };
        let mut engine = VaultEngine::new(&serde_json::to_string(&owner).unwrap()).unwrap();
        let inventory_sha256 = hex(&inventory_digest(&[]).unwrap());
        let result: Value = serde_json::from_str(&engine.rpc(json!({"op":"load","state":{"owner":owner,"binding":null,"header":null,"header_sha256":"","items":[],"inventory_sha256":inventory_sha256,"pending":[]}}).to_string())).unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["value"]["initialized"], false);
        let failed: Value = serde_json::from_str(
            &engine
                .rpc(json!({"op":"setup","password":"Synthetic long vault password"}).to_string()),
        )
        .unwrap();
        assert_eq!(failed["error"], "vault_entropy_failed");
    }
    struct Synthetic(u64);
    impl TrustedEntropy for Synthetic {
        fn fill(&mut self, out: &mut [u8]) -> Result<()> {
            for chunk in out.chunks_mut(32) {
                self.0 += 1;
                let block = Sha256::digest(self.0.to_be_bytes());
                chunk.copy_from_slice(&block[..chunk.len()]);
            }
            Ok(())
        }
    }
    fn fixture() -> (VaultEngine, Synthetic) {
        let owner = Owner {
            account: Uuid::from_u128(11),
            tenant: Uuid::from_u128(12),
            installation: Uuid::from_u128(13),
            epoch: 0,
        };
        let binding = Binding {
            owner_id: owner.account,
            tenant_id: owner.tenant,
            vault_id: Uuid::from_u128(14),
        };
        let mut entropy = Synthetic(100);
        let mut pending = PendingVault::prepare(
            binding,
            &SecretText::new("Synthetic fixture password".into()).unwrap(),
            &mut entropy,
        )
        .unwrap();
        let recovery = RecoveryKey::parse(&pending.recovery().expose_encoded()).unwrap();
        pending.verify_recovery(&recovery).unwrap();
        let (id, digest) = pending.candidate().unwrap();
        pending
            .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
            .unwrap();
        let active = pending.activate().unwrap();
        let header = active.header().clone();
        let state = VaultState {
            owner: owner.clone(),
            binding: Some(binding),
            header_sha256: hash(&header.to_json().unwrap()),
            header: Some(header),
            items: vec![],
            inventory_sha256: hex(&inventory_digest(&[]).unwrap()),
            pending: vec![],
        };
        let mut engine = VaultEngine::new(&serde_json::to_string(&owner).unwrap()).unwrap();
        engine.state = Some(state);
        engine.session = Some(ClientVault::new(active));
        (engine, entropy)
    }
    #[test]
    fn own_committed_key_head_exception_never_adopts_or_accepts_foreign_receipt() {
        let (mut e, mut entropy) = fixture();
        let old = e.current().unwrap().header_sha256.clone();
        e.session
            .as_mut()
            .unwrap()
            .prepare_password(
                &SecretText::new("Synthetic replacement password".into()).unwrap(),
                &mut entropy,
            )
            .unwrap();
        e.begin_key(true).unwrap();
        let op = e.pending.as_ref().unwrap();
        let Operation::Key {
            header, commitment, ..
        } = op
        else {
            panic!()
        };
        let header = header.clone();
        let commitment = commitment.clone();
        let head = VaultHead {
            header_sha256: hash(&header.to_json().unwrap()),
            revision: header.revision,
            envelope_epoch: header.envelope_epoch,
            generation: header.generation,
        };
        let receipt = Receipt {
            operation: op.id(),
            payload_sha256: hash(&serde_json::to_vec(op).unwrap()),
            state: "committed".into(),
            header_sha256: head.header_sha256.clone(),
            inventory_sha256: e.current().unwrap().inventory_sha256.clone(),
            commitment: Some(commitment.clone()),
        };
        e.check_pending_head(&receipt, &head).unwrap();
        assert_eq!(e.current().unwrap().header_sha256, old);
        assert!(e.active().is_err()); // Exact polling exception is not activation.
        let mut forged = receipt.clone();
        forged.payload_sha256 = "0".repeat(64);
        assert!(e.check_pending_head(&forged, &head).is_err());
        let mut foreign = head.clone();
        foreign.header_sha256 = old;
        assert!(e.check_pending_head(&receipt, &foreign).is_err());
        let current = e.current().unwrap();
        let state = VaultState {
            owner: current.owner.clone(),
            binding: current.binding,
            header: Some(header.clone()),
            header_sha256: head.header_sha256,
            items: vec![],
            inventory_sha256: current.inventory_sha256.clone(),
            pending: vec![],
        };
        e.acknowledge(receipt, state).unwrap();
        assert!(e.active().is_ok());
    }
    #[test]
    fn distinct_item_concurrent_commit_is_saved_and_retires_cached_inventory() {
        let (mut e, mut entropy) = fixture();
        let fields: ItemFields = serde_json::from_value(json!({"kind":"login","title":"Synthetic concurrent","username":"fixture","url":"https://example.test","notes":"","favorite":false,"expires_at":"","secret":"Synthetic encrypted item","totp":""})).unwrap();
        let own = e
            .active()
            .unwrap()
            .seal_item(Uuid::from_u128(21), 1, &fields, &mut entropy)
            .unwrap();
        let peer = e
            .active()
            .unwrap()
            .seal_item(Uuid::from_u128(22), 1, &fields, &mut entropy)
            .unwrap();
        let op = Operation::Put {
            id: Uuid::from_u128(23),
            expected_header: e.current().unwrap().header_sha256.clone(),
            expected_revision: 0,
            item: own.clone(),
        };
        let mut state = e.state.take().unwrap();
        let old = VaultState {
            owner: state.owner.clone(),
            binding: state.binding,
            header: state.header.clone(),
            header_sha256: state.header_sha256.clone(),
            items: vec![],
            inventory_sha256: state.inventory_sha256.clone(),
            pending: vec![],
        };
        state.items = vec![own, peer];
        state.inventory_sha256 = hex(&inventory_digest(&state.items).unwrap());
        let receipt = Receipt {
            operation: op.id(),
            payload_sha256: hash(&serde_json::to_vec(&op).unwrap()),
            state: "committed".into(),
            header_sha256: state.header_sha256.clone(),
            inventory_sha256: state.inventory_sha256.clone(),
            commitment: None,
        };
        e.state = Some(old);
        e.pending = Some(op);
        let outcome = e.acknowledge(receipt, state).unwrap();
        assert_eq!(outcome["saved"], true);
        assert_eq!(outcome["refresh_unlock"], true);
    }
    #[test]
    fn archive_batch_lost_ack_keeps_exact_operation_and_only_advances_after_receipt() {
        let (mut e, mut entropy) = fixture();
        let fields: ItemFields = serde_json::from_value(json!({"kind":"note","title":"Synthetic archive batch","username":"","url":"","notes":"","favorite":false,"expires_at":"","secret":"Synthetic encrypted archive value","totp":"JBSWY3DPEHPK3PXP"})).unwrap();
        e.batch = (31..33)
            .map(|n| {
                e.active()
                    .unwrap()
                    .seal_item(Uuid::from_u128(n), 1, &fields, &mut entropy)
                    .unwrap()
            })
            .collect();
        e.next_import(Uuid::from_u128(41)).unwrap();
        let exact = serde_json::to_vec(e.pending.as_ref().unwrap()).unwrap();
        // Lost network ACK leaves immutable operation/batch/index untouched.
        assert_eq!(e.batch_index, 0);
        assert!(e.next_import(Uuid::from_u128(42)).is_err());
        assert_eq!(
            serde_json::to_vec(e.pending.as_ref().unwrap()).unwrap(),
            exact
        );
        let mut state = e.state.take().unwrap();
        state.items.push(e.batch[0].clone());
        state.inventory_sha256 = hex(&inventory_digest(&state.items).unwrap());
        let op = e.pending.as_ref().unwrap();
        let receipt = Receipt {
            operation: op.id(),
            payload_sha256: hash(&exact),
            state: "committed".into(),
            header_sha256: state.header_sha256.clone(),
            inventory_sha256: state.inventory_sha256.clone(),
            commitment: None,
        };
        let mut wrong = receipt.clone();
        wrong.payload_sha256 = "0".repeat(64);
        // Retain old client inventory until exact durable receipt is checked.
        let mut old: VaultState =
            serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        old.items.clear();
        old.inventory_sha256 = hex(&inventory_digest(&[]).unwrap());
        e.state = Some(old);
        assert!(
            e.acknowledge(
                wrong,
                serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap()
            )
            .is_err()
        );
        assert_eq!(e.batch_index, 0);
        e.acknowledge(receipt, state).unwrap();
        assert_eq!(e.batch_index, 1);
        e.next_import(Uuid::from_u128(43)).unwrap();
        let Operation::Put { item, .. } = e.pending.as_ref().unwrap() else {
            panic!()
        };
        assert_eq!(item.id, e.batch[1].id);
        assert_ne!(item.id, e.batch[0].id);
        // An unpublished remaining operation can cancel without deleting ACKed data.
        e.dispatch(Request::Abort { receipt: None }).unwrap();
        e.dispatch(Request::CsvCancel).unwrap();
        assert_eq!(e.current().unwrap().items.len(), 1);
        assert!(e.batch.is_empty());
    }
}
