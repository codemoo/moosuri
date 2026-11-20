use super::*;
use crate::tests::{TEST_LOCK, admin, directory};
use moosuri_core::{MediaRef, ModelSelection, ModelSource, ProviderConfig, ProviderKind};

async fn image_chat_store() -> (tempfile::TempDir, Store, UserContext) {
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let provider = store
        .create_provider(
            &user,
            crate::ProviderChange {
                name: "Synthetic media and reminder integration".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: crate::CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&user, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_vision_probe(&user, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: provider.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    (dir, store, user)
}

async fn ready_image(store: &Store, user: &UserContext) -> MediaRef {
    let mut pixels = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut pixels, 80)
        .encode_image(&image::RgbImage::from_pixel(
            8,
            8,
            image::Rgb([12, 50, 100]),
        ))
        .unwrap();
    let image_id = Uuid::new_v4().to_string();
    let crate::media::UploadAdmission::Process(upload) = store
        .media_begin(user, image_id.clone(), Sha256::digest(&pixels).into())
        .await
        .unwrap()
    else {
        panic!("fresh synthetic upload")
    };
    store
        .media_finish(upload, Some(crate::media::normalize(&pixels).unwrap()))
        .await
        .unwrap();
    MediaRef {
        id: image_id,
        revision: 1,
    }
}

#[tokio::test]
async fn attached_and_inherited_images_cannot_create_or_revoke_reminders() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user) = image_chat_store().await;
    let context = Some(ScheduleChatContext {
        timezone: "Asia/Seoul".into(),
        target: None,
    });
    let clean = store
        .create_thread(&user, "Independent reminder".into())
        .await
        .unwrap();
    let original = store
        .submit_turn_with_schedule_context(
            &user,
            clean.id,
            Uuid::new_v4().to_string(),
            "remind me in 10 minutes to drink water".into(),
            None,
            context.clone(),
        )
        .await
        .unwrap();
    assert_eq!(original.run.state, RunState::Completed);
    let receipt = &original.run.schedule_receipts.unwrap()[0];
    let attachments = vec![ready_image(&store, &user).await];
    let thread = store
        .create_thread(&user, "Synthetic image authority".into())
        .await
        .unwrap();
    let request = Uuid::new_v4().to_string();
    let text = "remind me in 20 minutes to stretch".to_owned();
    let first = store
        .submit_turn_with_context(
            &user,
            thread.id.clone(),
            request.clone(),
            text.clone(),
            None,
            attachments.clone(),
            context.clone(),
        )
        .await
        .unwrap();
    assert_eq!(first.run.state, RunState::Completed);
    assert!(first.run.schedule_receipts.is_none());
    assert_eq!(first.assistant.len(), 1);
    assert!(first.assistant[0].text.contains("새 대화"));
    assert_eq!(store.schedules(&user).await.unwrap().len(), 1);
    let replay = store
        .submit_turn_with_context(
            &user,
            thread.id.clone(),
            request.clone(),
            text.clone(),
            None,
            attachments.clone(),
            context.clone(),
        )
        .await
        .unwrap();
    assert_eq!(replay.run.id, first.run.id);
    assert_eq!(replay.assistant.len(), 1);
    assert_eq!(replay.assistant[0].text, first.assistant[0].text);
    assert!(matches!(
        store
            .submit_turn_with_context(
                &user,
                thread.id.clone(),
                request.clone(),
                text.clone(),
                None,
                vec![],
                context.clone(),
            )
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .submit_turn_with_context(
                &user,
                thread.id.clone(),
                request,
                text,
                None,
                attachments.clone(),
                Some(ScheduleChatContext {
                    timezone: "UTC".into(),
                    target: None
                }),
            )
            .await,
        Err(StoreError::Conflict)
    ));

    let partial = store
        .submit_turn_with_schedule_context(
            &user,
            thread.id.clone(),
            Uuid::new_v4().to_string(),
            "remind me to stretch".into(),
            None,
            context.clone(),
        )
        .await
        .unwrap();
    assert!(partial.run.schedule_receipts.is_none());
    assert_eq!(partial.run.state, RunState::Queued);
    store
        .cancel_run(&user, partial.run.id, partial.run.attempt)
        .await
        .unwrap();
    let followed = store
        .submit_turn_with_schedule_context(
            &user,
            thread.id.clone(),
            Uuid::new_v4().to_string(),
            "in 10 minutes".into(),
            None,
            context.clone(),
        )
        .await
        .unwrap();
    assert!(followed.run.schedule_receipts.is_none());
    assert_eq!(followed.run.state, RunState::Queued);
    store
        .cancel_run(&user, followed.run.id, followed.run.attempt)
        .await
        .unwrap();
    let private_thread = thread.id.clone();
    store.call(move|db|{let count:i64=db.conn.query_row("SELECT count(*) FROM schedule_chat_drafts d JOIN chat_runs r ON r.tenant_id=d.tenant_id AND r.id=d.run_id WHERE r.thread_id=?1",[private_thread],|r|r.get(0))?;assert_eq!(count,0);Ok(())}).await.unwrap();
    let inherited = store
        .submit_turn_with_schedule_context(
            &user,
            thread.id,
            Uuid::new_v4().to_string(),
            "delete reminder".into(),
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: Some(ScheduleTarget {
                    id: receipt.id.clone(),
                    revision: receipt.revision,
                }),
            }),
        )
        .await
        .unwrap();
    assert_eq!(inherited.run.state, RunState::Completed);
    assert!(inherited.run.schedule_receipts.is_none());
    assert_eq!(inherited.assistant.len(), 1);
    assert_eq!(inherited.assistant[0].text, first.assistant[0].text);
    let still_owned = store.schedules(&user).await.unwrap();
    assert_eq!(still_owned.len(), 1);
    assert_eq!(still_owned[0].id, receipt.id);
    assert_eq!(still_owned[0].revision, receipt.revision);
    assert_eq!(still_owned[0].state, "active");
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        0
    );
    assert!(store.claim_run().await.unwrap().is_none());
    assert!(store.claim_agent_run().await.unwrap().is_none());

    let thread = store
        .create_thread(&user, "Synthetic ordinary image question".into())
        .await
        .unwrap();
    let question = store
        .submit_turn_with_context(
            &user,
            thread.id,
            Uuid::new_v4().to_string(),
            "What color is the attached image?".into(),
            None,
            attachments,
            context,
        )
        .await
        .unwrap();
    assert_eq!(question.run.state, RunState::Queued);
    assert!(question.assistant.is_empty());
    let claimed = store.claim_run().await.unwrap().unwrap();
    assert_eq!(claimed.lease.run_id(), question.run.id);
}

