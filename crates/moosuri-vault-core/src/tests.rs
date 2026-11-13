use crate::*;
use crate::{
    secret::Key,
    wire::{b64, unb64},
};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

struct Synthetic(u64);
impl TrustedEntropy for Synthetic {
    fn fill(&mut self, out: &mut [u8]) -> Result<()> {
        for chunk in out.chunks_mut(32) {
            self.0 += 1;
            let b = Sha256::digest(self.0.to_be_bytes());
            chunk.copy_from_slice(&b[..chunk.len()]);
        }
        Ok(())
    }
}
fn text(s: &str) -> SecretText {
    SecretText::new(s.to_owned()).unwrap()
}
fn binding(n: u128) -> Binding {
    Binding {
        owner_id: Uuid::from_u128(n),
        tenant_id: Uuid::from_u128(100),
        vault_id: Uuid::from_u128(200 + n),
    }
}
fn fields() -> ItemFields {
    ItemFields {
        kind: ItemKind::Login,
        title: text("Synthetic encrypted canary"),
        username: text("synthetic@example.test"),
        url: text("https://example.test/account"),
        secret: text("synthetic-password-canary"),
        notes: text("Unicode: 무수리 🌱"),
        favorite: true,
        expires_at: text("2030-01-01"),
        totp: text("otpauth://totp/Fixture?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"), // gitleaks:allow -- RFC6238 public test seed.
    }
}
fn ready(n: u128, entropy: &mut Synthetic) -> (ActiveVault, RecoveryKey) {
    let mut pending =
        PendingVault::prepare(binding(n), &text("Synthetic master passphrase"), entropy).unwrap();
    let recovery = RecoveryKey::parse(&pending.recovery().expose_encoded()).unwrap();
    pending.verify_recovery(&recovery).unwrap();
    let (id, digest) = pending.candidate().unwrap();
    pending
        .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    (pending.activate().unwrap(), recovery)
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn published_nist_aes256_gcm_and_rfc5869_hkdf_vectors() {
    // NIST SP800-38D AES-256-GCM, zero key/IV and16-byte zero plaintext.
    let key = Key(Zeroizing::new([0; 32]));
    let encrypted = crate::crypto::encrypt(&key, &[0; 12], &[], &[0; 16]).unwrap();
    assert_eq!(
        hex(&unb64(&encrypted, 32, 32).unwrap()),
        "cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919"
    );
    let ikm = [0x0b; 22];
    let salt: Vec<u8> = (0..=12).collect();
    let info: Vec<u8> = (0xf0..=0xf9).collect();
    let mut output = [0; 42];
    hkdf::Hkdf::<Sha256>::new(Some(&salt), &ikm)
        .expand(&info, &mut output)
        .unwrap();
    assert_eq!(
        hex(&output),
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
    );
}
#[test]
fn rfc9106_argon2id13_known_answer() {
    use argon2::{Algorithm, Argon2, AssociatedData, ParamsBuilder, Version};
    let mut params = ParamsBuilder::new();
    params
        .m_cost(32)
        .t_cost(3)
        .p_cost(4)
        .output_len(32)
        .data(AssociatedData::new(&[4; 12]).unwrap());
    let params = params.build().unwrap();
    let mut out = [0; 32];
    let mut memory = Zeroizing::new(vec![argon2::Block::new(); params.block_count()]);
    Argon2::new_with_secret(&[3; 8], Algorithm::Argon2id, Version::V0x13, params)
        .unwrap()
        .hash_password_into_with_memory(&[1; 32], &[2; 16], &mut out, &mut *memory)
        .unwrap();
    assert_eq!(
        hex(&out),
        "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659"
    );
}

#[test]
#[cfg(feature = "native-entropy")]
fn native_os_entropy_adapter_fills_owned_buffers_without_a_network_or_key_service() {
    let mut entropy = OsEntropy;
    let mut a = Zeroizing::new([0; 32]);
    let mut b = Zeroizing::new([0; 32]);
    entropy.fill(a.as_mut()).unwrap();
    entropy.fill(b.as_mut()).unwrap();
    assert!(*a != [0; 32] && *b != [0; 32] && *a != *b);
}
#[test]
fn rfc6238_all_algorithms_and_boundaries() {
    let times = [
        59,
        1_111_111_109,
        1_111_111_111,
        1_234_567_890,
        2_000_000_000,
        20_000_000_000,
    ];
    let vectors = [
        (
            TotpAlgorithm::Sha1,
            20,
            [
                "94287082", "07081804", "14050471", "89005924", "69279037", "65353130",
            ],
        ),
        (
            TotpAlgorithm::Sha256,
            32,
            [
                "46119246", "68084774", "67062674", "91819424", "90698825", "77737706",
            ],
        ),
        (
            TotpAlgorithm::Sha512,
            64,
            [
                "90693936", "25091201", "99943326", "93441116", "38618901", "47863826",
            ],
        ),
    ];
    for (algorithm, length, codes) in vectors {
        let seed: Vec<u8> = (0..length)
            .map(|n| b'1' + (n % 10) as u8)
            .map(|b| if b == b':' { b'0' } else { b })
            .collect();
        let seed = text(&data_encoding::BASE32_NOPAD.encode(&seed));
        let totp = Totp::from_base32(&seed, algorithm, 8, 30).unwrap();
        for (at, expected) in times.into_iter().zip(codes) {
            let code = totp.code_at(at).unwrap();
            assert_eq!(code.expose(), expected);
            assert!(code.valid_from <= at && at < code.valid_until);
        }
    }
    let uri = text(
        "otpauth://totp/Fixture%3AAccount?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Fixture&algorithm=SHA1&digits=8&period=30",
    );
    let totp = Totp::parse_uri(&uri).unwrap();
    assert_eq!(totp.code_at(59).unwrap().expose(), "94287082");
    assert_eq!(totp.code_at(60).unwrap().valid_from, 60);
    for raw in [
        "otpauth://hotp/x?secret=AAAAAAAAAAAAAAAA&counter=1",
        "otpauth://totp/x?secret=AAAAAAAAAAAAAAAA&secret=BBBBBBBBBBBBBBBB",
        "otpauth://totp/x?secret=AAAAAAAAAAAAAAAA&digits=9",
        "otpauth://totp/x?secret=AAAAAAAAAAAAAAAA&period=0",
        "otpauth://totp/x?secret=AAAAAAAAAAAAAAAA&algorithm=MD5",
        "otpauth://totp/x?secret=BAD!",
        "otpauth://totp/x?secret=AAAAAAAAAAAAAAAA#fragment",
    ] {
        assert!(Totp::parse_uri(&text(raw)).is_err());
    }
    assert!(Totp::from_base32(&text(&"A".repeat(513)), TotpAlgorithm::Sha1, 6, 30).is_err());
    assert!(totp.code_at(u64::MAX).is_err());
}
#[test]
fn candidates_require_recovery_and_exact_storage_confirmation() {
    let mut rng = Synthetic(0);
    let mut p =
        PendingVault::prepare(binding(1), &text("Synthetic master passphrase"), &mut rng).unwrap();
    let (id, digest) = p.candidate().unwrap();
    assert!(matches!(
        p.confirm_commit(StorageReceipt::from_verified_storage(id, digest)),
        Err(Error::State)
    ));
    let wrong =
        RecoveryKey::parse(&text("MR2-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")).unwrap();
    assert!(p.verify_recovery(&wrong).is_err());
    let recovery = RecoveryKey::parse(&p.recovery().expose_encoded()).unwrap();
    p.verify_recovery(&recovery).unwrap();
    assert!(
        p.confirm_commit(StorageReceipt::from_verified_storage(id, [0; 32]))
            .is_err()
    );
    p.confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    let active = p.activate().unwrap();
    assert_eq!(active.header().envelope_epoch, 1);
    let p =
        PendingVault::prepare(binding(2), &text("Synthetic master passphrase"), &mut rng).unwrap();
    assert!(matches!(p.activate(), Err(Error::State)));
}
#[test]
fn encrypted_payload_root_recovery_and_owner_item_revision_purpose_tamper() {
    let mut rng = Synthetic(0);
    let (mut a, recovery) = ready(1, &mut rng);
    let header = a.header().clone();
    let item = a
        .seal_item(Uuid::from_u128(500), 1, &fields(), &mut rng)
        .unwrap();
    let encoded = item.to_json().unwrap();
    for needle in [
        "synthetic-password-canary",
        "synthetic@example.test",
        "example.test",
        "무수리",
        "otpauth",
        "Synthetic encrypted canary",
    ] {
        assert!(!String::from_utf8_lossy(&encoded).contains(needle));
    }
    let opened = a.open_item(&item).unwrap();
    assert_eq!(opened.secret.expose(), "synthetic-password-canary");
    assert!(opened.notes.expose().contains("무수리"));
    assert!(unlock_password(header.clone(), binding(1), &text("wrong-passphrase")).is_err());
    assert!(
        unlock_password(
            header.clone(),
            binding(2),
            &text("Synthetic master passphrase")
        )
        .is_err()
    );
    let mut forged = header.clone();
    forged.binding = binding(2);
    assert!(unlock_recovery(forged, binding(2), &recovery).is_err());
    let mut forged = header.clone();
    forged.binding.tenant_id = Uuid::from_u128(101);
    let expected = forged.binding;
    assert!(unlock_recovery(forged, expected, &recovery).is_err());
    let mut restored = unlock_recovery(header.clone(), binding(1), &recovery).unwrap();
    assert_eq!(
        restored.open_item(&item).unwrap().username.expose(),
        "synthetic@example.test"
    );
    for problem in 0..6 {
        let mut bad = item.clone();
        match problem {
            0 => bad.id = Uuid::from_u128(501),
            1 => bad.revision = 2,
            2 => bad.generation = 2,
            3 => {
                let mut bytes = unb64(&bad.ciphertext, 16, 70_000).unwrap();
                bytes[0] ^= 1;
                bad.ciphertext = b64(&bytes)
            }
            4 => bad.ciphertext = bad.wrapped_key.clone(),
            _ => bad.nonce = b64(&[0; 12]),
        }
        assert!(a.open_item(&bad).is_err(), "tamper{problem}");
    }
    let mut changed = header.clone();
    changed.envelope_epoch += 1;
    assert!(unlock_recovery(changed, binding(1), &recovery).is_err());
    let mut changed = header;
    changed.envelopes[0].kind = crate::wire::EnvelopeKind::Recovery;
    assert!(unlock_recovery(changed, binding(1), &recovery).is_err());
}
#[test]
fn hostile_wire_and_kdf_reject_before_work_and_errors_are_fixed() {
    let mut rng = Synthetic(0);
    let (a, _) = ready(1, &mut rng);
    let raw = a.header().to_json().unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    v["envelopes"][0]["kdf"]["params"]["memory_kib"] = serde_json::json!(u32::MAX);
    let bytes = serde_json::to_vec(&v).unwrap();
    assert!(matches!(
        Header::from_json(&bytes, binding(1)),
        Err(Error::KdfPolicy)
    ));
    assert!(matches!(
        Header::from_json(&vec![b' '; 16_385], binding(1)),
        Err(Error::Limit)
    ));
    v = serde_json::from_slice(&raw).unwrap();
    v["version"] = serde_json::json!(1);
    assert!(matches!(
        Header::from_json(&serde_json::to_vec(&v).unwrap(), binding(1)),
        Err(Error::Unsupported)
    ));
    v["version"] = serde_json::json!(2);
    v["unexpected"] = serde_json::json!("synthetic-secret-error-canary");
    let error = match Header::from_json(&serde_json::to_vec(&v).unwrap(), binding(1)) {
        Err(e) => e,
        Ok(_) => panic!("unknown fields accepted"),
    };
    assert_eq!(error.to_string(), "vault_invalid_input");
    assert!(!format!("{error:?}").contains("canary"));
    assert!(matches!(
        PendingVault::prepare(binding(1), &text(&"x".repeat(1025)), &mut rng),
        Err(Error::Invalid)
    ));
    assert!(matches!(
        EncryptedItem::from_json(&vec![0; 100_001]),
        Err(Error::Limit)
    ));
    assert!(unb64("YQ", 1, 1).is_err());
}
#[test]
fn password_rewrap_retains_items_and_recovery_while_pending_drop_keeps_old_epoch() {
    let mut rng = Synthetic(0);
    let (mut a, recovery) = ready(1, &mut rng);
    let item = a
        .seal_item(Uuid::from_u128(500), 1, &fields(), &mut rng)
        .unwrap();
    let old_header = a.header().to_json().unwrap();
    {
        let p = a
            .prepare_password_change(&text("Changed synthetic master"), &mut rng)
            .unwrap();
        assert_eq!(p.header().envelope_epoch, 2);
    }
    assert_eq!(a.header().to_json().unwrap(), old_header);
    let mut pending = a
        .prepare_password_change(&text("Changed synthetic master"), &mut rng)
        .unwrap();
    let (id, digest) = pending.candidate().unwrap();
    assert!(
        pending
            .confirm_commit(StorageReceipt::from_verified_storage(id, [0; 32]))
            .is_err()
    );
    pending
        .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    assert!(pending.activate().unwrap().is_empty());
    let mut changed = unlock_password(
        a.header().clone(),
        binding(1),
        &text("Changed synthetic master"),
    )
    .unwrap();
    assert_eq!(
        changed.open_item(&item).unwrap().secret.expose(),
        "synthetic-password-canary"
    );
    assert!(
        unlock_password(
            a.header().clone(),
            binding(1),
            &text("Synthetic master passphrase")
        )
        .is_err()
    );
    let mut recovered = unlock_recovery(a.header().clone(), binding(1), &recovery).unwrap();
    assert_eq!(
        recovered.open_item(&item).unwrap().url.expose(),
        "https://example.test/account"
    );
    assert_eq!(item.id, Uuid::from_u128(500));
    assert_eq!(item.revision, 1);
}
#[test]
fn recovery_replacement_and_root_rotation_require_new_recovery_and_pinned_inventory() {
    let mut rng = Synthetic(0);
    let (mut a, old_recovery) = ready(1, &mut rng);
    let item = a
        .seal_item(Uuid::from_u128(500), 1, &fields(), &mut rng)
        .unwrap();
    let mut p = a.prepare_recovery_change(&mut rng).unwrap();
    let recovery = RecoveryKey::parse(&p.recovery().unwrap().expose_encoded()).unwrap();
    let (id, digest) = p.candidate().unwrap();
    assert!(
        p.confirm_commit(StorageReceipt::from_verified_storage(id, digest))
            .is_err()
    );
    p.verify_recovery(&recovery).unwrap();
    p.confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    p.activate().unwrap();
    assert!(unlock_recovery(a.header().clone(), binding(1), &old_recovery).is_err());
    assert!(unlock_recovery(a.header().clone(), binding(1), &recovery).is_ok());
    let inventory = inventory_digest(std::slice::from_ref(&item)).unwrap();
    assert!(
        a.prepare_root_rotation(
            &text("Rotated synthetic master"),
            std::slice::from_ref(&item),
            [0; 32],
            &mut rng
        )
        .is_err()
    );
    let old_header = a.header().clone();
    let mut p = a
        .prepare_root_rotation(
            &text("Rotated synthetic master"),
            std::slice::from_ref(&item),
            inventory,
            &mut rng,
        )
        .unwrap();
    let new_recovery = RecoveryKey::parse(&p.recovery().unwrap().expose_encoded()).unwrap();
    p.verify_recovery(&new_recovery).unwrap();
    let (id, digest) = p.candidate().unwrap();
    p.confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    let rotated = p.activate().unwrap();
    assert_eq!(rotated[0].id, item.id);
    assert_eq!(rotated[0].revision, 2);
    assert_eq!(a.header().generation, 2);
    assert!(a.open_item(&item).is_err());
    assert_eq!(
        a.open_item(&rotated[0]).unwrap().secret.expose(),
        "synthetic-password-canary"
    );
    let mut old = unlock_recovery(old_header, binding(1), &recovery).unwrap();
    assert!(old.open_item(&rotated[0]).is_err());
    assert!(inventory_digest(&[rotated[0].clone(), rotated[0].clone()]).is_err());
}
#[test]
fn synthetic_apple_csv_multiline_filter_duplicates_flags_and_stable_encrypted_batch() {
    let mut rng = Synthetic(0);
    let (mut a, _) = ready(1, &mut rng);
    let csv = "\u{feff}\"Title\",\"URL\",\"Username\",\"Password\",\"Notes\",\"OTPAuth\"\r\n\"무수리, fixture\",\"https://example.test/login\",\"chosen-user\",\"p\"\"ass\",\"line1\r\nline2 🌱\",\"otpauth://totp/Fixture?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ\"\r\n\"무수리, fixture\",\"https://example.test/login\",\"chosen-user\",\"p\"\"ass\",\"line1\r\nline2 🌱\",\"otpauth://totp/Fixture?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ\"\r\nOther,https://other.test,other-user,other-secret,,\r\nBlank,javascript:unsupported,chosen-user,,,invalid-seed\r\n";
    let mut import = AppleImport::preview(
        csv.as_bytes(),
        &mut a,
        &[],
        UsernameFilter::Exact(text("chosen-user")),
        &Continue,
    )
    .unwrap();
    assert_eq!(import.counts().rows.len(), 4);
    assert_eq!(import.counts().eligible, 2);
    assert_eq!(import.counts().duplicates, 1);
    assert_eq!(import.counts().filtered, 1);
    assert!(
        import.counts().rows[3].blank_password
            && import.counts().rows[3].unsupported_url
            && import.counts().rows[3].invalid_otp
    );
    let batch = import
        .seal_selected(&mut a, &[2, 5], &mut rng, &Continue)
        .unwrap()
        .to_vec();
    let same = import
        .seal_selected(&mut a, &[2, 5], &mut rng, &Continue)
        .unwrap();
    assert_eq!(batch[0].to_json().unwrap(), same[0].to_json().unwrap());
    assert!(matches!(
        import.seal_selected(&mut a, &[5, 2], &mut rng, &Continue),
        Err(Error::Conflict)
    ));
    let first = a.open_item(&batch[0]).unwrap();
    assert_eq!(first.title.expose(), "무수리, fixture");
    assert_eq!(first.secret.expose(), "p\"ass");
    assert_eq!(first.notes.expose(), "line1\r\nline2 🌱");
    assert_eq!(
        a.open_item(&batch[1]).unwrap().totp.expose(),
        "invalid-seed"
    );
    let check = AppleImport::preview(
        csv.as_bytes(),
        &mut a,
        &batch,
        UsernameFilter::All,
        &Continue,
    )
    .unwrap();
    assert_eq!(check.counts().duplicates, 3);
    assert_eq!(check.counts().eligible, 1);
}
#[test]
fn hostile_csv_bounds_quoting_encoding_cancellation_and_generic_empty_username_filter() {
    let mut rng = Synthetic(0);
    let (mut a, _) = ready(1, &mut rng);
    for raw in [
        b"Title,URL,Username,Password\na,b,c,\"unterminated".as_slice(),
        b"Title,URL,Username,Password\na,b,c,plain\"quote",
        b"Title,URL,Username,Password\na,b,c,\"quoted\"junk",
        b"Title,URL,Username,Password\na,b,c,\xff",
        b"Title,URL,Username,Password\na,b,c,pass,extra",
        b"Title,URL,Username,Password,Unknown\na,b,c,pass,private",
        b"Title,URL,Username,Username\na,b,c,d",
    ] {
        assert!(AppleImport::preview(raw, &mut a, &[], UsernameFilter::All, &Continue).is_err());
    }
    let huge = format!("Title,URL,Username,Password\na,b,c,{}", "p".repeat(65_537));
    assert!(matches!(
        AppleImport::preview(huge.as_bytes(), &mut a, &[], UsernameFilter::All, &Continue),
        Err(Error::Limit)
    ));
    let many = format!("Title,URL,Username,Password\n{}", "a,b,c,p\n".repeat(1001));
    assert!(matches!(
        AppleImport::preview(many.as_bytes(), &mut a, &[], UsernameFilter::All, &Continue),
        Err(Error::Limit)
    ));
    struct Stop;
    impl Cancellation for Stop {
        fn cancelled(&self) -> bool {
            true
        }
    }
    assert!(matches!(
        AppleImport::preview(
            b"Title,URL,Username,Password".as_slice(),
            &mut a,
            &[],
            UsernameFilter::All,
            &Stop
        ),
        Err(Error::Cancelled)
    ));
    let blank = "Title,URL,Username,Password\nblank,https://example.test,,\nother,https://example.test,other,secret\n";
    let p = AppleImport::preview(
        blank.as_bytes(),
        &mut a,
        &[],
        UsernameFilter::Exact(text("")),
        &Continue,
    )
    .unwrap();
    assert_eq!(p.counts().eligible, 1);
    assert_eq!(p.counts().filtered, 1);
    assert!(p.counts().rows[0].blank_password);
}

#[test]
fn entropy_collision_failure_and_streaming_csv_caps_cancel_and_stale_epoch() {
    let mut rng = Synthetic(0);
    let (mut vault, _) = ready(1, &mut rng);
    struct Failed;
    impl TrustedEntropy for Failed {
        fn fill(&mut self, _: &mut [u8]) -> Result<()> {
            Err(Error::Entropy)
        }
    }
    assert!(matches!(
        vault.seal_item(Uuid::from_u128(500), 1, &fields(), &mut Failed),
        Err(Error::Entropy)
    ));
    struct Zeroes;
    impl TrustedEntropy for Zeroes {
        fn fill(&mut self, out: &mut [u8]) -> Result<()> {
            out.fill(0);
            Ok(())
        }
    }
    vault
        .seal_item(Uuid::from_u128(500), 1, &fields(), &mut Zeroes)
        .unwrap();
    assert!(matches!(
        vault.seal_item(Uuid::from_u128(501), 1, &fields(), &mut Zeroes),
        Err(Error::Entropy)
    ));
    let blanks = format!("Title,URL,Username,Password\n{}", "\n".repeat(10_001));
    assert!(matches!(
        AppleImport::preview(
            blanks.as_bytes(),
            &mut vault,
            &[],
            UsernameFilter::All,
            &Continue
        ),
        Err(Error::Limit)
    ));
    let row = format!(
        "fixture,https://example.test,user,p,{}\n",
        "n".repeat(30_000)
    );
    let large = format!("Title,URL,Username,Password,Notes\n{}", row.repeat(360));
    assert!(matches!(
        AppleImport::preview(
            large.as_bytes(),
            &mut vault,
            &[],
            UsernameFilter::All,
            &Continue
        ),
        Err(Error::Limit)
    ));
    struct OneByte<'a>(&'a [u8]);
    impl std::io::Read for OneByte<'_> {
        fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            out[0] = self.0[0];
            self.0 = &self.0[1..];
            Ok(1)
        }
    }
    let csv = "\u{feff}Title,URL,Username,Password,Notes\r\n\"quoted, title\",https://example.test,user,secret,\"line1\r\nline2\"\r\nsecond,https://other.test,user,secret,notes\r\n";
    let mut import = AppleImport::preview(
        OneByte(csv.as_bytes()),
        &mut vault,
        &[],
        UsernameFilter::All,
        &Continue,
    )
    .unwrap();
    assert_eq!(import.counts().eligible, 2);
    struct MidCancel(std::cell::Cell<u32>);
    impl Cancellation for MidCancel {
        fn cancelled(&self) -> bool {
            let n = self.0.get();
            self.0.set(n + 1);
            n >= 2
        }
    }
    assert!(matches!(
        import.seal_selected(
            &mut vault,
            &[2, 3],
            &mut rng,
            &MidCancel(std::cell::Cell::new(0))
        ),
        Err(Error::Cancelled)
    ));
    let batch = import
        .seal_selected(&mut vault, &[2, 3], &mut rng, &Continue)
        .unwrap()
        .to_vec();
    assert_eq!(batch.len(), 2);
    assert_eq!(
        vault.open_item(&batch[0]).unwrap().notes.expose(),
        "line1\r\nline2"
    );
    let mut fresh = AppleImport::preview(
        csv.as_bytes(),
        &mut vault,
        &[],
        UsernameFilter::All,
        &Continue,
    )
    .unwrap();
    let mut pending = vault
        .prepare_password_change(&text("Changed synthetic master"), &mut rng)
        .unwrap();
    let (id, digest) = pending.candidate().unwrap();
    pending
        .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    pending.activate().unwrap();
    assert!(matches!(
        fresh.seal_selected(&mut vault, &[2], &mut rng, &Continue),
        Err(Error::Conflict)
    ));
}

