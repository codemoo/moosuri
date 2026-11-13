use crate::{Error, ItemFields, Result, SecretText, TrustedEntropy, secret::Key, wire::*};
use aes_gcm::{Aes256Gcm, KeyInit, aead::AeadInOut};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashSet};
use subtle::ConstantTimeEq;
use uuid::Uuid;
use zeroize::Zeroizing;

const NONCE_CAP: usize = 100_000;
type Nonces = HashSet<[u8; 12]>;
pub struct RecoveryKey(Zeroizing<[u8; 32]>);
impl RecoveryKey {
    pub fn parse(value: &SecretText) -> Result<Self> {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let text = value.expose();
        let encoded = text.strip_prefix("MR2-").ok_or(Error::Invalid)?;
        if encoded.len() != 43 {
            return Err(Error::Invalid);
        }
        let bytes = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(encoded)
                .map_err(|_| Error::Invalid)?,
        );
        let canonical = Zeroizing::new(URL_SAFE_NO_PAD.encode(&*bytes));
        if bytes.len() != 32 || canonical.as_str() != encoded {
            return Err(Error::Invalid);
        }
        let mut result = Zeroizing::new([0; 32]);
        result.copy_from_slice(&bytes);
        Ok(Self(result))
    }
    pub fn expose_encoded(&self) -> SecretText {
        use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
        let encoded = Zeroizing::new(URL_SAFE_NO_PAD.encode(self.0.as_slice()));
        SecretText(Zeroizing::new(format!("MR2-{}", encoded.as_str())))
    }
    fn random(entropy: &mut impl TrustedEntropy) -> Result<Self> {
        Ok(Self(Key::random(entropy)?.0))
    }
}
pub(crate) fn random_id(entropy: &mut impl TrustedEntropy) -> Result<Uuid> {
    let mut b = [0; 16];
    entropy.fill(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(Uuid::from_bytes(b))
}
fn random<const N: usize>(entropy: &mut impl TrustedEntropy) -> Result<[u8; N]> {
    let mut bytes = [0; N];
    entropy.fill(&mut bytes)?;
    Ok(bytes)
}
fn password_key(password: &SecretText, salt: &[u8], policy: &PasswordKdf) -> Result<Key> {
    policy.validate()?;
    if password.expose().is_empty() || password.expose().len() > 1024 || salt.len() != 16 {
        return Err(Error::Invalid);
    }
    let mut key = Zeroizing::new([0; 32]);
    let params = Params::new(policy.memory_kib, policy.iterations, policy.lanes, Some(32))
        .map_err(|_| Error::KdfPolicy)?;
    // argon2's convenience allocator does not wipe its full memory arena.
    // Own the bounded arena so every return path wipes it before deallocation.
    let mut memory = Zeroizing::new(Vec::new());
    memory
        .try_reserve_exact(params.block_count())
        .map_err(|_| Error::Limit)?;
    memory.resize(params.block_count(), argon2::Block::new());
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into_with_memory(
            password.expose().as_bytes(),
            salt,
            key.as_mut(),
            &mut *memory,
        )
        .map_err(|_| Error::Authentication)?;
    Ok(Key(key))
}
#[derive(Serialize)]
struct EnvelopeMeta<'a> {
    created_version: u8,
    generation: u64,
    id: Uuid,
    kdf: &'a Kdf,
    kind: EnvelopeKind,
    salt: &'a str,
}
#[derive(Serialize)]
struct EnvelopeAad<'a> {
    binding: Binding,
    domain: &'static str,
    envelope: EnvelopeMeta<'a>,
}
fn envelope_aad(binding: Binding, e: &Envelope) -> Result<Vec<u8>> {
    json(
        &EnvelopeAad {
            binding,
            domain: "moosuri-vault/v2/envelope",
            envelope: EnvelopeMeta {
                created_version: e.created_version,
                generation: e.generation,
                id: e.id,
                kdf: &e.kdf,
                kind: e.kind,
                salt: &e.salt,
            },
        },
        4096,
    )
}
#[derive(Serialize)]
struct ItemAad {
    binding: Binding,
    domain: &'static str,
    generation: u64,
    item_id: Uuid,
    revision: u64,
    version: u8,
}
fn item_aad(binding: Binding, item: &EncryptedItem, key: bool) -> Result<Vec<u8>> {
    json(
        &ItemAad {
            binding,
            domain: if key {
                "moosuri-vault/v2/item/key"
            } else {
                "moosuri-vault/v2/item/payload"
            },
            generation: item.generation,
            item_id: item.id,
            revision: item.revision,
            version: 2,
        },
        4096,
    )
}
pub(crate) fn encrypt(key: &Key, nonce: &[u8], aad: &[u8], plaintext: &[u8]) -> Result<String> {
    if nonce.len() != 12 || plaintext.len() > crate::secret::ITEM_LIMIT {
        return Err(Error::Limit);
    }
    let mut bytes = Zeroizing::new(plaintext.to_vec());
    let cipher = Aes256Gcm::new_from_slice(key.0.as_ref()).map_err(|_| Error::Invalid)?;
    let nonce: [u8; 12] = nonce.try_into().map_err(|_| Error::Invalid)?;
    cipher
        .encrypt_in_place(&nonce.into(), aad, &mut *bytes)
        .map_err(|_| Error::Authentication)?;
    Ok(b64(&bytes))
}
fn decrypt(
    key: &Key,
    nonce: &str,
    aad: &[u8],
    ciphertext: &str,
    cap: usize,
) -> Result<Zeroizing<Vec<u8>>> {
    let nonce = unb64(nonce, 12, 12)?;
    let mut bytes = Zeroizing::new(unb64(ciphertext, 16, cap + 16)?);
    let cipher = Aes256Gcm::new_from_slice(key.0.as_ref()).map_err(|_| Error::Invalid)?;
    let nonce: [u8; 12] = nonce.as_slice().try_into().map_err(|_| Error::Invalid)?;
    cipher
        .decrypt_in_place(&nonce.into(), aad, &mut *bytes)
        .map_err(|_| Error::Authentication)?;
    Ok(bytes)
}
#[derive(Serialize)]
struct RecoveryInfo {
    binding: Binding,
    domain: &'static str,
    envelope_id: Uuid,
}
fn recovery_key(binding: Binding, e: &Envelope, recovery: &RecoveryKey) -> Result<Key> {
    let salt = unb64(&e.salt, 16, 16)?;
    let info = json(
        &RecoveryInfo {
            binding,
            domain: "moosuri-vault/v2/recovery",
            envelope_id: e.id,
        },
        4096,
    )?;
    let mut key = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(&salt), recovery.0.as_ref())
        .expand(&info, key.as_mut())
        .map_err(|_| Error::Invalid)?;
    Ok(Key(key))
}
fn make_envelope(
    binding: Binding,
    generation: u64,
    root: &Key,
    value: Unlock<'_>,
    entropy: &mut impl TrustedEntropy,
) -> Result<Envelope> {
    let kind = match value {
        Unlock::Password(_) => EnvelopeKind::Password,
        Unlock::Recovery(_) => EnvelopeKind::Recovery,
    };
    let mut e = Envelope {
        ciphertext: String::new(),
        created_version: 2,
        generation,
        id: random_id(entropy)?,
        kdf: if kind == EnvelopeKind::Password {
            Kdf::Argon2id13 {
                params: PasswordKdf::default(),
            }
        } else {
            Kdf::HkdfSha256
        },
        kind,
        nonce: b64(&random::<12>(entropy)?),
        salt: b64(&random::<16>(entropy)?),
    };
    let wrapping = match value {
        Unlock::Password(p) => password_key(p, &unb64(&e.salt, 16, 16)?, &PasswordKdf::default())?,
        Unlock::Recovery(r) => recovery_key(binding, &e, r)?,
    };
    e.ciphertext = encrypt(
        &wrapping,
        &unb64(&e.nonce, 12, 12)?,
        &envelope_aad(binding, &e)?,
        root.0.as_ref(),
    )?;
    Ok(e)
}
enum Unlock<'a> {
    Password(&'a SecretText),
    Recovery(&'a RecoveryKey),
}
#[derive(Serialize)]
struct HeaderAuth<'a> {
    binding: Binding,
    domain: &'static str,
    envelope_epoch: u64,
    envelopes: &'a [Envelope],
    generation: u64,
    revision: u64,
    version: u8,
}
fn header_auth(h: &Header, root: &Key) -> Result<Vec<u8>> {
    let bytes = json(
        &HeaderAuth {
            binding: h.binding,
            domain: "moosuri-vault/v2/header",
            envelope_epoch: h.envelope_epoch,
            envelopes: &h.envelopes,
            generation: h.generation,
            revision: h.revision,
            version: 2,
        },
        HEADER_LIMIT,
    )?;
    let mut derived = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(Some(h.binding.vault_id.as_bytes()), root.0.as_ref())
        .expand(b"moosuri-vault/v2/header-auth", derived.as_mut())
        .map_err(|_| Error::Invalid)?;
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(derived.as_ref()).map_err(|_| Error::Invalid)?;
    mac.update(&bytes);
    Ok(mac.finalize().into_bytes().to_vec())
}
fn authenticate_header(h: &mut Header, root: &Key) -> Result<()> {
    h.authentication = b64(&header_auth(h, root)?);
    Ok(())
}
fn open_root(h: &Header, expected: Binding, value: Unlock<'_>) -> Result<Key> {
    h.validate(expected)?;
    let e = &h.envelopes[match value {
        Unlock::Password(_) => 0,
        Unlock::Recovery(_) => 1,
    }];
    let wrapping = match value {
        Unlock::Password(p) => password_key(
            p,
            &unb64(&e.salt, 16, 16)?,
            match &e.kdf {
                Kdf::Argon2id13 { params } => params,
                _ => return Err(Error::KdfPolicy),
            },
        )?,
        Unlock::Recovery(r) => recovery_key(expected, e, r)?,
    };
    let bytes = decrypt(
        &wrapping,
        &e.nonce,
        &envelope_aad(expected, e)?,
        &e.ciphertext,
        32,
    )?;
    if bytes.len() != 32 {
        return Err(Error::Authentication);
    }
    let mut root = Zeroizing::new([0; 32]);
    root.copy_from_slice(&bytes);
    let root = Key(root);
    if !bool::from(
        header_auth(h, &root)?
            .as_slice()
            .ct_eq(&unb64(&h.authentication, 32, 32)?),
    ) {
        return Err(Error::Authentication);
    }
    Ok(root)
}
#[derive(Clone, Copy)]
pub struct StorageReceipt {
    candidate_id: Uuid,
    digest: [u8; 32],
}
/// Admission from an authenticated current storage read, or a verified offline
/// archive manifest. This is not itself an authenticated protocol or journal.
#[derive(Clone, Copy)]
pub struct HeaderReceipt {
    binding: Binding,
    header_sha256: [u8; 32],
}
impl HeaderReceipt {
    /// TRUSTED boundary: do not synthesize this from an uncommitted candidate or
    /// an arbitrary server blob. Service/archive verification is a separate unit.
    pub fn from_verified_current_storage(binding: Binding, header_sha256: [u8; 32]) -> Self {
        Self {
            binding,
            header_sha256,
        }
    }
}
/// Raw candidate headers cannot be passed directly to either unlock function.
///
/// ```compile_fail
/// use moosuri_vault_core::{ActiveVault, Header, Binding, SecretText};
/// fn unlock_raw(h: Header, b: Binding, p: &SecretText) {
///     let _ = ActiveVault::unlock_password(h, b, p);
/// }
/// ```
#[derive(Clone)]
pub struct CommittedHeader {
    header: Header,
}
impl CommittedHeader {
    pub fn admit(header: Header, expected: Binding, receipt: HeaderReceipt) -> Result<Self> {
        header.validate(expected)?;
        let digest: [u8; 32] = Sha256::digest(header.to_json()?).into();
        if receipt.binding != expected || receipt.header_sha256 != digest {
            return Err(Error::State);
        }
        Ok(Self { header })
    }
}
impl StorageReceipt {
    /// TRUSTED boundary: adapter calls only after authenticating an exact durable
    /// storage/CAS ACK, or reconciling a lost ACK. This is not a signature/journal.
    pub fn from_verified_storage(candidate_id: Uuid, digest: [u8; 32]) -> Self {
        Self {
            candidate_id,
            digest,
        }
    }
}
#[derive(Serialize)]
struct Candidate<'a> {
    candidate_id: Uuid,
    domain: &'static str,
    header: &'a Header,
    items: &'a [EncryptedItem],
    parent_sha256: [u8; 32],
}
/// Public ciphertext-only commitment verification for the authenticated storage
/// adapter. Computing this digest does not authorize a candidate or prove fsync.
pub fn storage_commitment(
    id: Uuid,
    h: &Header,
    items: &[EncryptedItem],
    parent: [u8; 32],
) -> Result<[u8; 32]> {
    commitment(id, h, items, parent)
}
fn commitment(id: Uuid, h: &Header, items: &[EncryptedItem], parent: [u8; 32]) -> Result<[u8; 32]> {
    if items.len() > MAX_ITEMS {
        return Err(Error::Limit);
    }
    // Hash bounded records incrementally; never allocate a1000-item plaintext or
    // ciphertext JSON blob. Array boundaries/order and parent are authenticated.
    let meta = Candidate {
        candidate_id: id,
        domain: "moosuri-vault/v2/candidate",
        header: h,
        items: &[],
        parent_sha256: parent,
    };
    let mut hash = Sha256::new();
    hash.update(json(&meta, HEADER_LIMIT + 512)?);
    hash.update((items.len() as u64).to_be_bytes());
    for item in items {
        let bytes = item.to_json()?;
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    Ok(hash.finalize().into())
}
pub struct PendingVault {
    header: Header,
    root: Key,
    recovery: RecoveryKey,
    id: Uuid,
    recovery_verified: bool,
    confirmed: bool,
}
impl PendingVault {
    pub fn prepare(
        binding: Binding,
        password: &SecretText,
        entropy: &mut impl TrustedEntropy,
    ) -> Result<Self> {
        binding.validate()?;
        let root = Key::random(entropy)?;
        let recovery = RecoveryKey::random(entropy)?;
        let mut header = Header {
            authentication: String::new(),
            binding,
            envelope_epoch: 1,
            envelopes: vec![],
            generation: 1,
            revision: 1,
            version: 2,
        };
        header.envelopes = vec![
            make_envelope(binding, 1, &root, Unlock::Password(password), entropy)?,
            make_envelope(binding, 1, &root, Unlock::Recovery(&recovery), entropy)?,
        ];
        authenticate_header(&mut header, &root)?;
        Ok(Self {
            header,
            root,
            recovery,
            id: random_id(entropy)?,
            recovery_verified: false,
            confirmed: false,
        })
    }
    pub fn header(&self) -> &Header {
        &self.header
    }
    pub fn recovery(&self) -> &RecoveryKey {
        &self.recovery
    }
    pub fn verify_recovery(&mut self, value: &RecoveryKey) -> Result<()> {
        if !bool::from(self.recovery.0.as_ref().ct_eq(value.0.as_ref())) {
            return Err(Error::Authentication);
        }
        self.recovery_verified = true;
        Ok(())
    }
    pub fn candidate(&self) -> Result<(Uuid, [u8; 32])> {
        Ok((self.id, commitment(self.id, &self.header, &[], [0; 32])?))
    }
    pub fn confirm_commit(&mut self, receipt: StorageReceipt) -> Result<()> {
        let (id, digest) = self.candidate()?;
        if !self.recovery_verified || id != receipt.candidate_id || digest != receipt.digest {
            return Err(Error::State);
        }
        self.confirmed = true;
        Ok(())
    }
    pub fn activate(self) -> Result<ActiveVault> {
        if !self.confirmed {
            return Err(Error::State);
        }
        Ok(ActiveVault {
            header: self.header,
            root: self.root,
            nonces: HashSet::new(),
        })
    }
}
pub struct ActiveVault {
    header: Header,
    root: Key,
    nonces: Nonces,
}
impl ActiveVault {
    pub fn unlock_password(
        committed: CommittedHeader,
        expected: Binding,
        password: &SecretText,
    ) -> Result<Self> {
        let header = committed.header;
        let root = open_root(&header, expected, Unlock::Password(password))?;
        Ok(Self {
            header,
            root,
            nonces: HashSet::new(),
        })
    }
    pub fn unlock_recovery(
        committed: CommittedHeader,
        expected: Binding,
        recovery: &RecoveryKey,
    ) -> Result<Self> {
        let header = committed.header;
        let root = open_root(&header, expected, Unlock::Recovery(recovery))?;
        Ok(Self {
            header,
            root,
            nonces: HashSet::new(),
        })
    }
    pub fn header(&self) -> &Header {
        &self.header
    }
    pub fn committed_header(&self) -> CommittedHeader {
        CommittedHeader {
            header: self.header.clone(),
        }
    }
    pub(crate) fn header_digest(&self) -> Result<[u8; 32]> {
        Ok(Sha256::digest(self.header.to_json()?).into())
    }
    pub fn seal_item(
        &mut self,
        id: Uuid,
        revision: u64,
        fields: &ItemFields,
        entropy: &mut impl TrustedEntropy,
    ) -> Result<EncryptedItem> {
        seal(
            &self.header,
            &self.root,
            &mut self.nonces,
            id,
            revision,
            fields,
            entropy,
        )
    }
    pub fn open_item(&mut self, item: &EncryptedItem) -> Result<ItemFields> {
        open(&self.header, &self.root, &mut self.nonces, item)
    }
    pub fn prepare_password_change<'a>(
        &'a mut self,
        password: &SecretText,
        entropy: &mut impl TrustedEntropy,
    ) -> Result<PendingChange<'a>> {
        let mut header = self.header.clone();
        header.envelope_epoch = next(header.envelope_epoch)?;
        header.revision = next(header.revision)?;
        header.envelopes[0] = make_envelope(
            header.binding,
            header.generation,
            &self.root,
            Unlock::Password(password),
            entropy,
        )?;
        authenticate_header(&mut header, &self.root)?;
        PendingChange::new(self, header, None, vec![], None, HashSet::new(), entropy)
    }
    pub fn prepare_recovery_change<'a>(
        &'a mut self,
        entropy: &mut impl TrustedEntropy,
    ) -> Result<PendingChange<'a>> {
        let recovery = RecoveryKey::random(entropy)?;
        let mut header = self.header.clone();
        header.envelope_epoch = next(header.envelope_epoch)?;
        header.revision = next(header.revision)?;
        header.envelopes[1] = make_envelope(
            header.binding,
            header.generation,
            &self.root,
            Unlock::Recovery(&recovery),
            entropy,
        )?;
        authenticate_header(&mut header, &self.root)?;
        PendingChange::new(
            self,
            header,
            None,
            vec![],
            Some(recovery),
            HashSet::new(),
            entropy,
        )
    }
    /// `expected_inventory` must be independently pinned by the trusted client;
    /// an untrusted server-supplied subset cannot prove a complete vault.
    pub fn prepare_root_rotation<'a>(
        &'a mut self,
        password: &SecretText,
        items: &[EncryptedItem],
        expected_inventory: [u8; 32],
        entropy: &mut impl TrustedEntropy,
    ) -> Result<PendingChange<'a>> {
        if inventory_digest(items)? != expected_inventory {
            return Err(Error::Conflict);
        }
        let root = Key::random(entropy)?;
        let recovery = RecoveryKey::random(entropy)?;
        let mut header = self.header.clone();
        header.generation = next(header.generation)?;
        header.envelope_epoch = next(header.envelope_epoch)?;
        header.revision = next(header.revision)?;
        header.envelopes = vec![
            make_envelope(
                header.binding,
                header.generation,
                &root,
                Unlock::Password(password),
                entropy,
            )?,
            make_envelope(
                header.binding,
                header.generation,
                &root,
                Unlock::Recovery(&recovery),
                entropy,
            )?,
        ];
        authenticate_header(&mut header, &root)?;
        let mut nonces = HashSet::new();
        let mut output = Vec::with_capacity(items.len());
        for item in items {
            let fields = self.open_item(item)?;
            output.push(seal(
                &header,
                &root,
                &mut nonces,
                item.id,
                next(item.revision)?,
                &fields,
                entropy,
            )?);
        }
        PendingChange::new(
            self,
            header,
            Some(root),
            output,
            Some(recovery),
            nonces,
            entropy,
        )
    }
}
pub fn inventory_digest(items: &[EncryptedItem]) -> Result<[u8; 32]> {
    if items.len() > MAX_ITEMS {
        return Err(Error::Limit);
    }
    let mut ids = BTreeSet::new();
    let mut hash = Sha256::new();
    hash.update(b"moosuri-vault/v2/inventory\0");
    hash.update((items.len() as u64).to_be_bytes());
    for item in items {
        if !ids.insert(item.id) {
            return Err(Error::Conflict);
        }
        let bytes = item.to_json()?;
        hash.update((bytes.len() as u64).to_be_bytes());
        hash.update(bytes);
    }
    Ok(hash.finalize().into())
}
pub struct PendingChange<'a> {
    active: &'a mut ActiveVault,
    header: Header,
    root: Option<Key>,
    items: Vec<EncryptedItem>,
    recovery: Option<RecoveryKey>,
    nonces: Nonces,
    id: Uuid,
    parent: [u8; 32],
    recovery_verified: bool,
    confirmed: bool,
}
impl<'a> PendingChange<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        active: &'a mut ActiveVault,
        header: Header,
        root: Option<Key>,
        items: Vec<EncryptedItem>,
        recovery: Option<RecoveryKey>,
        nonces: Nonces,
        entropy: &mut impl TrustedEntropy,
    ) -> Result<Self> {
        let parent = active.header_digest()?;
        let recovery_verified = recovery.is_none();
        Ok(Self {
            active,
            header,
            root,
            items,
            recovery,
            nonces,
            id: random_id(entropy)?,
            parent,
            recovery_verified,
            confirmed: false,
        })
    }
    pub fn header(&self) -> &Header {
        &self.header
    }
    pub fn items(&self) -> &[EncryptedItem] {
        &self.items
    }
    pub fn recovery(&self) -> Option<&RecoveryKey> {
        self.recovery.as_ref()
    }
    pub fn verify_recovery(&mut self, value: &RecoveryKey) -> Result<()> {
        let expected = self.recovery.as_ref().ok_or(Error::State)?;
        if !bool::from(expected.0.as_ref().ct_eq(value.0.as_ref())) {
            return Err(Error::Authentication);
        }
        self.recovery_verified = true;
        Ok(())
    }
    pub fn candidate(&self) -> Result<(Uuid, [u8; 32])> {
        Ok((
            self.id,
            commitment(self.id, &self.header, &self.items, self.parent)?,
        ))
    }
    pub fn confirm_commit(&mut self, receipt: StorageReceipt) -> Result<()> {
        let (id, digest) = self.candidate()?;
        if !self.recovery_verified || id != receipt.candidate_id || digest != receipt.digest {
            return Err(Error::State);
        }
        self.confirmed = true;
        Ok(())
    }
    pub fn activate(mut self) -> Result<Vec<EncryptedItem>> {
        if !self.confirmed {
            return Err(Error::State);
        }
        self.active.header = self.header;
        if let Some(root) = self.root.take() {
            self.active.root = root;
            self.active.nonces = std::mem::take(&mut self.nonces)
        }
        Ok(self.items)
    }
}