#[tokio::test]
async fn inherited_image_reminder_denial_survives_a_subscription_switch() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user) = image_chat_store().await;
    let thread = store
        .create_thread(&user, "Image before subscription switch".into())
        .await
        .unwrap()
        .id;
    let image = ready_image(&store, &user).await;
    let attached = store
        .submit_turn_with_media(
            &user,
            thread.clone(),
            Uuid::new_v4().to_string(),
            "remind me in 20 minutes to stretch".into(),
            None,
            vec![image.clone()],
        )
        .await
        .unwrap();
    assert_eq!(attached.run.state, RunState::Completed);
    assert_eq!(attached.assistant.len(), 1);
    let schedule = store
        .create_schedule(
            &user,
            ScheduleCreate {
                request_id: Uuid::new_v4().to_string(),
                draft: ScheduleDraft {
                    title: "Keep this reminder".into(),
                    message: "Drink water in ten minutes.".into(),
                    rule: ScheduleRule::Once {
                        at: now().unwrap() + 600,
                    },
                    valid_until: None,
                },
            },
        )
        .await
        .unwrap();
    let binding = store
        .create_claude_binding(
            &user,
            "Synthetic subscription".into(),
            "a".repeat(64),
            "b".repeat(64),
        )
        .await
        .unwrap();
    store
        .record_claude_probe(&user, binding.clone(), "claude-synthetic".into())
        .await
        .unwrap();
    store
        .set_model_preference(
            &user,
            Some(ModelSelection {
                source: ModelSource::PersonalClaudeCode,
                provider_id: binding.id,
                provider_revision: binding.revision,
                model: "claude-synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    store.set_default_model(&user, None).await.unwrap();

    let request = Uuid::new_v4().to_string();
    let context = Some(ScheduleChatContext {
        timezone: "Asia/Seoul".into(),
        target: Some(ScheduleTarget {
            id: schedule.id.clone(),
            revision: schedule.revision,
        }),
    });
    let submit = || {
        store.submit_turn_with_schedule_context(
            &user,
            thread.clone(),
            request.clone(),
            "delete reminder".into(),
            None,
            context.clone(),
        )
    };
    let (first, replay) = tokio::join!(submit(), submit());
    let first = first.unwrap();
    let replay = replay.unwrap();
    assert_eq!(first.run.model.source, ModelSource::PersonalClaudeCode);
    assert_eq!(first.run.state, RunState::Completed);
    assert!(first.attachments.is_none());
    assert!(first.run.schedule_receipts.is_none());
    assert_eq!(first.assistant.len(), 1);
    assert_eq!(first.assistant[0].text, attached.assistant[0].text);
    assert_eq!(replay.run.id, first.run.id);
    assert_eq!(replay.assistant.len(), 1);
    assert_eq!(replay.assistant[0].text, first.assistant[0].text);

    assert!(matches!(
        store
            .submit_turn(
                &user,
                thread.clone(),
                Uuid::new_v4().to_string(),
                "What color is the previous image?".into(),
            )
            .await,
        Err(StoreError::AttachmentReselectionRequired)
    ));
    assert!(matches!(
        store
            .submit_turn_with_context(
                &user,
                thread.clone(),
                Uuid::new_v4().to_string(),
                "delete reminder".into(),
                None,
                vec![image],
                context,
            )
            .await,
        Err(StoreError::VisionRequired)
    ));
    let page = store.conversation(&user, thread, None).await.unwrap();
    assert_eq!(page.turns.len(), 2);
    assert_eq!(page.turns[1].run.id, first.run.id);
    assert_eq!(page.turns[1].assistant.len(), 1);
    let still_owned = store.schedules(&user).await.unwrap();
    assert_eq!(still_owned.len(), 1);
    assert_eq!(still_owned[0].id, schedule.id);
    assert_eq!(still_owned[0].revision, schedule.revision);
    assert_eq!(still_owned[0].state, "active");
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        0
    );
    assert!(store.claim_run().await.unwrap().is_none());
    assert!(store.claim_agent_run().await.unwrap().is_none());
}
