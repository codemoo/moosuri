use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};

async fn setup() -> (tempfile::TempDir, Store, UserContext, String) {
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let p = store
        .create_provider(
            &u,
            ProviderChange {
                name: "Synthetic".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &u,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: p.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let t = store.create_thread(&u, "Inbox".into()).await.unwrap().id;
    let owner = u.clone();
    store.call(move|db|{db.conn.execute("INSERT INTO gws_inbox_selections(tenant_id,account_id,auth_epoch,fingerprint,confirmed) VALUES(?1,?2,?3,?4,1)",params![owner.tenant.0,owner.account.id,owner.epoch,[7u8;32].as_slice()])?;Ok(())}).await.unwrap();
    (dir, store, u, t)
}
async fn card(s: &Store, u: &UserContext, t: &str, text: &str) -> ChatTurn {
    let turn = s
        .submit_turn(u, t.into(), Uuid::new_v4().to_string(), text.into())
        .await
        .unwrap_or_else(|e| panic!("fixture {text}: {e}"));
    complete_card(s, u, t, turn.run.id).await
}

async fn complete_card(s: &Store, u: &UserContext, t: &str, id: String) -> ChatTurn {
    // The fixture writes the same authoritative terminal marker as the runtime,
    // without invoking a model or network. Negative fixtures may use other text.
    let tenant = u.tenant.0.clone();
    let stored_id = id.clone();
    s.call(move|db|{db.conn.execute("UPDATE chat_runs SET state='completed' WHERE tenant_id=?1 AND id=?2",params![tenant,stored_id])?;db.conn.execute("INSERT INTO chat_output(tenant_id,run_id,item,id,ordinal,text,phase,refusal) VALUES(?1,?2,'mail-recovery-gws_setup','fixture-setup-card',1,'','final_answer',0)",params![tenant,stored_id])?;Ok(())}).await.unwrap();
    s.conversation(u, t.into(), None)
        .await
        .unwrap()
        .turns
        .into_iter()
        .find(|x| x.run.id == id)
        .unwrap()
}
#[tokio::test]
async fn duplicate_concurrent_and_restored_resume_share_one_canonical_turn() {
    let _guard = TEST_LOCK.lock().await;
    let (dir, s, u, t) = setup().await;
    let original = card(&s, &u, &t, "내 가장 최근 메일 보여줘").await;
    assert!(original.run.gmail_resume_available);
    let (a, b) = tokio::join!(
        s.resume_gmail(&u, t.clone(), original.run.id.clone(), [7; 32]),
        s.resume_gmail(&u, t.clone(), original.run.id.clone(), [7; 32])
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.run.id, b.run.id);
    assert_ne!(original.run.id, a.run.id);
    assert_eq!(
        s.conversation(&u, t.clone(), None)
            .await
            .unwrap()
            .turns
            .len(),
        2
    );
    // Fresh authenticated owner session after a reload can recover a lost ACK.
    let new_session = s
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    let replay = s
        .resume_gmail(
            &new_session.user,
            t.clone(),
            original.run.id.clone(),
            [7; 32],
        )
        .await
        .unwrap();
    assert_eq!(a.run.id, replay.run.id);
    assert!(
        s.resume_gmail(&new_session.user, t.clone(), a.run.id.clone(), [7; 32])
            .await
            .is_err()
    );
    s.cancel_run(&new_session.user, a.run.id, 1).await.unwrap();
    s.submit_turn(
        &new_session.user,
        t.clone(),
        Uuid::new_v4().to_string(),
        "new question".into(),
    )
    .await
    .unwrap();
    assert!(matches!(
        s.resume_gmail(&new_session.user, t, original.run.id, [7; 32])
            .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn rejects_semantic_compound_send_missing_card_wrong_thread_and_owner() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, s, u, t) = setup().await;
    for text in [
        "Summarize my recent emails",
        "show my latest email and reply",
        "send my latest email",
        "Translate show my latest email",
    ] {
        let c = card(&s, &u, &t, text).await;
        assert!(!c.run.gmail_resume_available);
        assert!(matches!(
            s.resume_gmail(&u, t.clone(), c.run.id, [7; 32]).await,
            Err(StoreError::Conflict)
        ));
    }
    let no = s
        .submit_turn(
            &u,
            t.clone(),
            Uuid::new_v4().to_string(),
            "show my latest email".into(),
        )
        .await
        .unwrap();
    assert!(
        s.resume_gmail(&u, t.clone(), no.run.id.clone(), [7; 32])
            .await
            .is_err()
    );
    s.cancel_run(&u, no.run.id, 1).await.unwrap();
    let c = card(&s, &u, &t, "show my latest email").await;
    let wrong = s.create_thread(&u, "other".into()).await.unwrap().id;
    assert!(matches!(
        s.resume_gmail(&u, wrong, c.run.id.clone(), [7; 32]).await,
        Err(StoreError::NotFound)
    ));
    let other = member(&s, &u, "other@example.test").await.user;
    assert!(
        s.resume_gmail(&other, t.clone(), c.run.id.clone(), [7; 32])
            .await
            .is_err()
    );
    assert!(matches!(
        s.resume_gmail(&u, t, c.run.id, [8; 32]).await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn original_payload_and_confirmation_revocation_are_fenced() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, s, u, t) = setup().await;
    let c = card(&s, &u, &t, "show my latest email").await;
    let id = c.run.id.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE chat_runs SET payload_digest=?1 WHERE id=?2",
            params![[0u8; 32].as_slice(), id],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.resume_gmail(&u, t.clone(), c.run.id, [7; 32])
            .await
            .is_err()
    );
    let c = card(&s, &u, &t, "show my latest email").await;
    s.call(move |db| {
        db.conn
            .execute("UPDATE gws_inbox_selections SET confirmed=0", [])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(matches!(
        s.resume_gmail(&u, t, c.run.id, [7; 32]).await,
        Err(StoreError::Conflict)
    ));
}

#[tokio::test]
async fn attachments_selected_artifacts_context_and_old_auth_never_resume() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, s, u, t) = setup().await;
    let turn = s
        .submit_turn_with_schedule_context(
            &u,
            t.clone(),
            Uuid::new_v4().to_string(),
            "show my latest email".into(),
            None,
            Some(moosuri_core::ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    let c = complete_card(&s, &u, &t, turn.run.id).await;
    assert!(!c.run.gmail_resume_available);
    assert!(
        s.resume_gmail(&u, t.clone(), c.run.id, [7; 32])
            .await
            .is_err()
    );
    let c = card(&s, &u, &t, "show my latest email").await;
    let media = Uuid::new_v4().to_string();
    s.media_begin(&u, media.clone(), [9; 32]).await.unwrap();
    let tenant = u.tenant.0.clone();
    let id = c.run.id.clone();
    s.call(move |db| {
        db.conn.execute(
            "INSERT INTO media_run_dependencies VALUES(?1,?2,?3,1)",
            params![tenant, id, media],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.resume_gmail(&u, t.clone(), c.run.id, [7; 32])
            .await
            .is_err()
    );
    let c = card(&s, &u, &t, "show my latest email").await;
    let tenant = u.tenant.0.clone();
    let id = c.run.id.clone();
    s.call(move|db|{db.conn.execute("INSERT OR REPLACE INTO personal_artifact_run_profiles VALUES(?1,?2,1,'synthetic-artifact',1,0)",params![tenant,id])?;Ok(())}).await.unwrap();
    assert!(
        s.resume_gmail(&u, t.clone(), c.run.id, [7; 32])
            .await
            .is_err()
    );
    // A rotated auth epoch never carries an earlier question into the account.
    let c = card(&s, &u, &t, "show my latest email").await;
    let changed = s
        .change_password(&u, "New-synthetic-password".into())
        .await
        .unwrap();
    assert!(
        s.resume_gmail(&changed.user, t, c.run.id, [7; 32])
            .await
            .is_err()
    );
}
