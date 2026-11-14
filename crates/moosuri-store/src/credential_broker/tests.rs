use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};
use moosuri_credential_broker::{
    auth::{SigningKey, authenticated_bytes},
    policy::LocalGrant,
};
use moosuri_vault_protocol::broker::Command;
const ORIGIN: &str = "https://assistant.example.test";
async fn paired(s: &Store, u: &UserContext) -> (Device, SigningKey, Binding, [u8; 32]) {
    let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let c = s
        .broker_pair_challenge(u, ORIGIN.into(), key.public_key())
        .await
        .unwrap();
    let d = s
        .broker_pair(PairProof {
            challenge: c.nonce,
            signature: key.sign(&pairing_bytes(&c).unwrap()),
        })
        .await
        .unwrap();
    let g = LocalGrant::select_fixed_probe(
        d.clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        1,
        1,
        now().unwrap() + 600,
    )
    .unwrap();
    let hash = g.probe_digest();
    assert!(matches!(
        command(
            s,
            &d,
            &key,
            Command::Register {
                binding: g.binding.clone()
            }
        )
        .await
        .unwrap(),
        Reply::Registered
    ));
    (d, key, g.binding, hash)
}
async fn signed(s: &Store, d: &Device, k: &SigningKey, command: Command) -> SignedCommand {
    let c = s.broker_auth_challenge(d.id).await.unwrap();
    SignedCommand {
        challenge: c.nonce,
        signature: k.sign(&authenticated_bytes(&c, "POST", DEVICE_PATH, &command).unwrap()),
        command,
    }
}
async fn command(s: &Store, d: &Device, k: &SigningKey, c: Command) -> Result<Reply> {
    s.broker_command(signed(s, d, k, c).await).await
}
async fn commit(s: &Store, d: &Device, k: &SigningKey, r: &Request) -> Result<Commitment> {
    match command(
        s,
        d,
        k,
        Command::Commit {
            invocation: r.invocation.clone(),
        },
    )
    .await?
    {
        Reply::Commitment { commitment } => Ok(*commitment),
        _ => Err(StoreError::Corrupt),
    }
}
#[tokio::test]
async fn pairing_single_use_wrong_key_helper_expiry_epoch_and_installation_fences() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let a = admin(&s).await.user;
    let b = member(&s, &a, "broker-b@example.test").await.user;
    let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let wrong = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let c = s
        .broker_pair_challenge(&b, ORIGIN.into(), key.public_key())
        .await
        .unwrap();
    assert!(
        s.broker_pair(PairProof {
            challenge: c.nonce,
            signature: wrong.sign(&pairing_bytes(&c).unwrap())
        })
        .await
        .is_err()
    );
    for mutation in 0..5 {
        let mut changed = c.clone();
        match mutation {
            0 => changed.origin = "https://forged.example.test".into(),
            1 => changed.device.owner.account = Uuid::new_v4(),
            2 => changed.device.owner.installation = Uuid::new_v4(),
            3 => changed.device.generation += 1,
            _ => changed.device.owner.tenant = Uuid::new_v4(),
        };
        assert!(
            s.broker_pair(PairProof {
                challenge: c.nonce,
                signature: key.sign(&pairing_bytes(&changed).unwrap())
            })
            .await
            .is_err()
        );
    }
    let invite = s
        .create_user(
            &a,
            "Restricted Broker".into(),
            "restricted-broker@example.test".into(),
        )
        .await
        .unwrap();
    let restricted = s
        .login(
            "restricted-broker@example.test".into(),
            invite.temporary_password,
        )
        .await
        .unwrap()
        .user;
    assert!(
        s.broker_pair_challenge(&restricted, ORIGIN.into(), key.public_key())
            .await
            .is_err()
    );
    let proof = PairProof {
        challenge: c.nonce,
        signature: key.sign(&pairing_bytes(&c).unwrap()),
    };
    let (left, right) = tokio::join!(s.broker_pair(proof.clone()), s.broker_pair(proof));
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let helper = s
        .login_chatgpt_helper("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(
        s.broker_pair_challenge(&helper, ORIGIN.into(), key.public_key())
            .await
            .is_err()
    );
    let c = s
        .broker_pair_challenge(&a, ORIGIN.into(), key.public_key())
        .await
        .unwrap();
    let nonce = c.nonce.to_string();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE credential_broker_pairs SET expires_at=0 WHERE nonce=?1",
            [nonce],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.broker_pair(PairProof {
            challenge: c.nonce,
            signature: key.sign(&pairing_bytes(&c).unwrap())
        })
        .await
        .is_err()
    );
    let c = s
        .broker_pair_challenge(&b, ORIGIN.into(), key.public_key())
        .await
        .unwrap();
    s.change_password(&b, "Synthetic-Broker-New-Password-2026".into())
        .await
        .unwrap();
    assert!(
        s.broker_pair(PairProof {
            challenge: c.nonce,
            signature: key.sign(&pairing_bytes(&c).unwrap())
        })
        .await
        .is_err()
    );
    let c = s
        .broker_pair_challenge(&a, ORIGIN.into(), key.public_key())
        .await
        .unwrap();
    s.call(|db| {
        db.conn.execute(
            "UPDATE backup_installation SET installation_id=?1",
            [Uuid::new_v4().to_string()],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.broker_pair(PairProof {
            challenge: c.nonce,
            signature: key.sign(&pairing_bytes(&c).unwrap())
        })
        .await
        .is_err()
    );
}
#[tokio::test]
async fn signed_method_path_payload_context_and_replay_are_checked() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = admin(&s).await.user;
    let (d, key, b, hash) = paired(&s, &u).await;
    let r = s
        .broker_fixture_probe(&u, d.clone(), b, hash)
        .await
        .unwrap();
    for mutation in 0..6 {
        let mut c = s.broker_auth_challenge(d.id).await.unwrap();
        let cmd = Command::Commit {
            invocation: r.invocation.clone(),
        };
        let (method, path) = match mutation {
            0 => ("GET", DEVICE_PATH),
            1 => ("POST", "/forged"),
            _ => ("POST", DEVICE_PATH),
        };
        match mutation {
            2 => c.device.generation += 1,
            3 => c.device.owner.tenant = Uuid::new_v4(),
            4 => c.origin = "https://lookalike.example.test".into(),
            5 => c.expires_at += 1,
            _ => (),
        };
        let input = SignedCommand {
            challenge: c.nonce,
            signature: key.sign(&authenticated_bytes(&c, method, path, &cmd).unwrap()),
            command: cmd,
        };
        assert!(s.broker_command(input).await.is_err());
    }
    let input = signed(
        &s,
        &d,
        &key,
        Command::Commit {
            invocation: r.invocation.clone(),
        },
    )
    .await;
    assert!(s.broker_command(input.clone()).await.is_ok());
    assert!(s.broker_command(input).await.is_err());
    let mut forged = signed(
        &s,
        &d,
        &key,
        Command::Recover {
            invocation: r.invocation.clone(),
        },
    )
    .await;
    if let Command::Recover { invocation } = &mut forged.command {
        invocation.call = Uuid::new_v4()
    };
    assert!(s.broker_command(forged).await.is_err());
}
#[tokio::test]
async fn writer_revoke_orders_lost_ack_and_original_owner_only_reconciliation() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = admin(&s).await.user;
    // A held signed precheck cannot outrun a completed revoke.
    let (d, k, b, h) = paired(&s, &u).await;
    let r = s.broker_fixture_probe(&u, d.clone(), b, h).await.unwrap();
    let held = signed(
        &s,
        &d,
        &k,
        Command::Commit {
            invocation: r.invocation.clone(),
        },
    )
    .await;
    s.broker_revoke(&u, d.id).await.unwrap();
    assert!(s.broker_command(held).await.is_err());
    // Commit wins first. Dropped reply, revoke, exact recover never mints nonce2.
    let (d, k, b, h) = paired(&s, &u).await;
    let r = s
        .broker_fixture_probe(&u, d.clone(), b.clone(), h)
        .await
        .unwrap();
    let c = commit(&s, &d, &k, &r).await.unwrap();
    s.broker_revoke(&u, d.id).await.unwrap();
    let again = commit(&s, &d, &k, &r).await.unwrap();
    assert!(again == c);
    let fresh = s.broker_fixture_probe(&u, d.clone(), b, h).await.unwrap();
    assert!(commit(&s, &d, &k, &fresh).await.is_err());
    let (other, otherkey, _, _) = paired(&s, &u).await;
    assert!(
        command(
            &s,
            &other,
            &otherkey,
            Command::Recover {
                invocation: r.invocation.clone()
            }
        )
        .await
        .is_err()
    );
    assert!(matches!(
        command(&s, &d, &k, Command::AckRevoke).await.unwrap(),
        Reply::Revoked
    ));
    let id = d.id.to_string();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT status FROM credential_broker_devices WHERE id=?1",
                [id],
                |r| r.get::<_, String>(0)
            )?,
            "revoked"
        );
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn signed_receipts_settle_once_after_cancel_logout_restore_without_resurrection() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = admin(&s).await.user;
    let (d, k, b, h) = paired(&s, &u).await;
    let r = s.broker_fixture_probe(&u, d.clone(), b, h).await.unwrap();
    let c = commit(&s, &d, &k, &r).await.unwrap();
    let receipt = Receipt {
        commitment: c,
        outcome: Outcome::ProjectedSuccess,
        projection: Projection::FixedProbeOk,
        projection_digest: moosuri_credential_broker::auth::projection_digest(
            Projection::FixedProbeOk,
        ),
    };
    let signed = SignedReceipt {
        signature: k.sign(&receipt_bytes(&receipt)),
        receipt,
    };
    for mutation in 0..8 {
        let mut wrong = signed.clone();
        match mutation {
            0 => wrong.receipt.commitment.nonce = Uuid::new_v4(),
            1 => wrong.receipt.commitment.request.digest[0] ^= 1,
            2 => wrong.receipt.commitment.request.device.generation += 1,
            3 => wrong.receipt.commitment.request.binding.grant_version += 1,
            4 => wrong.receipt.projection = Projection::None,
            5 => wrong.receipt.projection_digest[0] ^= 1,
            6 => wrong.receipt.commitment.request.device.owner.account = Uuid::new_v4(),
            _ => wrong.signature[0] ^= 1,
        };
        assert!(
            command(
                &s,
                &d,
                &k,
                Command::Settle {
                    receipt: Box::new(wrong)
                }
            )
            .await
            .is_err()
        );
    }
    let invocation = r.invocation.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE credential_broker_invocations SET cancelled=1 WHERE run_id=?1",
            [invocation.run.to_string()],
        )?;
        quarantine(&db.conn)?;
        db.conn.execute(
            "UPDATE backup_installation SET installation_id=?1",
            [Uuid::new_v4().to_string()],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    s.logout(&u).await.unwrap();
    for _ in 0..2 {
        assert!(matches!(
            command(
                &s,
                &d,
                &k,
                Command::Settle {
                    receipt: Box::new(signed.clone())
                }
            )
            .await
            .unwrap(),
            Reply::Settled
        ));
    }
    let mut changed = signed;
    changed.receipt.outcome = Outcome::Unknown;
    changed.receipt.projection = Projection::None;
    changed.receipt.projection_digest =
        moosuri_credential_broker::auth::projection_digest(Projection::None);
    changed.signature = k.sign(&receipt_bytes(&changed.receipt));
    assert!(
        command(
            &s,
            &d,
            &k,
            Command::Settle {
                receipt: Box::new(changed)
            }
        )
        .await
        .is_err()
    );
    s.call(|db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM credential_broker_commitments WHERE receipt IS NOT NULL",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM credential_broker_invocations WHERE cancelled=0",
                [],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn schema37_late_failure_rolls_back_then_retries_preserving_ciphertext() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    drop(Store::open(dir.path()).unwrap());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch("DROP INDEX local_notifications_source;DROP TRIGGER notification_suppression_monotonic;DROP TABLE notification_source_preferences;ALTER TABLE local_notifications DROP COLUMN source_suppressed;ALTER TABLE local_notifications DROP COLUMN push_suppressed;DROP TABLE learned_skill_receipts;DROP TABLE learned_skill_observations;DROP TABLE learned_skill_uses;DROP TABLE learned_skill_proofs;DROP TABLE learned_skills;DROP TABLE credential_broker_auth;DROP TABLE credential_broker_bindings;DROP TABLE credential_broker_devices;DROP TABLE credential_broker_pairs;DROP TABLE credential_broker_invocations;DROP TABLE credential_broker_commitments;CREATE TABLE credential_broker_commitments(conflict INTEGER);PRAGMA user_version=36;").unwrap();
    drop(conn);
    assert!(Store::open(dir.path()).is_err());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        36
    );
    assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'credential_broker_%'",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'vault_%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    conn.execute_batch("DROP TABLE credential_broker_commitments;")
        .unwrap();
    drop(conn);
    drop(Store::open(dir.path()).unwrap());
}