/// Trusted client state owner for asynchronous storage round trips. Pending
/// changes own their new keys while all active operations remain blocked. No
/// self-referential Rust/JS object or manufactured storage receipt is necessary.
pub struct ClientVault {
    active: ActiveVault,
    pending: Option<OwnedChange>,
}
struct OwnedChange {
    header: Header,
    root: Option<Key>,
    items: Vec<EncryptedItem>,
    recovery: Option<RecoveryKey>,
    nonces: Nonces,
    id: Uuid,
    parent: [u8; 32],
    recovery_verified: bool,
    confirmed: bool,
}
impl PendingChange<'_> {
    fn detach(self) -> OwnedChange {
        OwnedChange {
            header: self.header,
            root: self.root,
            items: self.items,
            recovery: self.recovery,
            nonces: self.nonces,
            id: self.id,
            parent: self.parent,
            recovery_verified: self.recovery_verified,
            confirmed: self.confirmed,
        }
    }
}
impl ClientVault {
    pub fn new(active: ActiveVault) -> Self {
        Self {
            active,
            pending: None,
        }
    }
    pub fn active(&mut self) -> Result<&mut ActiveVault> {
        if self.pending.is_some() {
            return Err(Error::State);
        }
        Ok(&mut self.active)
    }
    pub fn prepare_password(
        &mut self,
        password: &SecretText,
        entropy: &mut impl TrustedEntropy,
    ) -> Result<()> {
        if self.pending.is_some() {
            return Err(Error::State);
        }
        self.pending = Some(
            self.active
                .prepare_password_change(password, entropy)?
                .detach(),
        );
        Ok(())
    }
    pub fn prepare_recovery(&mut self, entropy: &mut impl TrustedEntropy) -> Result<()> {
        if self.pending.is_some() {
            return Err(Error::State);
        }
        self.pending = Some(self.active.prepare_recovery_change(entropy)?.detach());
        Ok(())
    }
    pub fn prepare_rotation(
        &mut self,
        password: &SecretText,
        items: &[EncryptedItem],
        inventory: [u8; 32],
        entropy: &mut impl TrustedEntropy,
    ) -> Result<()> {
        if self.pending.is_some() {
            return Err(Error::State);
        }
        self.pending = Some(
            self.active
                .prepare_root_rotation(password, items, inventory, entropy)?
                .detach(),
        );
        Ok(())
    }
    pub fn pending_header(&self) -> Result<&Header> {
        Ok(&self.pending.as_ref().ok_or(Error::State)?.header)
    }
    pub fn pending_items(&self) -> Result<&[EncryptedItem]> {
        Ok(&self.pending.as_ref().ok_or(Error::State)?.items)
    }
    pub fn pending_recovery(&self) -> Result<Option<&RecoveryKey>> {
        Ok(self.pending.as_ref().ok_or(Error::State)?.recovery.as_ref())
    }
    pub fn verify_recovery(&mut self, value: &RecoveryKey) -> Result<()> {
        let p = self.pending.as_mut().ok_or(Error::State)?;
        let expected = p.recovery.as_ref().ok_or(Error::State)?;
        if !bool::from(expected.0.as_ref().ct_eq(value.0.as_ref())) {
            return Err(Error::Authentication);
        }
        p.recovery_verified = true;
        Ok(())
    }
    pub fn pending_candidate(&self) -> Result<(Uuid, [u8; 32])> {
        let p = self.pending.as_ref().ok_or(Error::State)?;
        Ok((p.id, commitment(p.id, &p.header, &p.items, p.parent)?))
    }
    pub fn confirm_commit(&mut self, receipt: StorageReceipt) -> Result<()> {
        let (id, digest) = self.pending_candidate()?;
        let p = self.pending.as_mut().ok_or(Error::State)?;
        if !p.recovery_verified || id != receipt.candidate_id || digest != receipt.digest {
            return Err(Error::State);
        }
        p.confirmed = true;
        Ok(())
    }
    pub fn activate(&mut self) -> Result<Vec<EncryptedItem>> {
        let p = self.pending.as_ref().ok_or(Error::State)?;
        if !p.confirmed || p.parent != self.active.header_digest()? {
            return Err(Error::State);
        }
        let p = self.pending.take().ok_or(Error::State)?;
        PendingChange {
            active: &mut self.active,
            header: p.header,
            root: p.root,
            items: p.items,
            recovery: p.recovery,
            nonces: p.nonces,
            id: p.id,
            parent: p.parent,
            recovery_verified: p.recovery_verified,
            confirmed: p.confirmed,
        }
        .activate()
    }
    /// Only after exact storage reconciliation says this candidate was not adopted.
    pub fn abort_pending(&mut self) {
        self.pending = None;
    }
}

