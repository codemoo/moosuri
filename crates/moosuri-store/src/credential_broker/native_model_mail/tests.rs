use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_credential_broker::{auth::SigningKey, google::GoogleIdentity, policy::LocalGrant};
async fn command(s: &Store, d: &Device, k: &SigningKey, command: Command) -> Result<Reply> {
    let c = s.broker_auth_challenge(d.id).await?;
    s.broker_command(SignedCommand {
        challenge: c.nonce,
        signature: k.sign(
            &moosuri_credential_broker::auth::authenticated_bytes(
                &c,
                "POST",
                DEVICE_PATH,
                &command,
            )
            .unwrap(),
        ),
        command,
    })
    .await
}
#[tokio::test]
async fn native_model_payload_is_owner_device_grant_digest_and_background_bound() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let owner = admin(&s).await.user;
    let other = member(&s, &owner, "native-model-other@example.test")
        .await
        .user;
    let k = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let pair = s
        .broker_pair_challenge(
            &owner,
            "https://assistant.example.test".into(),
            k.public_key(),
        )
        .await
        .unwrap();
    let d = s
        .broker_pair(PairProof {
            challenge: pair.nonce,
            signature: k.sign(&pairing_bytes(&pair).unwrap()),
        })
        .await
        .unwrap();
    let identity = GoogleIdentity {
        subject: "synthetic-model-subject".into(),
        email: "synthetic@example.test".into(),
    };
    let item = Uuid::new_v4();
    let vault = Uuid::new_v4();
    let expires = now().unwrap() + 300;
    let display =
        LocalGrant::select_google_body(d.clone(), item, vault, identity.clone(), 1, 1, expires)
            .unwrap();
    command(
        &s,
        &d,
        &k,
        Command::RegisterGoogleBody {
            binding: display.binding.clone(),
        },
    )
    .await
    .unwrap();
    let read = MailModelRequest {
        version: 1,
        background: false,
        read: MailModelRead::Search {
            query: "quote".into(),
            cursor: None,
        },
    };
    assert!(
        s.request_native_model_mail(
            &owner,
            Uuid::new_v4().to_string(),
            d.id,
            display.binding.id,
            read.clone(),
            expires
        )
        .await
        .is_err()
    );
    let model = LocalGrant::select_google_model_read(
        d.clone(),
        item,
        vault,
        identity,
        1,
        1,
        expires,
        false,
    )
    .unwrap();
    command(
        &s,
        &d,
        &k,
        Command::RegisterGoogleModelRead {
            binding: model.binding.clone(),
            background: false,
        },
    )
    .await
    .unwrap();
    let id = Uuid::new_v4().to_string();
    let job = s
        .request_native_model_mail(
            &owner,
            id.clone(),
            d.id,
            model.binding.id,
            read.clone(),
            expires,
        )
        .await
        .unwrap();
    assert!(job.receipt.is_none());
    assert!(s.native_model_mail_job(&other, id.clone()).await.is_err());
    assert!(
        s.request_native_model_mail(
            &owner,
            id.clone(),
            d.id,
            model.binding.id,
            MailModelRequest {
                version: 1,
                background: false,
                read: MailModelRead::Search {
                    query: "changed".into(),
                    cursor: None
                }
            },
            expires
        )
        .await
        .is_err()
    );
    let mut bg = read.clone();
    bg.background = true;
    assert!(
        s.request_native_model_mail(
            &owner,
            Uuid::new_v4().to_string(),
            d.id,
            model.binding.id,
            bg,
            expires
        )
        .await
        .is_err()
    );
    assert!(
        s.request_native_model_mail(
            &owner,
            Uuid::new_v4().to_string(),
            d.id,
            model.binding.id,
            MailModelRequest {
                version: 1,
                background: false,
                read: MailModelRead::Message {
                    selection: Uuid::new_v4()
                }
            },
            expires
        )
        .await
        .is_err()
    );
    let Reply::Waiting { requests, .. } = command(&s, &d, &k, Command::Poll).await.unwrap() else {
        panic!("waiting")
    };
    assert_eq!(requests.len(), 1);
    let r = &requests[0];
    assert_eq!(
        r.digest,
        moosuri_credential_broker::auth::hash(&read.bytes())
    );
    assert!(r.binding == model.binding);
    assert!(
        command(
            &s,
            &d,
            &k,
            Command::ModelMailPayload {
                invocation: r.invocation.clone(),
                request_digest: [0; 32]
            }
        )
        .await
        .is_err()
    );
    let Reply::ModelMailPayload { payload } = command(
        &s,
        &d,
        &k,
        Command::ModelMailPayload {
            invocation: r.invocation.clone(),
            request_digest: moosuri_credential_broker::auth::hash(&request_bytes(r)),
        },
    )
    .await
    .unwrap() else {
        panic!("payload")
    };
    assert_eq!(payload.bytes(), read.bytes());
    // A signed search's selectors are typed, query-bound and exact-grant-version bound.
    let Reply::Commitment { commitment } = command(
        &s,
        &d,
        &k,
        Command::CommitCurrent {
            invocation: r.invocation.clone(),
            request_digest: moosuri_credential_broker::auth::hash(&request_bytes(r)),
        },
    )
    .await
    .unwrap() else {
        panic!("commitment")
    };
    let cursor = Uuid::new_v4();
    let thread = Uuid::new_v4();
    let projection = Projection::GoogleMailModelRead {
        result: Box::new(MailModelResult {
            version: 1,
            status: MailModelStatus::Ready,
            messages: vec![MailModelMessage {
                selection: Uuid::new_v4(),
                thread,
                observation: Uuid::new_v4(),
                subject: "Synthetic source".into(),
                from: "source@example.test".into(),
                text: "Bounded source".into(),
                partial: false,
                content_digest: [7; 32],
                attachments: vec![],
            }],
            next_cursor: Some(cursor),
            partial: false,
            attachment: None,
        }),
    };
    let receipt = Receipt {
        commitment: *commitment,
        outcome: Outcome::ProjectedSuccess,
        projection_digest: moosuri_credential_broker::auth::projection_digest(projection.clone()),
        projection,
    };
    let signed = SignedReceipt {
        signature: k.sign(&receipt_bytes(&receipt)),
        receipt,
    };
    command(
        &s,
        &d,
        &k,
        Command::Settle {
            receipt: Box::new(signed),
        },
    )
    .await
    .unwrap();
    for wrong in [
        MailModelRead::Search {
            query: "changed query".into(),
            cursor: Some(cursor),
        },
        MailModelRead::Message { selection: cursor },
        MailModelRead::Message { selection: thread },
    ] {
        assert!(matches!(
            s.request_native_model_mail(
                &owner,
                Uuid::new_v4().to_string(),
                d.id,
                model.binding.id,
                MailModelRequest {
                    version: 1,
                    background: false,
                    read: wrong
                },
                expires
            )
            .await,
            Err(StoreError::Forbidden)
        ));
    }
    s.request_native_model_mail(
        &owner,
        Uuid::new_v4().to_string(),
        d.id,
        model.binding.id,
        MailModelRequest {
            version: 1,
            background: false,
            read: MailModelRead::Search {
                query: "quote".into(),
                cursor: Some(cursor),
            },
        },
        expires,
    )
    .await
    .unwrap();
    let mut current = model.binding.clone();
    current.epoch += 1;
    current.grant_version += 1;
    current.item_epoch += 1;
    current.grant_hash = [83; 32];
    command(
        &s,
        &d,
        &k,
        Command::RefreshGoogleModelRead {
            expected: model.binding.clone(),
            binding: current.clone(),
            background: false,
        },
    )
    .await
    .unwrap();
    assert!(matches!(
        s.request_native_model_mail(
            &owner,
            Uuid::new_v4().to_string(),
            d.id,
            current.id,
            MailModelRequest {
                version: 1,
                background: false,
                read: MailModelRead::Search {
                    query: "quote".into(),
                    cursor: Some(cursor)
                }
            },
            expires
        )
        .await,
        Err(StoreError::Forbidden)
    ));
    s.request_native_model_mail(
        &owner,
        Uuid::new_v4().to_string(),
        d.id,
        current.id,
        read.clone(),
        expires,
    )
    .await
    .unwrap();
    // Exact binding revocation blocks both payload admission and future backend reads.
    let device = d.id.to_string();
    let binding = model.binding.id.to_string();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE credential_broker_bindings SET active=0 WHERE device_id=?1 AND binding_id=?2",
            params![device, binding],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        command(
            &s,
            &d,
            &k,
            Command::ModelMailPayload {
                invocation: r.invocation.clone(),
                request_digest: moosuri_credential_broker::auth::hash(&request_bytes(r))
            }
        )
        .await
        .is_err()
    );
}