#[tokio::test]
async fn actual_sealed_restore_quarantines_enrollment_and_preserves_signed_settlement() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let _guard = TEST_LOCK.lock().await;
    let dir = tempfile::Builder::new()
        .prefix("moosuri-broker-restore-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let data = dir.path().join("source");
    let kit = dir.path().join("kit");
    let heads = dir.path().join("heads");
    std::fs::DirBuilder::new().mode(0o700).create(&kit).unwrap();
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&heads)
        .unwrap();
    let store = Store::open(&data).unwrap();
    let user = admin(&store).await.user;
    let (device, key, binding, hash) = paired(&store, &user).await;
    let request = store
        .broker_fixture_probe(&user, device.clone(), binding.clone(), hash)
        .await
        .unwrap();
    let committed = commit(&store, &device, &key, &request).await.unwrap();
    let uncommitted = store
        .broker_fixture_probe(&user, device.clone(), binding.clone(), hash)
        .await
        .unwrap();
    let pending = store
        .broker_pair_challenge(&user, ORIGIN.into(), key.public_key())
        .await
        .unwrap();
    let old_auth = store.broker_auth_challenge(device.id).await.unwrap();
    let receipt = Receipt {
        commitment: committed.clone(),
        outcome: Outcome::Unknown,
        projection: Projection::None,
        projection_digest: moosuri_credential_broker::auth::projection_digest(Projection::None),
    };
    let signed_receipt = SignedReceipt {
        signature: key.sign(&receipt_bytes(&receipt)),
        receipt,
    };
    command(
        &store,
        &device,
        &key,
        Command::Settle {
            receipt: Box::new(signed_receipt.clone()),
        },
    )
    .await
    .unwrap();
    drop(store);
    let identity = kit.join("identity");
    let recipient = crate::backup::keygen(&identity).unwrap();
    crate::backup::setup(&data, &recipient, &identity).unwrap();
    let cipher = kit.join("current.age");
    let trusted = heads.join("head.json");
    let dest = dir.path().join("restored");
    crate::cutover::checkpoint(&data, &cipher, &trusted).unwrap();
    crate::cutover::restore(&cipher, &trusted, &identity, &dest).unwrap();
    let restored = Store::open(&dest).unwrap();
    assert!(
        restored
            .broker_pair(PairProof {
                challenge: pending.nonce,
                signature: key.sign(&pairing_bytes(&pending).unwrap())
            })
            .await
            .is_err()
    );
    let stale = Command::Recover {
        invocation: request.invocation.clone(),
    };
    assert!(
        restored
            .broker_command(SignedCommand {
                challenge: old_auth.nonce,
                signature: key
                    .sign(&authenticated_bytes(&old_auth, "POST", DEVICE_PATH, &stale).unwrap()),
                command: stale
            })
            .await
            .is_err()
    );
    assert!(
        commit(&restored, &device, &key, &uncommitted)
            .await
            .is_err()
    );
    let recovered = command(
        &restored,
        &device,
        &key,
        Command::Recover {
            invocation: request.invocation.clone(),
        },
    )
    .await
    .unwrap();
    let Reply::Commitment {
        commitment: recovered,
    } = recovered
    else {
        panic!("missing retained commitment")
    };
    assert!(*recovered == committed);
    assert!(matches!(
        command(
            &restored,
            &device,
            &key,
            Command::Settle {
                receipt: Box::new(signed_receipt)
            }
        )
        .await
        .unwrap(),
        Reply::Settled
    ));
    assert!(
        command(&restored, &device, &key, Command::Register { binding })
            .await
            .is_err()
    );
    restored
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM credential_broker_pairs", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM credential_broker_devices WHERE status='active'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM credential_broker_bindings WHERE active=1",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM credential_broker_commitments WHERE receipt IS NOT NULL",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            Ok(())
        })
        .await
        .unwrap();
}