#[cfg(all(test, feature = "native-entropy"))]
mod client_state_tests {
    use super::*;
    #[test]
    fn owned_pending_keeps_old_header_blocks_active_work_and_requires_matching_receipt() {
        let b = Binding {
            owner_id: Uuid::from_u128(1),
            tenant_id: Uuid::from_u128(2),
            vault_id: Uuid::from_u128(3),
        };
        let password = SecretText::new("Synthetic owned client password".into()).unwrap();
        let mut p = PendingVault::prepare(b, &password, &mut crate::OsEntropy).unwrap();
        let recovery = RecoveryKey::parse(&p.recovery().expose_encoded()).unwrap();
        p.verify_recovery(&recovery).unwrap();
        let (id, d) = p.candidate().unwrap();
        p.confirm_commit(StorageReceipt::from_verified_storage(id, d))
            .unwrap();
        let mut c = ClientVault::new(p.activate().unwrap());
        let original = c.active().unwrap().header().to_json().unwrap();
        c.prepare_recovery(&mut crate::OsEntropy).unwrap();
        assert!(c.active().is_err());
        let (id, d) = c.pending_candidate().unwrap();
        assert!(
            c.confirm_commit(StorageReceipt::from_verified_storage(id, d))
                .is_err()
        );
        assert!(c.activate().is_err());
        c.abort_pending();
        assert_eq!(c.active().unwrap().header().to_json().unwrap(), original);
        c.prepare_password(
            &SecretText::new("Synthetic replacement password".into()).unwrap(),
            &mut crate::OsEntropy,
        )
        .unwrap();
        let (id, d) = c.pending_candidate().unwrap();
        assert!(
            c.confirm_commit(StorageReceipt::from_verified_storage(id, [0; 32]))
                .is_err()
        );
        assert!(c.activate().is_err());
        c.confirm_commit(StorageReceipt::from_verified_storage(id, d))
            .unwrap();
        c.activate().unwrap();
        assert_ne!(c.active().unwrap().header().to_json().unwrap(), original);
        let active = c.active().unwrap();
        let h = active.committed_header();
        assert!(
            ActiveVault::unlock_password(
                h,
                b,
                &SecretText::new("Synthetic replacement password".into()).unwrap()
            )
            .is_ok()
        );
    }
}

