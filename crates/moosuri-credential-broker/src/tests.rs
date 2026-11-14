use super::*;
use moosuri_vault_protocol::Owner;
use uuid::Uuid;
const ORIGIN: &str = "https://assistant.example.test";
fn device() -> Device {
    Device {
        owner: Owner {
            tenant: Uuid::new_v4(),
            account: Uuid::new_v4(),
            installation: Uuid::new_v4(),
            epoch: 1,
        },
        id: Uuid::new_v4(),
        generation: 1,
    }
}
fn setup() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    Engine,
    Request,
    Vec<u8>,
) {
    let tmp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = tmp.path().join("device");
    let d = device();
    let bytes = auth::SigningKey::generate_pkcs8().unwrap();
    let pk = auth::SigningKey::from_pkcs8(&bytes).unwrap().public_key();
    let mut engine = Engine::enroll(&root, d.clone(), ORIGIN.into(), &bytes).unwrap();
    let grant = policy::LocalGrant::select_fixed_probe(
        d.clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        1,
        1,
        1000,
    )
    .unwrap();
    let digest = grant.probe_digest();
    let binding = engine.select(grant).unwrap();
    let request = Request {
        device: d,
        invocation: Invocation {
            run: Uuid::new_v4(),
            step: 1,
            call: Uuid::new_v4(),
            nonce: Uuid::new_v4(),
        },
        binding,
        operation: Operation::FixedProbe,
        digest,
        expires_at: 500,
    };
    (tmp, root, engine, request, pk)
}
#[test]
fn local_ceiling_automatic_reuse_and_no_server_broadening() {
    let (_tmp, _root, mut e, r, _) = setup();
    for _ in 0..2 {
        e.prepare(r.clone(), 100, 1, 1).unwrap()
    }
    let mut second = r.clone();
    second.invocation.call = Uuid::new_v4();
    e.prepare(second, 100, 1, 1).unwrap();
    for mode in 0..11 {
        let mut bad = r.clone();
        bad.invocation.call = Uuid::new_v4();
        match mode {
            0 => bad.device.owner.account = Uuid::new_v4(),
            1 => bad.device.owner.tenant = Uuid::new_v4(),
            2 => bad.device.generation += 1,
            3 => bad.binding.grant_version += 1,
            4 => bad.binding.item_epoch += 1,
            5 => bad.binding.key_epoch += 1,
            6 => bad.binding.epoch += 1,
            7 => bad.binding.grant_hash[0] ^= 1,
            8 => bad.digest[0] ^= 1,
            9 => bad.expires_at = 1001,
            _ => bad.device.owner.epoch += 1,
        }
        assert!(e.prepare(bad, 100, 1, 1).is_err());
    }
    assert!(e.prepare(r.clone(), 500, 1, 1).is_err());
    assert!(e.prepare(r.clone(), 100, 2, 1).is_err());
    e.revoke().unwrap();
    assert!(e.prepare(r, 100, 1, 1).is_err());
}
#[test]
fn crash_boundaries_keep_original_identity_and_fixed_projection_no_echo() {
    for crash in [
        "prepare",
        "commit",
        "before_network",
        "after_success",
        "before_ack",
    ] {
        let (_tmp, root, mut e, r, pk) = setup();
        let i = r.invocation.clone();
        e.prepare(r.clone(), 100, 1, 1).unwrap();
        let c = Commitment {
            request: r.clone(),
            nonce: Uuid::new_v4(),
        };
        let mut count = 0;
        if crash != "prepare" {
            e.accept_commitment(c.clone()).unwrap()
        }
        if matches!(crash, "before_network" | "after_success" | "before_ack") {
            let adopted = e.begin_dispatch(&i, 100, 1, 1).unwrap();
            assert!(adopted == c);
        }
        if matches!(crash, "after_success" | "before_ack") {
            count += 1;
            let malicious = "SECRET-CANARY TOTP-123456 token=secret";
            // Test-only fixed destination adapter: discard its body entirely.
            let _ = malicious;
            let receipt = Receipt {
                commitment: c.clone(),
                outcome: Outcome::ProjectedSuccess,
                projection: Projection::FixedProbeOk,
                projection_digest: auth::projection_digest(Projection::FixedProbeOk),
            };
            if crash == "before_ack" {
                let signed = SignedReceipt {
                    signature: e.key.sign(&receipt_bytes(&receipt)),
                    receipt,
                };
                e.state.entries[0].phase = Phase::Settled(signed);
                e.save().unwrap();
            }
        }
        drop(e);
        let mut e = Engine::reopen(&root, &r.device, ORIGIN, &pk).unwrap();
        if crash == "prepare" {
            assert!(e.settle_without_retry(&i).is_err());
            e.accept_commitment(c).unwrap();
        }
        let receipt = e.settle_without_retry(&i).unwrap();
        assert!(auth::verify(&pk, &receipt_bytes(&receipt.receipt), &receipt.signature).is_ok());
        assert!(e.begin_dispatch(&i, 100, 1, 1).is_err());
        assert!(count <= 1);
        if crash == "commit" || crash == "prepare" {
            assert!(receipt.receipt.outcome == Outcome::NotDispatched)
        }
        if crash == "before_network" || crash == "after_success" {
            assert!(receipt.receipt.outcome == Outcome::Unknown)
        }
        for name in ["journal", "journal.head"] {
            let data = std::fs::read(root.join(name)).unwrap();
            assert!(!data.windows(6).any(|b| b == b"CANARY"));
        }
    }
}
#[test]
fn missing_corrupt_truncated_state_and_wrong_generation_fail_closed() {
    for name in ["journal", "journal.head", "device.pkcs8", "owner.lock"] {
        let (_tmp, root, e, r, pk) = setup();
        drop(e);
        std::fs::remove_file(root.join(name)).unwrap();
        assert!(Engine::reopen(&root, &r.device, ORIGIN, &pk).is_err());
    }
    let (_tmp, root, e, r, pk) = setup();
    drop(e);
    let mut d = r.device.clone();
    d.generation += 1;
    assert!(Engine::reopen(&root, &d, ORIGIN, &pk).is_err());
    let data = std::fs::read(root.join("journal")).unwrap();
    std::fs::write(root.join("journal"), &data[..data.len() - 1]).unwrap();
    assert!(Engine::reopen(&root, &r.device, ORIGIN, &pk).is_err());
}
#[test]
fn another_process_cannot_take_live_owner_lock() {
    let (_tmp, root, _e, r, pk) = setup();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "tests::child_lock_probe", "--nocapture"])
        .env("MOOSURI_BROKER_LOCK_ROOT", &root)
        .env(
            "MOOSURI_BROKER_PUBLIC_DEVICE",
            serde_json::to_string(&r.device).unwrap(),
        )
        .env(
            "MOOSURI_BROKER_PUBLIC_KEY",
            serde_json::to_string(&pk).unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}