fn synthetic_committed(header: Header) -> Result<CommittedHeader> {
    let expected = header.binding;
    let digest = Sha256::digest(header.to_json()?).into();
    CommittedHeader::admit(
        header,
        expected,
        HeaderReceipt::from_verified_current_storage(expected, digest),
    )
}
fn unlock_password(
    header: Header,
    expected: Binding,
    password: &SecretText,
) -> Result<ActiveVault> {
    crate::ActiveVault::unlock_password(synthetic_committed(header)?, expected, password)
}
fn unlock_recovery(
    header: Header,
    expected: Binding,
    recovery: &RecoveryKey,
) -> Result<ActiveVault> {
    crate::ActiveVault::unlock_recovery(synthetic_committed(header)?, expected, recovery)
}
#[test]
fn committed_header_admission_rejects_candidate_and_utf8_password_is_not_normalized() {
    let mut rng = Synthetic(0);
    let (mut a, _) = ready(1, &mut rng);
    let digest = Sha256::digest(a.header().to_json().unwrap()).into();
    let receipt = HeaderReceipt::from_verified_current_storage(binding(1), digest);
    let p = a
        .prepare_password_change(&text("Changed synthetic master"), &mut rng)
        .unwrap();
    assert!(matches!(
        CommittedHeader::admit(p.header().clone(), binding(1), receipt),
        Err(Error::State)
    ));
    drop(p);
    let pending = PendingVault::prepare(binding(2), &text("Synthetic é master"), &mut rng).unwrap();
    let h = pending.header().clone();
    assert!(unlock_password(h.clone(), binding(2), &text("Synthetic e\u{301} master")).is_err());
    assert!(unlock_password(h, binding(2), &text("Synthetic é master")).is_ok());
}
