use super::*;
use crate::tests::{TEST_LOCK, admin, directory};
use moosuri_core::{MediaRef, ModelSelection, ProviderConfig, ProviderKind};

#[tokio::test]
async fn attached_and_inherited_images_cannot_create_or_revoke_reminders() {
    let _guard = TEST_LOCK.lock().await;
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
                provider_id: provider.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
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
        .media_begin(&user, image_id.clone(), Sha256::digest(&pixels).into())
        .await
        .unwrap()
    else {
        panic!("fresh synthetic upload")
    };
    store
        .media_finish(upload, Some(crate::media::normalize(&pixels).unwrap()))
        .await
        .unwrap();
    let attachments = vec![MediaRef {
        id: image_id,
        revision: 1,
    }];
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
    assert!(first.assistant.iter().any(|m| m.text.contains("새 대화")));
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
                attachments,
                Some(ScheduleChatContext {
                    timezone: "UTC".into(),
                    target: None
                }),
            )
            .await,
        Err(StoreError::Conflict)
    ));

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
    let still_owned = store.schedules(&user).await.unwrap();
    assert_eq!(still_owned.len(), 1);
    assert_eq!(still_owned[0].id, receipt.id);
    assert_eq!(still_owned[0].revision, receipt.revision);
    assert_eq!(still_owned[0].state, "active");
    assert!(store.claim_run().await.unwrap().is_none());
    assert!(store.claim_agent_run().await.unwrap().is_none());
}