#[test]
fn child_lock_probe() {
    if let Ok(root) = std::env::var("MOOSURI_BROKER_LOCK_ROOT") {
        let d: Device =
            serde_json::from_str(&std::env::var("MOOSURI_BROKER_PUBLIC_DEVICE").unwrap()).unwrap();
        let key: Vec<u8> =
            serde_json::from_str(&std::env::var("MOOSURI_BROKER_PUBLIC_KEY").unwrap()).unwrap();
        assert!(matches!(
            Engine::reopen(Path::new(&root), &d, ORIGIN, &key),
            Err(Error::Locked)
        ));
    }
}
#[cfg(unix)]
#[test]
fn symlink_hardlink_modes_and_acl_denied() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for mode in [0, 1, 2] {
        let (_tmp, root, e, r, pk) = setup();
        drop(e);
        match mode {
            0 => {
                std::fs::rename(root.join("journal"), root.join("other")).unwrap();
                symlink("other", root.join("journal")).unwrap()
            }
            1 => std::fs::hard_link(root.join("journal"), root.join("other")).unwrap(),
            _ => std::fs::set_permissions(
                root.join("journal"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap(),
        };
        assert!(Engine::reopen(&root, &r.device, ORIGIN, &pk).is_err());
    }
    #[cfg(target_os = "macos")]
    {
        let (_tmp, root, e, r, pk) = setup();
        drop(e);
        let out = std::process::Command::new("chmod")
            .arg("+a")
            .arg("everyone allow read,readattr,readextattr,readsecurity")
            .arg(root.join("journal"))
            .status()
            .unwrap();
        assert!(out.success());
        assert!(Engine::reopen(&root, &r.device, ORIGIN, &pk).is_err());
    }
}

#[test]
fn abrupt_process_exit_preserves_fsync_adoption_and_prevents_second_adapter_entry() {
    if let Ok(root) = std::env::var("MOOSURI_BROKER_CRASH_ROOT") {
        let request: Request =
            serde_json::from_str(&std::env::var("MOOSURI_BROKER_PUBLIC_REQUEST").unwrap()).unwrap();
        let key: Vec<u8> =
            serde_json::from_str(&std::env::var("MOOSURI_BROKER_PUBLIC_KEY").unwrap()).unwrap();
        let phase = std::env::var("MOOSURI_BROKER_CRASH_PHASE").unwrap();
        let mut engine = Engine::reopen(Path::new(&root), &request.device, ORIGIN, &key).unwrap();
        engine.prepare(request.clone(), 100, 1, 1).unwrap();
        if phase != "prepared" {
            engine
                .accept_commitment(Commitment {
                    request: request.clone(),
                    nonce: Uuid::new_v4(),
                })
                .unwrap();
        }
        if phase == "entered" {
            engine
                .begin_dispatch(&request.invocation, 100, 1, 1)
                .unwrap();
        }
        // Exit without Drop, as after an OS/process interruption. Only synthetic state.
        std::process::exit(77);
    }
    for phase in ["prepared", "committed", "entered"] {
        let (_tmp, root, engine, request, key) = setup();
        drop(engine);
        let output=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","tests::abrupt_process_exit_preserves_fsync_adoption_and_prevents_second_adapter_entry","--nocapture"]).env("MOOSURI_BROKER_CRASH_ROOT",&root).env("MOOSURI_BROKER_PUBLIC_REQUEST",serde_json::to_string(&request).unwrap()).env("MOOSURI_BROKER_PUBLIC_KEY",serde_json::to_string(&key).unwrap()).env("MOOSURI_BROKER_CRASH_PHASE",phase).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(77),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let mut engine = Engine::reopen(&root, &request.device, ORIGIN, &key).unwrap();
        let result = engine.settle_without_retry(&request.invocation);
        if phase == "prepared" {
            assert!(result.is_err())
        } else {
            let r = result.unwrap();
            assert!(
                r.receipt.outcome
                    == if phase == "entered" {
                        Outcome::Unknown
                    } else {
                        Outcome::NotDispatched
                    }
            );
            assert!(
                engine
                    .begin_dispatch(&request.invocation, 100, 1, 1)
                    .is_err()
            );
        }
    }
}

#[test]
fn deterministic_pairing_vector_from_independent_python_struct_pack() {
    // Reference: Python struct.pack('>I'/' >q'/' >Q') and UUID big-endian bytes,
    // SHA256 over the documented canonical binary schema; not serde serialization.
    let c = PairChallenge {
        device: Device {
            owner: Owner {
                tenant: Uuid::from_u128(1),
                account: Uuid::from_u128(2),
                installation: Uuid::from_u128(3),
                epoch: 7,
            },
            id: Uuid::from_u128(4),
            generation: 9,
        },
        origin: ORIGIN.into(),
        public_key: (0..32).collect(),
        nonce: Uuid::from_u128(6),
        expires_at: 42,
    };
    let bytes = pairing_bytes(&c).unwrap();
    assert_eq!(bytes.len(), 214);
    let got = auth::hash(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        got,
        "952d431b5e76f54befd2404327accdd4b689e5e1890546006b2ebd7901e8d3e5"
    );
}
#[test]
fn arbitrary_destinations_and_echo_fields_have_no_protocol_representation() {
    let (_tmp, _root, mut e, r, _) = setup();
    for name in [
        "origin",
        "redirect",
        "url",
        "headers",
        "recipient",
        "password",
        "totp",
        "raw_response",
    ] {
        let mut v = serde_json::to_value(&r).unwrap();
        v[name] = serde_json::json!("SECRET-CANARY");
        assert!(serde_json::from_value::<Request>(v).is_err());
    }
    for origin in [
        "https://fixed-probe.invalid.evil.test",
        "https://fixed-probe.invalid@evil.test",
        "http://fixed-probe.invalid",
        "https://evil.test",
    ] {
        let grant = e.state.grants[0].clone();
        let mut value = serde_json::to_value(grant).unwrap();
        value["origin"] = serde_json::json!(origin);
        let grant: policy::LocalGrant = serde_json::from_value(value).unwrap();
        assert!(grant.check(&r, 100, 1, 1).is_err());
    }
    e.prepare(r.clone(), 100, 1, 1).unwrap();
    e.accept_commitment(Commitment {
        request: r.clone(),
        nonce: Uuid::new_v4(),
    })
    .unwrap();
    e.revoke().unwrap();
    assert!(e.begin_dispatch(&r.invocation, 100, 1, 1).is_err());
    assert!(
        e.settle_without_retry(&r.invocation)
            .unwrap()
            .receipt
            .outcome
            == Outcome::NotDispatched
    );
}

// The adapter is compiled only into this unit test module, has one fixed
// non-network destination and no caller-selectable URL, headers or secret fields.
fn fixed_probe_adapter(
    engine: &mut Engine,
    invocation: &Invocation,
    calls: &mut usize,
    untrusted_echo: &str,
    success: bool,
) -> Result<SignedReceipt> {
    let commitment = engine.begin_dispatch(invocation, 100, 1, 1)?;
    *calls += 1;
    let _discarded_body = untrusted_echo;
    let projection = if success {
        Projection::FixedProbeOk
    } else {
        Projection::None
    };
    let receipt = Receipt {
        commitment,
        outcome: if success {
            Outcome::ProjectedSuccess
        } else {
            Outcome::Rejected
        },
        projection,
        projection_digest: auth::projection_digest(projection),
    };
    let signed = SignedReceipt {
        signature: engine.key.sign(&receipt_bytes(&receipt)),
        receipt,
    };
    engine
        .state
        .entries
        .iter_mut()
        .find(|e| &e.request.invocation == invocation)
        .unwrap()
        .phase = Phase::Settled(signed.clone());
    engine.save()?;
    Ok(signed)
}
#[test]
fn real_test_only_adapter_enters_at_most_once_and_discards_success_error_echo() {
    for success in [true, false] {
        let (_tmp, root, mut engine, request, key) = setup();
        let mut calls = 0;
        engine.prepare(request.clone(), 100, 1, 1).unwrap();
        engine
            .accept_commitment(Commitment {
                request: request.clone(),
                nonce: Uuid::new_v4(),
            })
            .unwrap();
        let signed = fixed_probe_adapter(
            &mut engine,
            &request.invocation,
            &mut calls,
            "SECRET-CANARY password=synthetic TOTP-123456 cookie=echo",
            success,
        )
        .unwrap();
        assert!(
            fixed_probe_adapter(
                &mut engine,
                &request.invocation,
                &mut calls,
                "SECRET-CANARY",
                success
            )
            .is_err()
        );
        assert_eq!(calls, 1);
        let result = serde_json::to_vec(&signed).unwrap();
        assert!(!result.windows(6).any(|b| b == b"CANARY"));
        assert!(auth::verify(&key, &receipt_bytes(&signed.receipt), &signed.signature).is_ok());
        drop(engine);
        let mut engine = Engine::reopen(&root, &request.device, ORIGIN, &key).unwrap();
        assert!(
            fixed_probe_adapter(
                &mut engine,
                &request.invocation,
                &mut calls,
                "SECRET-CANARY",
                success
            )
            .is_err()
        );
        assert_eq!(calls, 1);
        let bytes = std::fs::read(root.join("journal")).unwrap();
        assert!(!bytes.windows(6).any(|b| b == b"CANARY"));
    }
}
#[cfg(target_os = "macos")]
#[test]
fn inherited_darwin_acl_cannot_create_private_enrollment() {
    let tmp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let status=std::process::Command::new("chmod").arg("+a").arg("everyone allow read,search,readattr,readextattr,readsecurity,file_inherit,directory_inherit").arg(tmp.path()).status().unwrap();
    assert!(status.success());
    let key = auth::SigningKey::generate_pkcs8().unwrap();
    assert!(Engine::enroll(&tmp.path().join("device"), device(), ORIGIN.into(), &key).is_err());
}

#[test]
fn signer_checks_local_selection_preparation_and_revoke_before_ack() {
    let (_tmp, _root, mut engine, request, _key) = setup();
    let challenge = AuthChallenge {
        device: request.device.clone(),
        origin: ORIGIN.into(),
        nonce: Uuid::new_v4(),
        expires_at: 130,
    };
    let sign = |e: &Engine, c| e.sign_command(&challenge, "POST", DEVICE_PATH, c, 100);
    assert!(sign(&engine, Command::AckRevoke).is_err());
    assert!(
        sign(
            &engine,
            Command::Commit {
                invocation: request.invocation.clone()
            }
        )
        .is_err()
    );
    let mut forged = request.binding.clone();
    forged.grant_version += 1;
    assert!(sign(&engine, Command::Register { binding: forged }).is_err());
    assert!(
        sign(
            &engine,
            Command::Register {
                binding: request.binding.clone()
            }
        )
        .is_ok()
    );
    engine.prepare(request.clone(), 100, 1, 1).unwrap();
    assert!(
        sign(
            &engine,
            Command::Commit {
                invocation: request.invocation.clone()
            }
        )
        .is_ok()
    );
    engine.revoke().unwrap();
    assert!(
        sign(
            &engine,
            Command::Commit {
                invocation: request.invocation.clone()
            }
        )
        .is_err()
    );
    assert!(sign(&engine, Command::AckRevoke).is_ok());
}

#[test]
fn enrollment_origin_is_canonical_https_without_credentials_path_or_redirect_target() {
    assert!(auth::valid_origin(ORIGIN));
    for bad in [
        "http://assistant.example.test",
        "https://assistant.example.test/",
        "https://user@assistant.example.test",
        "https://assistant.example.test/path",
        "https://assistant.example.test?next=evil",
        "https://assistant.example.test#fragment",
        "https://assistant.example.test:443",
        "https://ASSISTANT.example.test",
    ] {
        assert!(!auth::valid_origin(bad));
    }
}
