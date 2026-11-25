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
fn status_name(status: ImportStatus) -> &'static str {
    match status {
        ImportStatus::New => "new",
        ImportStatus::ChangedAccount => "changed",
        ImportStatus::Duplicate => "duplicate",
        ImportStatus::Filtered => "filtered",
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
    LegacyV1Preview {
        archive: String,
        secret: SecretText,
        recovery: bool,
    },
    ArchiveSelect {
        rows: Vec<u32>,
    },
    CsvPreview {
        csv: CsvText,
        /// Exact username match (existing behaviour).
        username: Option<SecretText>,
        /// Case-insensitive "contains any of" list; mutually exclusive with `username`.
        usernames: Option<Vec<SecretText>>,
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
    /// Trusted-client preflight with the exact serialized sizes the service
    /// admits against. Only growth beyond the bound is refused, so a historical
    /// vault that is already above it can still edit in place or shrink.
    fn fits_wire(&self, add: &[EncryptedItem]) -> Result<()> {
        let items = &self.current()?.items;
        let used = inventory_wire_bytes(items)?;
        let mut next = used;
        for item in add {
            if let Some(old) = items.iter().find(|v| v.id == item.id) {
                next -= item_wire_cost(old)?;
            }
            next = next.checked_add(item_wire_cost(item)?).ok_or(Error::Limit)?;
        }
        if next > INVENTORY_WIRE_LIMIT && next > used {
            return Err(Error::Limit);
        }
        Ok(())
    }
    /// Keep the delivered-count when the exact same immutable batch is selected
    /// again (retry, duplicate click); only a different batch restarts at zero.
    fn adopt_batch(&mut self, batch: Vec<EncryptedItem>) {
        let same = !batch.is_empty()
            && batch.len() == self.batch.len()
            && batch.iter().zip(&self.batch).all(|(a, b)| a.id == b.id);
        self.batch = batch;
        if !same {
            self.batch_index = 0;
        }
    }
    fn preview_archive(
        &mut self,
        archive: &str,
        secret: &SecretText,
        recovery: bool,
        legacy: bool,
    ) -> Result<Value> {
        self.mutable()?;
        self.no_import()?;
        let items = self.current()?.items.clone();
        let import = if legacy {
            ArchiveImport::preview_legacy_v1(
                archive.as_bytes(),
                secret,
                recovery,
                self.active()?,
                &items,
                &Continue,
            )?
        } else {
            ArchiveImport::preview(
                archive.as_bytes(),
                secret,
                recovery,
                self.active()?,
                &items,
                &Continue,
            )?
        };
        let p = import.counts();
        let result = json!({"eligible":p.eligible,"duplicates":p.duplicates,"filtered":0,"existing":p.existing,"capacity":p.capacity,"limit":MAX_ITEMS,"bytes_used":import.existing_bytes(),"bytes_limit":INVENTORY_WIRE_LIMIT,"rows":p.rows.iter().map(|r|json!({"row":r.row,"status":status_name(r.status),"blank_password":false,"unsupported_url":false,"invalid_otp":false})).collect::<Vec<_>>()});
        self.archive_import = Some(import);
        self.batch_index = 0;
        Ok(result)
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
        if state.owner != self.owner || state.items.len() > MAX_ITEMS || state.pending.len() > 32 {
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
                self.fits_wire(std::slice::from_ref(&encrypted))?;
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
            } => self.preview_archive(&archive, &secret, recovery, false),
            Request::LegacyV1Preview {
                archive,
                secret,
                recovery,
            } => self.preview_archive(&archive, &secret, recovery, true),
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
                self.adopt_batch(result?);
                Ok(json!({"selected":self.batch.len()}))
            }
            Request::CsvPreview {
                csv,
                username,
                usernames,
            } => {
                self.mutable()?;
                self.no_import()?;
                if csv.0.len() > 10 * 1024 * 1024 {
                    return Err(Error::Limit);
                }
                // Two selectors, or an empty/blank list, never fall back to "all".
                let filter = match (username, usernames) {
                    (Some(_), Some(_)) => return Err(Error::Invalid),
                    (Some(v), None) => UsernameFilter::Exact(v),
                    (None, Some(list)) => UsernameFilter::contains_any(list)?,
                    (None, None) => UsernameFilter::All,
                };
                let items = self.current()?.items.clone();
                self.import = Some(AppleImport::preview(
                    csv.0.as_bytes(),
                    self.active()?,
                    &items,
                    filter,
                    &Continue,
                )?);
                self.batch.clear();
                self.batch_index = 0;
                let staged = self.import.as_ref().ok_or(Error::State)?;
                let (p, bytes) = (staged.counts(), staged.existing_bytes());
                Ok(
                    json!({"eligible":p.eligible,"duplicates":p.duplicates,"filtered":p.filtered,"existing":p.existing,"capacity":p.capacity,"limit":MAX_ITEMS,"bytes_used":bytes,"bytes_limit":INVENTORY_WIRE_LIMIT,"rows":p.rows.iter().map(|r|json!({"row":r.row,"status":status_name(r.status),"blank_password":r.blank_password,"unsupported_url":r.unsupported_url,"invalid_otp":r.invalid_otp})).collect::<Vec<_>>()}),
                )
            }
            Request::CsvSelect { rows } => {
                self.mutable()?;
                let mut import = self.import.take().ok_or(Error::State)?;
                // Keep the staged import on failure (e.g. a known over-capacity
                // selection) so the user can adjust the selection and retry.
                let result = match self.active() {
                    Ok(active) => import
                        .seal_selected(active, &rows, &mut BrowserEntropy, &Continue)
                        .map(|v| v.to_vec()),
                    Err(e) => Err(e),
                };
                self.import = Some(import);
                self.adopt_batch(result?);
                Ok(json!({"selected":self.batch.len()}))
            }
            Request::CsvNext => {
                self.mutable()?;
                // No selection is a state error, never "finished". Finished means
                // every selected item is acknowledged and is checked BEFORE any
                // capacity test, so an inventory of exactly MAX_ITEMS is not a failure.
                if self.batch.is_empty() {
                    return Err(Error::State);
                }
                if self.batch_index >= self.batch.len() {
                    return Ok(Value::Null);
                }
                self.next_import(id)
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
    fn next_import(&mut self, operation_id: impl FnOnce() -> Result<Uuid>) -> Result<Value> {
        self.mutable()?;
        // The whole remaining batch must still fit; a known over-limit import is
        // refused before its next operation is created, never half-published.
        let remaining = self.batch.len().saturating_sub(self.batch_index);
        if self.current()?.items.len().saturating_add(remaining) > MAX_ITEMS {
            return Err(Error::Limit);
        }
        self.fits_wire(&self.batch[self.batch_index..])?;
        let item = self
            .batch
            .get(self.batch_index)
            .ok_or(Error::State)?
            .clone();
        self.pending = Some(Operation::Put {
            id: operation_id()?,
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
    fn legacy_v1_batch_retains_exact_operation_until_ack_and_cancel_keeps_committed_items() {
        let (mut e, mut entropy) = fixture();
        let archive =
            include_str!("../../moosuri-vault-core/tests/fixtures/legacy-v1-webcrypto.json");
        let meta: Value = serde_json::from_str(include_str!(
            "../../moosuri-vault-core/tests/fixtures/legacy-v1-vector-meta.json"
        ))
        .unwrap();
        let secret = || SecretText::new(meta["recovery"].as_str().unwrap().into()).unwrap();
        assert!(
            e.dispatch(Request::ArchivePreview {
                archive: archive.into(),
                secret: secret(),
                recovery: true
            })
            .is_err()
        );
        assert!(e.no_import().is_ok());
        let preview = e
            .dispatch(Request::LegacyV1Preview {
                archive: archive.into(),
                secret: secret(),
                recovery: true,
            })
            .unwrap();
        assert_eq!(preview["eligible"], 2);
        assert!(!preview.to_string().contains("SYNTHETIC"));
        let mut import = e.archive_import.take().unwrap();
        e.batch = import
            .seal_selected(e.active().unwrap(), &[1, 2], &mut entropy, &Continue)
            .unwrap()
            .to_vec();
        e.archive_import = Some(import);
        e.next_import(|| Ok(Uuid::from_u128(441))).unwrap();
        let exact = serde_json::to_vec(e.pending.as_ref().unwrap()).unwrap();
        assert_eq!(e.batch_index, 0);
        assert!(e.next_import(|| Ok(Uuid::from_u128(442))).is_err());
        assert_eq!(
            serde_json::to_vec(e.pending.as_ref().unwrap()).unwrap(),
            exact
        );
        let mut state = e.state.take().unwrap();
        state.items.push(e.batch[0].clone());
        state.inventory_sha256 = hex(&inventory_digest(&state.items).unwrap());
        let receipt = Receipt {
            operation: e.pending.as_ref().unwrap().id(),
            payload_sha256: hash(&exact),
            state: "committed".into(),
            header_sha256: state.header_sha256.clone(),
            inventory_sha256: state.inventory_sha256.clone(),
            commitment: None,
        };
        let mut wrong = receipt.clone();
        wrong.payload_sha256 = "00".repeat(32);
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
        e.dispatch(Request::CsvCancel).unwrap();
        assert!(e.batch.is_empty());
        assert!(e.archive_import.is_none());
        assert_eq!(e.current().unwrap().items.len(), 1);
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
        e.next_import(|| Ok(Uuid::from_u128(41))).unwrap();
        let exact = serde_json::to_vec(e.pending.as_ref().unwrap()).unwrap();
        // Lost network ACK leaves immutable operation/batch/index untouched.
        assert_eq!(e.batch_index, 0);
        assert!(e.next_import(|| Ok(Uuid::from_u128(42))).is_err());
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
        e.next_import(|| Ok(Uuid::from_u128(43))).unwrap();
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
    fn filler_state(e: &mut VaultEngine, entropy: &mut Synthetic, count: u128) -> VaultState {
        let mut items = vec![];
        for n in 0..count {
            let fields: ItemFields = serde_json::from_value(json!({"kind":"login","title":format!("Synthetic filler {n}"),"username":"fixture","url":"https://example.test","notes":"","favorite":false,"expires_at":"","secret":"Synthetic encrypted item","totp":""})).unwrap();
            items.push(
                e.active()
                    .unwrap()
                    .seal_item(Uuid::from_u128(1000 + n), 1, &fields, entropy)
                    .unwrap(),
            );
        }
        let current = e.current().unwrap();
        VaultState {
            owner: current.owner.clone(),
            binding: current.binding,
            header: current.header.clone(),
            header_sha256: current.header_sha256.clone(),
            inventory_sha256: inventory_digest(&items)
                .map(|d| hex(&d))
                .unwrap_or_default(),
            items,
            pending: vec![],
        }
    }
    #[test]
    fn shared_item_limit_loads_beyond_legacy_256_and_rejects_over_limit_state() {
        let (mut e, mut entropy) = fixture();
        let state = filler_state(&mut e, &mut entropy, 300);
        e.dispatch(Request::Load { state }).unwrap();
        assert_eq!(e.current().unwrap().items.len(), 300);
        let found = e
            .dispatch(Request::List {
                query: SecretText::new("filler 29".into()).unwrap(),
            })
            .unwrap();
        assert_eq!(found.as_array().unwrap().len(), 11);
        let full = filler_state(&mut e, &mut entropy, MAX_ITEMS as u128);
        e.dispatch(Request::Load { state: full }).unwrap();
        let over = filler_state(&mut e, &mut entropy, MAX_ITEMS as u128 + 1);
        assert!(e.dispatch(Request::Load { state: over }).is_err());
        assert_eq!(e.current().unwrap().items.len(), MAX_ITEMS);
    }
    #[test]
    fn csv_any_of_filter_reports_capacity_and_refuses_over_limit_selection_without_partial_batch() {
        let (mut e, mut entropy) = fixture();
        let state = filler_state(&mut e, &mut entropy, MAX_ITEMS as u128 - 2);
        e.dispatch(Request::Load { state }).unwrap();
        let csv = "Title,URL,Username,Password,OTPAuth\n\
            a,https://a.test,Fixture.One,pw1,JBSWY3DPEHPK3PXP\n\
            b,https://b.test,FIXTURE.two,pw2,not-a-seed\n\
            c,https://c.test,fixture.THREE,pw3,\n\
            d,https://d.test,someone-else,pw4,garbage\n";
        let secret = |v: &str| SecretText::new(v.into()).unwrap();
        let preview = |e: &mut VaultEngine, one: Option<&str>, any: Option<Vec<&str>>| {
            e.dispatch(Request::CsvPreview {
                csv: CsvText(Zeroizing::new(csv.into())),
                username: one.map(secret),
                usernames: any.map(|l| l.into_iter().map(secret).collect()),
            })
        };
        // Ambiguous or broad selectors are refused, never treated as "all".
        assert!(preview(&mut e, Some("x"), Some(vec!["fixture."])).is_err());
        assert!(preview(&mut e, None, Some(vec![])).is_err());
        assert!(preview(&mut e, None, Some(vec![""])).is_err());
        assert!(preview(&mut e, None, Some(vec!["fixture.", " "])).is_err());
        assert!(e.import.is_none());
        let p = preview(&mut e, None, Some(vec!["FIXTURE."])).unwrap();
        assert_eq!(
            (p["eligible"].clone(), p["filtered"].clone(), p["duplicates"].clone()),
            (json!(3), json!(1), json!(0))
        );
        assert_eq!(
            (p["existing"].clone(), p["capacity"].clone(), p["limit"].clone()),
            (json!(MAX_ITEMS - 2), json!(2), json!(MAX_ITEMS))
        );
        assert_eq!(p["rows"].as_array().unwrap().len(), 3);
        assert_eq!(p["rows"][1]["invalid_otp"], true);
        assert!(!p.to_string().contains("pw1"));
        // Three rows cannot fit in two slots: refused whole, staged import survives.
        assert!(matches!(
            e.dispatch(Request::CsvSelect { rows: vec![2, 3, 4] }),
            Err(Error::Limit)
        ));
        assert!(e.batch.is_empty() && e.import.is_some());
        let mut import = e.import.take().unwrap();
        e.batch = import
            .seal_selected(e.active().unwrap(), &[2, 3], &mut entropy, &Continue)
            .unwrap()
            .to_vec();
        e.import = Some(import);
        e.next_import(|| Ok(Uuid::from_u128(500))).unwrap();
        e.dispatch(Request::Abort { receipt: None }).unwrap();
        // Another writer filled a slot: the remaining batch no longer fits.
        let extra = e.batch[0].clone();
        e.state.as_mut().unwrap().items.push(extra);
        assert!(matches!(
            e.next_import(|| Ok(Uuid::from_u128(501))),
            Err(Error::Limit)
        ));
        assert!(e.pending.is_none());
        e.dispatch(Request::CsvCancel).unwrap();
        let p = preview(&mut e, Some("Fixture.One"), None).unwrap();
        // The concurrent writer inserted the exact first row, so it is now a duplicate.
        assert_eq!((p["eligible"].clone(), p["duplicates"].clone(), p["filtered"].clone()), (json!(0), json!(1), json!(3)));
        let p = preview(&mut e, None, None);
        assert!(p.is_err()); // import still staged
    }
    fn sealed(e: &mut VaultEngine, entropy: &mut Synthetic, n: u128) -> EncryptedItem {
        let fields: ItemFields = serde_json::from_value(json!({"kind":"login","title":format!("Synthetic batch {n}"),"username":"fixture","url":"https://example.test","notes":"","favorite":false,"expires_at":"","secret":"Synthetic encrypted item","totp":""})).unwrap();
        e.active()
            .unwrap()
            .seal_item(Uuid::from_u128(7000 + n), 1, &fields, entropy)
            .unwrap()
    }
    #[test]
    fn repeated_selection_keeps_delivered_index_but_a_different_batch_restarts() {
        let (mut e, mut entropy) = fixture();
        let batch: Vec<_> = (0..3).map(|n| sealed(&mut e, &mut entropy, n)).collect();
        e.adopt_batch(batch.clone());
        e.batch_index = 2;
        // Retry or duplicate click returns the identical immutable ciphertexts.
        e.adopt_batch(batch.clone());
        assert_eq!(e.batch_index, 2);
        e.adopt_batch(batch[..2].to_vec());
        assert_eq!(e.batch_index, 0);
        e.batch_index = 1;
        e.adopt_batch(vec![]);
        assert_eq!((e.batch.len(), e.batch_index), (0, 0));
    }
    #[test]
    fn finished_batch_at_exactly_max_items_is_null_but_capacity_failure_and_no_batch_are_errors() {
        let (mut e, mut entropy) = fixture();
        let state = filler_state(&mut e, &mut entropy, MAX_ITEMS as u128);
        e.dispatch(Request::Load { state }).unwrap();
        // No selection is never reported as "finished".
        assert!(matches!(e.dispatch(Request::CsvNext), Err(Error::State)));
        let extra = sealed(&mut e, &mut entropy, 1);
        e.batch = vec![extra];
        // Not yet delivered: a real capacity failure.
        assert!(matches!(e.dispatch(Request::CsvNext), Err(Error::Limit)));
        assert!(e.pending.is_none());
        // Fully delivered inventory of exactly MAX_ITEMS: finished, not an error.
        e.batch_index = 1;
        assert!(e.dispatch(Request::CsvNext).unwrap().is_null());
    }
    #[test]
    fn exact_wire_size_preflight_refuses_growth_only() {
        let (mut e, mut entropy) = fixture();
        let synthetic = |n: u128, bytes: usize| EncryptedItem {
            ciphertext: "A".repeat(bytes / 3 * 4),
            generation: 1,
            id: Uuid::from_u128(n + 1),
            key_nonce: "A".repeat(16),
            nonce: "A".repeat(16),
            revision: 1,
            version: 2,
            wrapped_key: "A".repeat(64),
        };
        let mut items = vec![];
        let mut used = 0;
        while used + item_wire_cost(&synthetic(0, 30_000)).unwrap() <= INVENTORY_WIRE_LIMIT {
            let next = synthetic(items.len() as u128, 30_000);
            used += item_wire_cost(&next).unwrap();
            items.push(next);
        }
        assert!(items.len() < MAX_ITEMS);
        e.state.as_mut().unwrap().items = items;
        assert!(matches!(
            e.fits_wire(&[synthetic(9_000, 30_000)]),
            Err(Error::Limit)
        ));
        assert!(e.fits_wire(&[synthetic(0, 30_000)]).is_ok()); // same-size edit
        // A real small item still fits an inventory with free room.
        let small = sealed(&mut e, &mut entropy, 9);
        e.state.as_mut().unwrap().items.truncate(10);
        assert!(e.fits_wire(&[small]).is_ok());
    }
}
