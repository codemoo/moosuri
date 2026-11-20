use super::*;
use crate::tests::{TEST_LOCK, directory};
fn root() -> tempfile::TempDir {
    let d = directory();
    fs::set_permissions(
        d.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    d
}
fn view(a: &Allocation, email: &str, temporary: bool) -> SessionView {
    SessionView {
        user: moosuri_core::AccountSummary {
            id: a.account_id.clone(),
            name: "Synthetic member".into(),
            email: email.into(),
            role: Role::Member,
            must_change_password: temporary,
        },
        password_change_required: temporary,
        chat_draft_scope: None,
    }
}
fn allocate(r: &Path, email: &str) -> Allocation {
    let a = reserve(r, Uuid::new_v4(), email.into()).unwrap();
    set_routing(r, &a, true).unwrap();
    a
}
fn bootstrap_a(r: &Path) -> (Allocation, SessionView) {
    let a = allocate(r, "a@example.test");
    let v = view(&a, "a@example.test", false);
    bootstrap(r, Uuid::new_v4(), &a, &v).unwrap();
    (a, v)
}
#[test]
fn role_changes_eligibility_replay_suspension_and_last_usable_administrator() {
    let d = root();
    let r = d.path();
    let (a, av) = bootstrap_a(r);
    let b = allocate(r, "b@example.test");
    let bv = view(&b, "b@example.test", false);
    let s = checked_session(r, &a, &av).unwrap();
    assert!(checked_session(r, &b, &bv).is_err());
    // Observation checks exact identity, private member role, temp-password and readiness.
    let mut wrong = bv.clone();
    wrong.user.id = a.account_id.clone();
    assert!(observe_session(r, &b, &wrong).is_err());
    wrong = bv.clone();
    wrong.user.role = Role::Admin;
    assert!(observe_session(r, &b, &wrong).is_err());
    observe_session(r, &b, &view(&b, "b@example.test", true)).unwrap();
    let bm = status(r, Uuid::parse_str(&b.account_id).unwrap()).unwrap();
    assert!(
        change_member(
            r,
            &s,
            Uuid::new_v4(),
            Uuid::parse_str(&b.account_id).unwrap(),
            1,
            bm.role_revision,
            Change::Promote
        )
        .is_err()
    );
    let bm = observe_session(r, &b, &bv).unwrap();
    let bid = Uuid::parse_str(&b.account_id).unwrap();
    let request = Uuid::new_v4();
    let promoted =
        change_member(r, &s, request, bid, 1, bm.role_revision, Change::Promote).unwrap();
    assert_eq!(promoted.role, "admin");
    assert_eq!(
        change_member(r, &s, request, bid, 1, bm.role_revision, Change::Promote)
            .unwrap()
            .role_revision,
        promoted.role_revision
    );
    assert!(change_member(r, &s, request, bid, 1, bm.role_revision, Change::Suspend).is_err());
    assert!(
        change_member(
            r,
            &s,
            Uuid::new_v4(),
            bid,
            1,
            bm.role_revision,
            Change::Demote
        )
        .is_err()
    );
    let bs = checked_session(r, &b, &bv).unwrap();
    let am = status(r, Uuid::parse_str(&a.account_id).unwrap()).unwrap();
    let suspended = change_member(
        r,
        &bs,
        Uuid::new_v4(),
        Uuid::parse_str(&a.account_id).unwrap(),
        1,
        am.role_revision,
        Change::Suspend,
    )
    .unwrap();
    assert!(observe_session(r, &a, &av).is_err());
    assert!(lookup_ready(r, "a@example.test".into()).is_err());
    assert!(lookup_id(r, Uuid::parse_str(&a.account_id).unwrap()).is_err());
    assert!(list_members(r, &s).is_err());
    // Only B is usable: neither demotion, suspension nor operator fencing may remove B.
    assert!(set_routing(r, &b, false).is_err());
    for change in [Change::Demote, Change::Suspend] {
        assert!(
            change_member(
                r,
                &bs,
                Uuid::new_v4(),
                bid,
                1,
                promoted.role_revision,
                change
            )
            .is_err()
        );
    }
    let resumed = change_member(
        r,
        &bs,
        Uuid::new_v4(),
        Uuid::parse_str(&a.account_id).unwrap(),
        1,
        suspended.role_revision,
        Change::Resume,
    )
    .unwrap();
    assert!(!resumed.suspended);
    let id = Uuid::parse_str(&a.account_id).unwrap();
    assert!(matches!(
        with_ready_epoch(r, id, 0, |_| -> Result<()> { panic!("stale admission") }),
        Err(StoreError::Forbidden)
    ));
    assert_eq!(with_ready_epoch(r, id, 1, Ok).unwrap(), a);
    assert!(lookup_ready(r, "a@example.test".into()).is_ok());
    assert!(list_members(r, &s).is_err());
    let fresh = checked_session(r, &a, &av).unwrap();
    let members = list_members(r, &fresh).unwrap();
    assert_eq!(members.len(), 2);
    let json = serde_json::to_string(&members).unwrap();
    assert!(!json.contains("password") && !json.contains("session") && !json.contains("token"));
}
#[test]
fn bootstrap_is_singleton_replay_cannot_restore_demoted_role_or_new_generation() {
    let d = root();
    let r = d.path();
    let a = allocate(r, "a@example.test");
    let av = view(&a, "a@example.test", false);
    let id = Uuid::new_v4();
    assert!(bootstrap(r, id, &a, &view(&a, "a@example.test", true)).is_err());
    let first = bootstrap(r, id, &a, &av).unwrap();
    assert_eq!(
        bootstrap(r, id, &a, &av).unwrap().role_revision,
        first.role_revision
    );
    let b = allocate(r, "b@example.test");
    let bv = view(&b, "b@example.test", false);
    assert!(bootstrap(r, Uuid::new_v4(), &b, &bv).is_err());
    let bm = observe_session(r, &b, &bv).unwrap();
    let s = checked_session(r, &a, &av).unwrap();
    change_member(
        r,
        &s,
        Uuid::new_v4(),
        Uuid::parse_str(&b.account_id).unwrap(),
        1,
        bm.role_revision,
        Change::Promote,
    )
    .unwrap();
    let bs = checked_session(r, &b, &bv).unwrap();
    change_member(
        r,
        &bs,
        Uuid::new_v4(),
        Uuid::parse_str(&a.account_id).unwrap(),
        1,
        first.role_revision,
        Change::Demote,
    )
    .unwrap();
    assert_eq!(bootstrap(r, id, &a, &av).unwrap().role, "member");
    assert!(checked_session(r, &a, &av).is_err());
    let mut next = a.clone();
    next.generation = 2;
    {
        let (_lock, c) = open(r, false).unwrap();
        c.execute(
            "UPDATE allocations SET generation=2 WHERE account_id=?1",
            [&a.account_id],
        )
        .unwrap();
    }
    assert!(observe_session(r, &a, &av).is_err());
    assert!(bootstrap(r, id, &next, &av).is_err());
    assert_eq!(observe_session(r, &next, &av).unwrap().role, "member");
    assert!(list_members(r, &s).is_err());
}
#[test]
fn demotion_and_fencing_race_serializes_against_ready_admins() {
    let d = root();
    let r = d.path();
    let (a, av) = bootstrap_a(r);
    let b = allocate(r, "b@example.test");
    let bv = view(&b, "b@example.test", false);
    let bm = observe_session(r, &b, &bv).unwrap();
    let s = checked_session(r, &a, &av).unwrap();
    let bid = Uuid::parse_str(&b.account_id).unwrap();
    let promoted = change_member(
        r,
        &s,
        Uuid::new_v4(),
        bid,
        1,
        bm.role_revision,
        Change::Promote,
    )
    .unwrap();
    let barrier = std::sync::Barrier::new(2);
    let (fenced, demoted) = std::thread::scope(|scope| {
        let fence = scope.spawn(|| {
            barrier.wait();
            set_routing(r, &a, false)
        });
        let demote = scope.spawn(|| {
            let bs = checked_session(r, &b, &bv).unwrap();
            barrier.wait();
            change_member(
                r,
                &bs,
                Uuid::new_v4(),
                bid,
                1,
                promoted.role_revision,
                Change::Demote,
            )
        });
        (fence.join().unwrap(), demote.join().unwrap())
    });
    assert_ne!(fenced.is_ok(), demoted.is_ok());
    let (_lock, c) = open(r, false).unwrap();
    assert_eq!(count(&c).unwrap(), 1);
}
#[test]
fn checked_proof_expires_and_generation_or_fence_revokes_it() {
    let d = root();
    let r = d.path();
    let (a, av) = bootstrap_a(r);
    let mut s = checked_session(r, &a, &av).unwrap();
    s.issued = Instant::now() - Duration::from_secs(6);
    assert!(list_members(r, &s).is_err());
    let b = allocate(r, "b@example.test");
    let bv = view(&b, "b@example.test", false);
    let bm = observe_session(r, &b, &bv).unwrap();
    let s = checked_session(r, &a, &av).unwrap();
    change_member(
        r,
        &s,
        Uuid::new_v4(),
        Uuid::parse_str(&b.account_id).unwrap(),
        1,
        bm.role_revision,
        Change::Promote,
    )
    .unwrap();
    set_routing(r, &a, false).unwrap();
    assert!(list_members(r, &s).is_err());
    assert!(checked_session(r, &a, &av).is_err());
    assert!(set_routing(r, &a, true).is_err());
}
#[tokio::test]
async fn two_actual_private_accounts_keep_member_role_and_only_verified_password_changed_session_is_eligible()
 {
    let _g = TEST_LOCK.lock().await;
    let registry = root();
    let r = registry.path();
    let mut actual = Vec::new();
    for email in ["a@example.test", "b@example.test"] {
        let a = allocate(r, email);
        let volume = root();
        let owner = crate::personal_volume::VolumeOwner {
            account: Uuid::parse_str(&a.account_id).unwrap(),
            tenant: Uuid::parse_str(&a.tenant_id).unwrap(),
            volume: Uuid::parse_str(&a.volume_id).unwrap(),
            generation: 1,
        };
        owner
            .initialize(
                volume.path(),
                "Synthetic Member".into(),
                email.into(),
                "Synthetic-temporary-password-42".into(),
            )
            .await
            .unwrap();
        let store = owner.open_verified(volume.path()).await.unwrap();
        let temp = store
            .login(email.into(), "Synthetic-temporary-password-42".into())
            .await
            .unwrap();
        let tv = SessionView {
            user: temp.user.account().clone(),
            password_change_required: temp.user.password_change_required(),
            chat_draft_scope: None,
        };
        assert!(!observe_session(r, &a, &tv).unwrap().eligible);
        assert!(bootstrap(r, Uuid::new_v4(), &a, &tv).is_err());
        let changed = store
            .change_password(&temp.user, "Synthetic-changed-password-42".into())
            .await
            .unwrap();
        let v = SessionView {
            user: changed.user.account().clone(),
            password_change_required: changed.user.password_change_required(),
            chat_draft_scope: None,
        };
        assert!(observe_session(r, &a, &v).unwrap().eligible);
        actual.push((a, v, store, volume));
    }
    let (a, av, _, _) = &actual[0];
    let (b, bv, _, _) = &actual[1];
    bootstrap(r, Uuid::new_v4(), a, av).unwrap();
    let s = checked_session(r, a, av).unwrap();
    assert!(checked_session(r, b, bv).is_err());
    let bm = status(r, Uuid::parse_str(&b.account_id).unwrap()).unwrap();
    change_member(
        r,
        &s,
        Uuid::new_v4(),
        Uuid::parse_str(&b.account_id).unwrap(),
        1,
        bm.role_revision,
        Change::Promote,
    )
    .unwrap();
    assert!(checked_session(r, b, bv).is_ok());
    for (_, v, store, _) in &actual {
        assert_eq!(v.user.role, Role::Member);
        assert!(
            store
                .mail_connections(
                    &store
                        .login(v.user.email.clone(), "Synthetic-changed-password-42".into())
                        .await
                        .unwrap()
                        .user
                )
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn ready_roster_shows_invitation_then_verified_name_at_the_exact_generation() {
    let d = root();
    let r = d.path();
    let (admin, admin_view) = bootstrap_a(r);
    let session = checked_session(r, &admin, &admin_view).unwrap();
    let req = Uuid::new_v4();
    let invitation = reserve_invitation(
        r,
        &session,
        req,
        "초대 이름".into(),
        "member@example.test".into(),
    )
    .unwrap();
    let allocation = lookup(r, invitation.email.clone()).unwrap();
    activate_invitation(r, req, &allocation).unwrap();
    let row = || {
        list_members(r, &session)
            .unwrap()
            .into_iter()
            .find(|m| m.account_id == allocation.account_id)
            .unwrap()
    };
    let ready = row();
    assert_eq!(ready.display_name.as_deref(), Some("초대 이름"));
    assert!(ready.invitation_request_id.is_none());
    let mut login = view(&allocation, &invitation.email, false);
    login.user.name = "확인한 이름".into();
    observe_session(r, &allocation, &login).unwrap();
    assert_eq!(row().display_name.as_deref(), Some("확인한 이름"));
    {
        let (_lock, c) = open(r, false).unwrap();
        c.execute(
            "UPDATE allocations SET generation=generation+1 WHERE account_id=?1",
            [&allocation.account_id],
        )
        .unwrap();
    }
    assert!(row().display_name.is_none());
}

#[test]
fn invitations_reserve_only_metadata_and_replay_is_bound_to_payload_actor_revision_and_operation_kind()
 {
    let d = root();
    let r = d.path();
    let (a, av) = bootstrap_a(r);
    let s = checked_session(r, &a, &av).unwrap();
    let request = Uuid::new_v4();
    let invite = reserve_invitation(
        r,
        &s,
        request,
        "  Synthetic Invite  ".into(),
        " INVITEE@EXAMPLE.TEST ".into(),
    )
    .unwrap();
    assert_eq!(invite.email, "invitee@example.test");
    assert_eq!(invite.name, "Synthetic Invite");
    let rows = list_members(r, &s).unwrap();
    let row = rows
        .iter()
        .find(|m| m.account_id == invite.account_id)
        .unwrap();
    assert_eq!(row.invitation_request_id, Some(request));
    assert_eq!(row.invitation_name.as_deref(), Some("Synthetic Invite"));
    assert_eq!(row.display_name.as_deref(), Some("Synthetic Invite"));
    assert_eq!(
        operator_invitation(r, request).unwrap().unwrap().name,
        invite.name
    );
    assert!(operator_invitation(r, Uuid::new_v4()).unwrap().is_none());
    assert_eq!(
        reserve_invitation(r, &s, request, invite.name.clone(), invite.email.clone())
            .unwrap()
            .account_id,
        invite.account_id
    );
    assert!(reserve_invitation(r, &s, request, "Other Name".into(), invite.email.clone()).is_err());
    let recovered = reserve(r, request, invite.email.clone()).unwrap();
    assert_eq!(recovered.account_id, invite.account_id);
    assert!(lookup_ready(r, invite.email.clone()).is_err());
    assert!(
        change_member(
            r,
            &s,
            request,
            Uuid::parse_str(&a.account_id).unwrap(),
            1,
            2,
            Change::Demote
        )
        .is_err()
    );
    assert!(reserve(r, request, "other@example.test".into()).is_err());
    let op = Uuid::new_v4();
    let unobserved = status(r, Uuid::parse_str(&invite.account_id).unwrap()).unwrap();
    change_member(
        r,
        &s,
        op,
        Uuid::parse_str(&invite.account_id).unwrap(),
        1,
        unobserved.role_revision,
        Change::Suspend,
    )
    .unwrap();
    assert!(reserve_invitation(r, &s, op, "Third".into(), "third@example.test".into()).is_err());
    assert!(reserve(r, op, "third@example.test".into()).is_err());
    {
        let (_lock, c) = open(r, false).unwrap();
        c.execute(
            "UPDATE allocations SET generation=2 WHERE account_id=?1",
            [&invite.account_id],
        )
        .unwrap();
    }
    assert!(operator_invitation(r, request).is_err());
    assert!(reserve_invitation(r, &s, request, invite.name.clone(), invite.email.clone()).is_err());
    let b = allocate(r, "b@example.test");
    let bv = view(&b, "b@example.test", false);
    let bm = observe_session(r, &b, &bv).unwrap();
    change_member(
        r,
        &s,
        Uuid::new_v4(),
        Uuid::parse_str(&b.account_id).unwrap(),
        1,
        bm.role_revision,
        Change::Promote,
    )
    .unwrap();
    let bs = checked_session(r, &b, &bv).unwrap();
    let am = status(r, Uuid::parse_str(&a.account_id).unwrap()).unwrap();
    change_member(
        r,
        &bs,
        Uuid::new_v4(),
        Uuid::parse_str(&a.account_id).unwrap(),
        1,
        am.role_revision,
        Change::Demote,
    )
    .unwrap();
    assert!(reserve_invitation(r, &s, request, invite.name, invite.email).is_err());
}
#[test]
fn installation_schema_one_migrates_on_lookup_and_future_version_is_rejected() {
    let d = root();
    let r = d.path();
    let a = allocate(r, "a@example.test");
    {
        let (_lock, c) = open(r, false).unwrap();
        c.execute_batch("DROP TABLE engine_quota_requests; DROP TABLE engine_connections; DROP TABLE engine_bindings; DROP TABLE engine_settings; ALTER TABLE allocations DROP COLUMN session_epoch;DROP TABLE installation_members;DROP TABLE installation_admin_bootstrap;DROP TABLE installation_admin_operations;PRAGMA user_version=1;").unwrap();
    }
    assert_eq!(lookup_ready(r, "a@example.test".into()).unwrap(), a);
    {
        let (_lock, c) = open(r, false).unwrap();
        assert_eq!(
            c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            6
        );
        c.execute_batch("PRAGMA user_version=7").unwrap();
    }
    assert!(lookup(r, "a@example.test".into()).is_err());
}

#[test]
fn installation_engine_choice_ignores_existing_member_overrides() {
    use crate::runtime_registry::engine::{self, InstallationChange, ModelChoice};
    let directory = root();
    let registry = directory.path();
    let (admin, view) = bootstrap_a(registry);
    let member = allocate(registry, "b@example.test");
    let unbound = allocate(registry, "c@example.test");
    let session = checked_session(registry, &admin, &view).unwrap();
    let accounts = [&admin, &member].map(|allocation| allocation.account_id.parse().unwrap());
    let connection_revision = Uuid::new_v4();
    let host = ModelChoice {
        model: "host-model".into(),
        effort: None,
    };
    let selected = ModelChoice {
        model: "installation-model".into(),
        effort: Some("high".into()),
    };
    engine::publish_models(
        registry,
        connection_revision,
        moosuri_core::ProviderKind::OpenAi,
        engine::ConnectionMode::Api,
        &host,
        &[host.clone(), selected.clone()],
        &accounts,
    )
    .unwrap();
    {
        let (_lock, c) = open(registry, false).unwrap();
        let legacy_choice = serde_json::to_string(&host).unwrap();
        for (account, previous_connection) in accounts
            .into_iter()
            .zip([connection_revision, Uuid::new_v4()])
        {
            c.execute(
                "UPDATE engine_bindings SET choice=?1,choice_connection=?2 WHERE account_id=?3",
                params![
                    legacy_choice,
                    previous_connection.to_string(),
                    account.to_string()
                ],
            )
            .unwrap();
        }
    }
    let settings = serde_json::to_value(engine::read(registry, &session).unwrap()).unwrap();
    assert!(settings.get("members").is_none());
    engine::change(
        registry,
        &session,
        InstallationChange {
            revision: settings["revision"].as_i64().unwrap(),
            connection_revision,
            enabled: true,
            default_choice: Some(selected.clone()),
        },
    )
    .unwrap();
    for account in accounts {
        let actual = engine::member_model_policy(registry, account)
            .unwrap()
            .unwrap();
        assert_eq!(actual.model, selected.model);
        assert_eq!(actual.effort, selected.effort);
    }
    assert!(matches!(
        engine::member_model_policy(registry, unbound.account_id.parse().unwrap()),
        Err(StoreError::Forbidden)
    ));
}

#[test]
fn concurrent_first_setup_has_one_identity_and_retries_preserve_it() {
    let root = root();
    let ids = [Uuid::new_v4(), Uuid::new_v4()];
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let root = root.path();
        let barrier = &barrier;
        let workers = ids.map(|id| {
            scope.spawn(move || {
                barrier.wait();
                reserve_first_invitation(root, id, "First".into(), "first@example.test".into())
            })
        });
        workers.map(|w| w.join().unwrap())
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let first = results
        .into_iter()
        .find_map(std::result::Result::ok)
        .unwrap();
    let replay = reserve_first_invitation(
        root.path(),
        first.request_id,
        "First".into(),
        "FIRST@example.test".into(),
    )
    .unwrap();
    assert_eq!(replay.account_id, first.account_id);
    assert!(
        reserve_first_invitation(
            root.path(),
            first.request_id,
            "Other".into(),
            "first@example.test".into()
        )
        .is_err()
    );
    assert_eq!(
        operator_invitation(root.path(), first.request_id)
            .unwrap()
            .unwrap()
            .account_id,
        first.account_id
    );
    assert_eq!(
        status(root.path(), Uuid::parse_str(&first.account_id).unwrap())
            .unwrap()
            .role,
        "member"
    );
    let other = super::tests::root();
    reserve(other.path(), first.request_id, first.email.clone()).unwrap();
    assert!(
        reserve_first_invitation(other.path(), first.request_id, first.name, first.email).is_err()
    );
}

#[test]
fn legacy_registry_keeps_selected_engine_settings_when_broker_first_publishes_its_mode() {
    use super::super::engine::{self, ConnectionMode, InstallationChange, ModelChoice};
    let d = root();
    let r = d.path();
    let (admin, view) = bootstrap_a(r);
    let session = checked_session(r, &admin, &view).unwrap();
    let account = admin.account_id.parse().unwrap();
    let connection = Uuid::new_v4();
    let model = ModelChoice {
        model: "synthetic".into(),
        effort: None,
    };
    let publish = |mode| {
        engine::publish_models(
            r,
            connection,
            moosuri_core::ProviderKind::OpenAi,
            mode,
            &model,
            std::slice::from_ref(&model),
            &[account],
        )
    };
    let settings = || serde_json::to_value(engine::read(r, &session).unwrap()).unwrap();
    publish(ConnectionMode::ChatgptPlan).unwrap();
    engine::change(
        r,
        &session,
        InstallationChange {
            revision: settings()["revision"].as_i64().unwrap(),
            connection_revision: connection,
            enabled: false,
            default_choice: Some(model.clone()),
        },
    )
    .unwrap();
    {
        let (_lock, c) = open(r, false).unwrap();
        c.execute_batch(
            "DROP TABLE engine_quota_requests; ALTER TABLE engine_settings DROP COLUMN connection_mode;
            ALTER TABLE installation_members DROP COLUMN display_name; PRAGMA user_version=4;",
        )
        .unwrap();
    }
    let legacy = settings();
    assert!(legacy["connection_mode"].is_null());
    assert_eq!(legacy["enabled"], false);
    publish(ConnectionMode::ChatgptPlan).unwrap();
    let current = settings();
    assert_eq!(current["connection_mode"], "chatgpt_plan");
    assert_eq!(current["default_choice"], legacy["default_choice"]);
    assert_eq!(current["enabled"], false);
    assert!(publish(ConnectionMode::Api).is_err());
    publish(ConnectionMode::ChatgptPlan).unwrap();
    assert_eq!(settings()["revision"], current["revision"]);
}

#[test]
fn web_worker_selects_only_current_reserved_web_requests_and_pages_past_waiting_members() {
    let first_root = root();
    reserve_first_invitation(
        first_root.path(),
        Uuid::new_v4(),
        "First".into(),
        "first@example.test".into(),
    )
    .unwrap();
    assert!(
        pending_web_invitations(first_root.path(), 0)
            .unwrap()
            .is_empty()
    );

    let d = root();
    let r = d.path();
    let (a, v) = bootstrap_a(r);
    let session = checked_session(r, &a, &v).unwrap();
    reserve(r, Uuid::new_v4(), "raw@example.test".into()).unwrap();
    let mut expected = Vec::new();
    for n in 0..22 {
        let id = Uuid::new_v4();
        let email = format!("synthetic-{n}@example.test");
        reserve_invitation(r, &session, id, format!("Synthetic {n}"), email.clone()).unwrap();
        let alloc = lookup(r, email).unwrap();
        match n {
            0 => activate_invitation(r, id, &alloc).unwrap(),
            1 => set_routing(r, &alloc, false).unwrap(),
            2 => {
                let (_lock, c) = open(r, false).unwrap();
                c.execute(
                    "UPDATE allocations SET generation=generation+1 WHERE request_id=?1",
                    [id.to_string()],
                )
                .unwrap();
            }
            _ => expected.push(id),
        }
    }
    let p1 = pending_web_invitations(r, 0).unwrap();
    assert_eq!(p1.len(), 16);
    let p2 = pending_web_invitations(r, p1.last().unwrap().0).unwrap();
    assert_eq!(
        p1.iter().chain(&p2).map(|r| r.1).collect::<Vec<_>>(),
        expected
    );
    assert!(
        pending_web_invitations(r, p2.last().unwrap().0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn quota_requests_expire_and_demoted_admin_cannot_trigger_or_receive_shared_key_capacity() {
    use super::super::engine::{self, quota};
    let directory = root();
    let r = directory.path();
    let (a, av) = bootstrap_a(r);
    let b = allocate(r, "second@example.test");
    let bv = view(&b, "second@example.test", false);
    observe_session(r, &b, &bv).unwrap();
    let admin = checked_session(r, &a, &av).unwrap();
    let model = engine::ModelChoice {
        model: "fixture".into(),
        effort: None,
    };
    let connection = Uuid::new_v4();
    engine::publish_models(
        r,
        connection,
        moosuri_core::ProviderKind::OpenRouter,
        engine::ConnectionMode::Api,
        &model,
        std::slice::from_ref(&model),
        &[a.account_id.parse().unwrap()],
    )
    .unwrap();
    let request = |session, probe_id| {
        quota::request(
            r,
            session,
            quota::Request {
                connection_revision: connection,
                revision: 1,
                probe_id,
            },
        )
    };
    {
        let (_lock, c) = open(r, false).unwrap();
        c.execute_batch("DROP TABLE engine_quota_requests; PRAGMA user_version=5;")
            .unwrap();
    }
    let selected = request(&admin, None).unwrap().probe_id;
    {
        let (_lock, c) = open(r, false).unwrap();
        c.execute(
            "UPDATE engine_quota_requests SET requested_at=unixepoch()-31",
            [],
        )
        .unwrap();
    }
    assert!(quota::claim(r, connection).unwrap().is_none());
    assert!(matches!(
        request(&admin, Some(selected)),
        Err(StoreError::Conflict)
    ));
    let pending = request(&admin, None).unwrap().probe_id;
    assert_ne!(selected, pending);
    let member = status(r, b.account_id.parse().unwrap()).unwrap();
    change_member(
        r,
        &admin,
        Uuid::new_v4(),
        b.account_id.parse().unwrap(),
        b.generation,
        member.role_revision,
        Change::Promote,
    )
    .unwrap();
    let other_admin = checked_session(r, &b, &bv).unwrap();
    assert!(matches!(
        request(&other_admin, Some(pending)),
        Err(StoreError::Conflict)
    ));
    let member = status(r, a.account_id.parse().unwrap()).unwrap();
    change_member(
        r,
        &other_admin,
        Uuid::new_v4(),
        a.account_id.parse().unwrap(),
        a.generation,
        member.role_revision,
        Change::Demote,
    )
    .unwrap();
    assert!(quota::claim(r, connection).unwrap().is_none());
    assert!(matches!(
        request(&admin, Some(pending)),
        Err(StoreError::Forbidden)
    ));
}
