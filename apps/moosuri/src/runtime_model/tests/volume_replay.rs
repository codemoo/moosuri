use super::*;
use moosuri_core::{AccountSummary, ProviderKind, Role, RunState, SessionView};
use moosuri_store::{
    Store, UserContext,
    personal_text_preview::{Admission as LocalAdmission, ModelPin, Work},
    personal_volume::VolumeOwner,
    runtime_registry::admin,
};

/// Cold synthetic snapshot. The last Store handle joins its writer before copying.
fn copy_cold_volume(source: &Path, target: &Path) {
    for item in fs::read_dir(source).unwrap() {
        let item = item.unwrap();
        let destination = target.join(item.file_name());
        let kind = item.file_type().unwrap();
        if kind.is_dir() {
            fs::create_dir(&destination).unwrap();
            fs::set_permissions(&destination, fs::Permissions::from_mode(0o700)).unwrap();
            copy_cold_volume(&item.path(), &destination);
        } else {
            assert!(kind.is_file());
            fs::copy(item.path(), destination).unwrap();
        }
    }
}

async fn prepare_call(
    store: &Store,
    user: &UserContext,
    owner: VolumeOwner,
    thread: &str,
    id: &str,
    pin: &ModelPin,
) -> Work {
    match store
        .begin_text_preview(
            user,
            owner,
            thread.into(),
            id.into(),
            "Remember this conversation's title".into(),
            pin.clone(),
        )
        .await
        .unwrap()
    {
        LocalAdmission::Fresh(work) => work,
        _ => panic!("snapshot must predate request admission"),
    }
}

#[tokio::test]
async fn suspension_lost_reply_and_cold_volume_restore_charge_one_model_call() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let (url, provider, provider_task) = upstream().await;
    let (options, mut bindings) = fixture(&root, url);
    // Exactly one 10-token settlement leaves room for one more 2048-token reservation.
    bindings[0].budget.max_tokens = bindings[0].budget.reservation_tokens + 10;
    select_binding(&options, &bindings[0]);
    let member = &bindings[0];
    let owner = VolumeOwner {
        account: member.account_id,
        tenant: member.tenant_id,
        volume: member.volume_id,
        generation: member.generation as u64,
    };
    let active = dir(&root, "member");
    let snapshot = dir(&root, "snapshot");
    let email = "a@example.invalid";
    let initial_password = "Synthetic-restore-once-42";
    owner
        .initialize(
            &active,
            "Restore fixture".into(),
            email.into(),
            initial_password.into(),
        )
        .await
        .unwrap();
    let store = owner.open_verified(&active).await.unwrap();
    let init = store
        .login(email.into(), initial_password.into())
        .await
        .unwrap();
    let login = store
        .change_password(&init.user, "Changed-restore-once-42".into())
        .await
        .unwrap();
    let thread = store
        .create_thread(&login.user, "Only restored thread".into())
        .await
        .unwrap()
        .id;
    drop(store);
    copy_cold_volume(&active, &snapshot);

    let operator = runtime_registry::lookup_id(&options.registry, bindings[1].account_id).unwrap();
    let view = SessionView {
        user: AccountSummary {
            id: operator.account_id.clone(),
            name: "Operator".into(),
            email: "b@example.invalid".into(),
            role: Role::Member,
            must_change_password: false,
        },
        password_change_required: false,
        chat_draft_scope: None,
    };
    admin::bootstrap(&options.registry, Uuid::new_v4(), &operator, &view).unwrap();
    let admin_session = admin::checked_session(&options.registry, &operator, &view).unwrap();
    let apply = |action| {
        let current = admin::status(&options.registry, owner.account).unwrap();
        admin::change_member(
            &options.registry,
            &admin_session,
            Uuid::new_v4(),
            owner.account,
            current.generation,
            current.role_revision,
            action,
        )
        .unwrap()
    };
    let stop = CancellationToken::new();
    let server_options = options.clone_for_test();
    let signal = stop.clone();
    let server = tokio::spawn(async move { serve(&server_options, signal).await });
    let socket = path(&options, member);
    listening(&socket).await;

    let store = owner.open_verified(&active).await.unwrap();
    let id = Uuid::new_v4().to_string();
    let example = request(member);
    let pin = ModelPin {
        reservation_tokens: None,
        selection_epoch: None,
        connection_revision: example.connection_revision,
        model: example.model,
        effort: None,
        kind: ProviderKind::Compatible,
        max_output_tokens: 32,
    };
    let work = prepare_call(&store, &login.user, owner, &thread, &id, &pin).await;
    provider.mode.store(2, Ordering::SeqCst);
    let mut link = UnixStream::connect(&socket).await.unwrap();
    link.write_all(work.request()).await.unwrap();
    link.write_all(b"\n").await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), provider.entered.notified())
        .await
        .unwrap();
    assert!(apply(admin::Change::Suspend).suspended);
    drop(link);
    let denied: Reply =
        serde_json::from_slice(&personal_call(&socket, work.request()).await.unwrap()).unwrap();
    assert_eq!(denied.state, "account_fenced");
    assert!(!apply(admin::Change::Resume).suspended);
    provider.release.notify_one();
    let first_frame = work.request().to_owned();
    drop(work);
    drop(store);

    let restored = owner.open_verified(&snapshot).await.unwrap();
    let new_login = restored
        .login(email.into(), "Changed-restore-once-42".into())
        .await
        .unwrap();
    let replay = prepare_call(&restored, &new_login.user, owner, &thread, &id, &pin).await;
    assert_eq!(replay.request(), first_frame);
    let response = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let raw = personal_call(&socket, replay.request()).await.unwrap();
            let reply: Reply = serde_json::from_slice(&raw).unwrap();
            if reply.state == "settled_response_unavailable" {
                assert!(reply.output.is_none());
                assert_eq!(reply.receipt.unwrap()["charged_tokens"], 10);
                break raw;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let turn = restored
        .settle_text_preview(replay, Some(response))
        .await
        .unwrap();
    assert_eq!(turn.run.state, RunState::OutcomeUnknown);
    assert!(!turn.run.can_retry);
    assert!(turn.assistant.is_empty());
    assert_eq!(provider.count.load(Ordering::SeqCst), 1);
    provider.mode.store(0, Ordering::SeqCst);
    assert_eq!(call(&socket, &request(member)).await.state, "settled");
    assert_eq!(
        call(&socket, &request(member)).await.state,
        "quota_exhausted"
    );
    assert_eq!(provider.count.load(Ordering::SeqCst), 2);
    stop.cancel();
    server.await.unwrap().unwrap();
    provider_task.abort();
}