fn fresh_nonce(nonces: &mut Nonces, entropy: &mut impl TrustedEntropy) -> Result<[u8; 12]> {
    if nonces.len() >= NONCE_CAP {
        return Err(Error::Limit);
    }
    for _ in 0..16 {
        let nonce = random::<12>(entropy)?;
        if nonces.insert(nonce) {
            return Ok(nonce);
        }
    }
    Err(Error::Entropy)
}
fn seal(
    header: &Header,
    root: &Key,
    nonces: &mut Nonces,
    id: Uuid,
    revision: u64,
    fields: &ItemFields,
    entropy: &mut impl TrustedEntropy,
) -> Result<EncryptedItem> {
    rev(revision)?;
    if id.is_nil() {
        return Err(Error::Invalid);
    }
    let plain = fields.encode()?;
    let key = Key::random(entropy)?;
    let wrap_nonce = fresh_nonce(nonces, entropy)?;
    let nonce = random::<12>(entropy)?;
    let mut item = EncryptedItem {
        ciphertext: String::new(),
        generation: header.generation,
        id,
        key_nonce: b64(&wrap_nonce),
        nonce: b64(&nonce),
        revision,
        version: 2,
        wrapped_key: String::new(),
    };
    item.wrapped_key = encrypt(
        root,
        &wrap_nonce,
        &item_aad(header.binding, &item, true)?,
        key.0.as_ref(),
    )?;
    item.ciphertext = encrypt(
        &key,
        &nonce,
        &item_aad(header.binding, &item, false)?,
        &plain,
    )?;
    Ok(item)
}
fn open(
    header: &Header,
    root: &Key,
    nonces: &mut Nonces,
    item: &EncryptedItem,
) -> Result<ItemFields> {
    item.validate()?;
    if item.generation != header.generation {
        return Err(Error::Authentication);
    }
    let nonce = unb64(&item.key_nonce, 12, 12)?;
    let mut fixed = [0; 12];
    fixed.copy_from_slice(&nonce);
    if !nonces.contains(&fixed) && nonces.len() >= NONCE_CAP {
        return Err(Error::Limit);
    }
    nonces.insert(fixed);
    let bytes = decrypt(
        root,
        &item.key_nonce,
        &item_aad(header.binding, item, true)?,
        &item.wrapped_key,
        32,
    )?;
    if bytes.len() != 32 {
        return Err(Error::Authentication);
    }
    let mut key = Zeroizing::new([0; 32]);
    key.copy_from_slice(&bytes);
    let key = Key(key);
    let plain = decrypt(
        &key,
        &item.nonce,
        &item_aad(header.binding, item, false)?,
        &item.ciphertext,
        crate::secret::ITEM_LIMIT,
    )?;
    ItemFields::decode(&plain)
}
