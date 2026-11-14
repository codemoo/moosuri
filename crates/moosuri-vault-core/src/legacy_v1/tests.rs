use super::*;
use crate::crypto::encrypt;
use serde_json::{Value, json as value};
const VECTOR: &[u8] = include_bytes!("../../tests/fixtures/legacy-v1-webcrypto.json");
fn meta() -> Value {
    serde_json::from_str(include_str!(
        "../../tests/fixtures/legacy-v1-vector-meta.json"
    ))
    .unwrap()
}
fn secret(s: &str) -> SecretText {
    SecretText::new(s.into()).unwrap()
}
fn destination() -> ActiveVault {
    let binding = Binding {
        owner_id: Uuid::from_u128(801),
        tenant_id: Uuid::from_u128(802),
        vault_id: Uuid::from_u128(803),
    };
    let mut p = PendingVault::prepare(
        binding,
        &secret("SYNTHETIC new v2 destination password"),
        &mut OsEntropy,
    )
    .unwrap();
    p.verify_recovery(&RecoveryKey::parse(&p.recovery().expose_encoded()).unwrap())
        .unwrap();
    let (id, digest) = p.candidate().unwrap();
    p.confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    p.activate().unwrap()
}
fn root() -> Key {
    let raw = unb64(meta()["root"].as_str().unwrap(), 32, 32).unwrap();
    let mut key = Zeroizing::new([0; 32]);
    key.copy_from_slice(&raw);
    Key(key)
}
fn manifest_key(a: &Archive) -> Key {
    let mut key = Zeroizing::new([0; 32]);
    Hkdf::<Sha256>::new(
        Some(&unb64(&a.manifest.nonce, 12, 12).unwrap()),
        root().0.as_ref(),
    )
    .expand(
        format!(
            "musuri-vault/v1/archive-key/{}/{}",
            a.header.id, a.header.generation
        )
        .as_bytes(),
        key.as_mut(),
    )
    .unwrap();
    Key(key)
}
fn manifest_plain(a: &Archive) -> Vec<u8> {
    json(
        &Manifest {
            count: a.items.len(),
            digest: b64(&Sha256::digest(
                json(
                    &Inventory {
                        header: &a.header,
                        items: &a.items,
                    },
                    ARCHIVE_LIMIT,
                )
                .unwrap(),
            )),
            ids: a.items.iter().map(|i| i.id).collect(),
            version: 1,
        },
        262128,
    )
    .unwrap()
}
fn retag(a: &mut Archive) {
    // Public synthetic key only; a fresh manifest nonce avoids even test nonce reuse.
    a.manifest.nonce = b64(&[9; 12]);
    a.manifest.ciphertext = encrypt(
        &manifest_key(a),
        &[9; 12],
        &json(
            &ArchiveAad {
                domain: "musuri-vault/v1/archive",
                header: &a.header,
                version: 1,
            },
            16384,
        )
        .unwrap(),
        &manifest_plain(a),
    )
    .unwrap();
}
#[test]
fn legacy_v1_cross_runtime_exact_wire_password_recovery_and_local_reencryption() {
    let metadata = meta();
    let a: Archive = serde_json::from_slice(VECTOR).unwrap();
    a.validate(&Continue).unwrap();
    // Rust reproduces the independent WebCrypto/libsodium producer's entire wire,
    // including KDF output, canonical AAD, wrapped roots/keys and both payloads.
    for e in &a.header.envelopes {
        let key = match e.kind {
            EnvelopeKind::Password => password_key(
                &secret(metadata["password"].as_str().unwrap()),
                &unb64(&e.salt, 16, 16).unwrap(),
                &PasswordKdf::default(),
            )
            .unwrap(),
            EnvelopeKind::Recovery => {
                use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
                let raw = URL_SAFE_NO_PAD
                    .decode(
                        metadata["recovery"]
                            .as_str()
                            .unwrap()
                            .strip_prefix("MR1-")
                            .unwrap(),
                    )
                    .unwrap();
                let mut key = Zeroizing::new([0; 32]);
                Hkdf::<Sha256>::new(Some(&unb64(&e.salt, 16, 16).unwrap()), &raw)
                    .expand(
                        format!("musuri-vault/v1/recovery/{}/{}", a.header.id, e.id).as_bytes(),
                        key.as_mut(),
                    )
                    .unwrap();
                Key(key)
            }
        };
        let aad = json(
            &EnvelopeAad {
                domain: "musuri-vault/v1/envelope",
                envelope: EnvelopeMeta {
                    created_version: e.created_version,
                    generation: e.generation,
                    id: e.id,
                    kdf: &e.kdf,
                    kind: e.kind,
                    salt: &e.salt,
                },
                vault_id: a.header.id,
            },
            4096,
        )
        .unwrap();
        assert_eq!(
            encrypt(
                &key,
                &unb64(&e.nonce, 12, 12).unwrap(),
                &aad,
                root().0.as_ref()
            )
            .unwrap(),
            e.ciphertext
        );
    }
    for (n, item) in a.items.iter().enumerate() {
        let raw = unb64(metadata["item_keys"][n].as_str().unwrap(), 32, 32).unwrap();
        let mut key = Zeroizing::new([0; 32]);
        key.copy_from_slice(&raw);
        let key = Key(key);
        assert_eq!(
            encrypt(
                &root(),
                &unb64(&item.key_nonce, 12, 12).unwrap(),
                &item_aad(&a.header, item, true).unwrap(),
                key.0.as_ref()
            )
            .unwrap(),
            item.wrapped_key
        );
        assert_eq!(
            encrypt(
                &key,
                &unb64(&item.nonce, 12, 12).unwrap(),
                &item_aad(&a.header, item, false).unwrap(),
                metadata["plaintext_json"][n].as_str().unwrap().as_bytes()
            )
            .unwrap(),
            item.ciphertext
        );
    }
    assert_eq!(
        encrypt(
            &manifest_key(&a),
            &unb64(&a.manifest.nonce, 12, 12).unwrap(),
            &json(
                &ArchiveAad {
                    domain: "musuri-vault/v1/archive",
                    header: &a.header,
                    version: 1
                },
                16384
            )
            .unwrap(),
            &manifest_plain(&a)
        )
        .unwrap(),
        a.manifest.ciphertext
    );
    assert_eq!(
        json(&a, ARCHIVE_LIMIT).unwrap(),
        VECTOR.strip_suffix(b"\n").unwrap()
    );
    let mut current = destination();
    for recovery in [false, true] {
        let source = secret(
            metadata[if recovery { "recovery" } else { "password" }]
                .as_str()
                .unwrap(),
        );
        let mut import = ArchiveImport::preview_legacy_v1(
            VECTOR,
            &source,
            recovery,
            &mut current,
            &[],
            &Continue,
        )
        .unwrap();
        assert_eq!(import.counts().eligible, 2);
        let items = import
            .seal_selected(&mut current, &[1, 2], &mut OsEntropy, &Continue)
            .unwrap()
            .to_vec();
        assert_eq!(
            serde_json::to_vec(
                import
                    .seal_selected(&mut current, &[1, 2], &mut OsEntropy, &Continue)
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_vec(&items).unwrap()
        );
        assert!(
            import
                .seal_selected(&mut current, &[1], &mut OsEntropy, &Continue)
                .is_err()
        );
        for (n, item) in items.iter().enumerate() {
            assert_eq!(item.version, 2);
            assert_eq!(item.revision, 1);
            assert!(a.items.iter().all(|old| old.id != item.id));
            let f = current.open_item(item).unwrap();
            let old = &metadata["fields"][n];
            for (value, key) in [
                (&f.title, "title"),
                (&f.username, "username"),
                (&f.url, "url"),
                (&f.secret, "secret"),
                (&f.notes, "notes"),
                (&f.expires_at, "expires_at"),
            ] {
                assert_eq!(value.expose(), old[key].as_str().unwrap());
            }
            assert_eq!(f.favorite, old["favorite"].as_bool().unwrap());
            assert_eq!(f.totp.expose(), old["totp"].as_str().unwrap_or(""));
        }
        let duplicate = ArchiveImport::preview_legacy_v1(
            VECTOR,
            &source,
            recovery,
            &mut current,
            &items,
            &Continue,
        )
        .unwrap();
        assert_eq!(duplicate.counts().duplicates, 2);
    }
}
#[test]
fn legacy_v1_hostile_structure_is_rejected_before_root_kdf_and_raw_dumps_are_unsupported() {
    let mut current = destination();
    let original: Value = serde_json::from_slice(VECTOR).unwrap();
    let mut cases = vec![];
    let mut bad = original.clone();
    bad["header"]["envelopes"][1]["kdf"]["memory"] = value!(2147483648u64);
    cases.push(serde_json::to_vec(&bad).unwrap());
    let mut bad = original.clone();
    bad["items"] = value!(vec![original["items"][0].clone(); 257]);
    cases.push(serde_json::to_vec(&bad).unwrap());
    let mut bad = original.clone();
    bad["header"]["id"] = value!("10000000-0000-4000-A000-000000000001");
    cases.push(serde_json::to_vec(&bad).unwrap());
    let mut bad = original.clone();
    bad["header"]["generation"] = value!(9007199254740992u64);
    cases.push(serde_json::to_vec(&bad).unwrap());
    let mut bad = original.clone();
    bad["items"][1]["key_nonce"] = bad["items"][0]["key_nonce"].clone();
    cases.push(serde_json::to_vec(&bad).unwrap());
    let mut bad = original.clone();
    bad["items"][0]["wrapped_key"] = value!("A");
    cases.push(serde_json::to_vec(&bad).unwrap());
    let mut bad = original.clone();
    bad.as_object_mut().unwrap().remove("manifest");
    cases.push(serde_json::to_vec(&bad).unwrap());
    let mut bad = original;
    bad["unknown"] = value!(true);
    cases.push(serde_json::to_vec(&bad).unwrap());
    cases.push(
        std::str::from_utf8(VECTOR)
            .unwrap()
            .replacen("\"format\":", "\"format\":\"musuri-vault\",\"format\":", 1)
            .into_bytes(),
    );
    cases.push(VECTOR[..VECTOR.len() - 10].to_vec());
    cases.push(vec![b' '; ARCHIVE_LIMIT + 1]);
    for bytes in cases {
        ROOT_OPENS.with(|n| n.set(0));
        assert!(
            ArchiveImport::preview_legacy_v1(
                &bytes,
                &secret("invalid"),
                true,
                &mut current,
                &[],
                &Continue
            )
            .is_err()
        );
        assert_eq!(ROOT_OPENS.with(|n| n.get()), 0);
    }
}
#[test]
fn legacy_v1_authentication_all_payloads_cancel_and_no_legacy_fallback() {
    let metadata = meta();
    let source = secret(metadata["recovery"].as_str().unwrap());
    let mut current = destination();
    assert!(
        ArchiveImport::preview_legacy_v1(
            VECTOR,
            &secret("SYNTHETIC wrong password"),
            false,
            &mut current,
            &[],
            &Continue
        )
        .is_err()
    );
    assert!(ArchiveImport::preview(VECTOR, &source, true, &mut current, &[], &Continue).is_err());
    let mut a: Archive = serde_json::from_slice(VECTOR).unwrap();
    a.items.reverse();
    assert!(
        ArchiveImport::preview_legacy_v1(
            &json(&a, ARCHIVE_LIMIT).unwrap(),
            &source,
            true,
            &mut current,
            &[],
            &Continue
        )
        .is_err()
    );
    let mut a: Archive = serde_json::from_slice(VECTOR).unwrap();
    a.items.pop();
    assert!(
        ArchiveImport::preview_legacy_v1(
            &json(&a, ARCHIVE_LIMIT).unwrap(),
            &source,
            true,
            &mut current,
            &[],
            &Continue
        )
        .is_err()
    );
    let mut a: Archive = serde_json::from_slice(VECTOR).unwrap();
    let mut bytes = unb64(&a.items[1].ciphertext, 16, 65552).unwrap();
    bytes[0] ^= 1;
    a.items[1].ciphertext = b64(&bytes);
    retag(&mut a);
    assert!(
        ArchiveImport::preview_legacy_v1(
            &json(&a, ARCHIVE_LIMIT).unwrap(),
            &source,
            true,
            &mut current,
            &[],
            &Continue
        )
        .is_err(),
        "valid manifest with corrupt final payload returns no preview"
    );
    struct Stop;
    impl Cancellation for Stop {
        fn cancelled(&self) -> bool {
            true
        }
    }
    ROOT_OPENS.with(|n| n.set(0));
    assert!(matches!(
        ArchiveImport::preview_legacy_v1(VECTOR, &source, true, &mut current, &[], &Stop),
        Err(Error::Cancelled)
    ));
    assert_eq!(ROOT_OPENS.with(|n| n.get()), 0);
}
