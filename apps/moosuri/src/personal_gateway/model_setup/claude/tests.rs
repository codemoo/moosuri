//! State-transition tests for the subscription setup. They run the real
//! receipt/config functions on a private directory and the real handler
//! functions against a `Gateway` built like the other gateway tests. Nothing
//! here reaches the host runtime, a helper process or a provider: every case
//! stops before a host call, and the cases that would need one are listed in
//! the integration notes instead of being faked.
use super::*;
use crate::engine_login::Endpoint;
use receipt::{Publication, Seen};

fn alloc(epoch: i64) -> SessionAllocation {
    SessionAllocation {
        allocation: Allocation {
            account_id: Uuid::new_v4().to_string(),
            tenant_id: Uuid::new_v4().to_string(),
            volume_id: Uuid::new_v4().to_string(),
            generation: 2,
        },
        epoch,
    }
}
fn endpoint(uid: u32) -> Endpoint {
    Endpoint {
        socket: "/run/moosuri/worker/engine.sock".into(),
        worker_uid: uid,
    }
}
fn receipt_for(a: &Allocation, id: Uuid, state: Stage) -> Receipt {
    let o = owner_of(a).unwrap();
    Receipt {
        version: receipt::VERSION,
        request_id: id,
        account_id: o.account_id,
        tenant_id: o.tenant_id,
        volume_id: o.volume_id,
        generation: a.generation,
        digest: "digest".into(),
        config_sha256: Some(receipt::sha(&config_for(a, id))),
        replace_revision: None,
        model: "claude-sonnet-5-5".into(),
        effort: Some("medium".into()),
        state,
        updated: 1,
        mode: Mode::Connect,
        target_revision: None,
    }
}
fn config_for(a: &Allocation, id: Uuid) -> Vec<u8> {
    receipt::config_bytes(&Publication {
        request: id,
        owner: &owner_of(a).unwrap(),
        peer_uid: 10001,
        endpoint: &endpoint(60001),
        model: "claude-sonnet-5-5",
        effort: Some("medium"),
    })
    .unwrap()
}
fn private_dir() -> (tempfile::TempDir, PathBuf, File) {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().canonicalize().unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    let d = model_dir(&p).unwrap();
    (t, p, d)
}
fn job(a: &SessionAllocation, session: &str) -> (Job, mpsc::Receiver<Zeroizing<String>>) {
    let (tx, rx) = mpsc::channel(1);
    let j = Job::new(
        Uuid::new_v4(),
        a,
        session,
        "digest".into(),
        None,
        None,
        "claude-sonnet-5-5".into(),
        Some("medium".into()),
        tx,
        CancellationToken::new(),
    );
    (j, rx)
}
/// A gateway whose model-setup directory is a private temp directory. No
/// member service exists: only the functions that run after authentication.
fn gateway() -> (Vec<tempfile::TempDir>, Gateway, PathBuf, File) {
    let base = tempfile::tempdir().unwrap();
    let options = crate::personal_gateway::tests::options(&base.path().canonicalize().unwrap());
    let mut g = Gateway::new(&options).unwrap();
    let (keep, dir, handle) = private_dir();
    g.model_setup = true;
    g.model_setup_dir = Some(dir.clone());
    (vec![base, keep], g, dir, handle)
}
fn state_of(v: &Value) -> &str {
    v["operation"]["state"].as_str().unwrap()
}

#[test]
fn config_is_exact_only_for_the_receipts_request_owner_and_model() {
    let a = alloc(1).allocation;
    let id = Uuid::new_v4();
    let r = receipt_for(&a, id, Stage::AwaitingCode);
    let exact = config_for(&a, id);
    assert!(crate::runtime_model::setup_config_parses(&exact));
    assert_eq!(receipt::classify(None, &r), Relation::Absent);
    assert_eq!(receipt::classify(Some(&exact), &r), Relation::Exact);
    assert_eq!(
        receipt::classify(Some(&config_for(&a, Uuid::new_v4())), &r),
        Relation::Other
    );
    for garbage in [
        &b"not json"[..],
        b"{}",
        br#"{"connection_revision":"nope"}"#,
    ] {
        assert_eq!(receipt::classify(Some(garbage), &r), Relation::Unreadable);
    }
    // Same revision but a different owner field, model, effort or binding set
    // is never "ours", and is never retirable either.
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut v: Value = serde_json::from_slice(&exact).unwrap();
        f(&mut v);
        serde_json::to_vec(&v).unwrap()
    };
    let foreign = Uuid::new_v4().to_string();
    for changed in [
        edit(&|v| v["bindings"][0]["tenant_id"] = json!(foreign)),
        edit(&|v| v["bindings"][0]["volume_id"] = json!(foreign)),
        edit(&|v| v["bindings"][0]["account_id"] = json!(foreign)),
        edit(&|v| v["bindings"][0]["generation"] = json!(99)),
        edit(&|v| v["bindings"][0]["worker"]["socket"] = json!("/run/other.sock")),
        edit(&|v| v["bindings"][0]["peer_uid"] = json!(12001)),
        edit(&|v| v["provider"]["base_url"] = json!("https://example.test")),
        edit(&|v| v["model"] = json!("claude-opus-4-1")),
        edit(&|v| v["effort"] = json!("high")),
        edit(&|v| v["subscription"] = json!(null)),
        edit(&|v| {
            let first = v["bindings"][0].clone();
            v["bindings"].as_array_mut().unwrap().push(first);
        }),
    ] {
        let rel = receipt::classify(Some(&changed), &r);
        assert_eq!(rel, Relation::Mismatch);
        assert!(!rel.may_retire());
    }
    assert!(Relation::Absent.may_retire() && Relation::Other.may_retire());
    assert!(!Relation::Exact.may_retire() && !Relation::Unreadable.may_retire());
}

#[test]
fn receipt_states_resolve_against_the_published_config() {
    let a = alloc(1).allocation;
    let id = Uuid::new_v4();
    use Relation::*;
    for live in [
        Stage::Preparing,
        Stage::AwaitingCode,
        Stage::Verifying,
        Stage::Activating,
    ] {
        let r = receipt_for(&a, id, live);
        assert_eq!(r.effective(Exact), Stage::CleanupPending);
        assert_eq!(r.effective(Absent), Stage::Interrupted);
        assert_eq!(r.effective(Other), Stage::Interrupted);
        assert_eq!(r.effective(Mismatch), Stage::CleanupPending);
        assert_eq!(r.effective(Unreadable), Stage::CleanupPending);
        assert!(r.unresolved(Absent) && r.unresolved(Unreadable) && r.unresolved(Exact));
    }
    let pending = receipt_for(&a, id, Stage::CleanupPending);
    assert_eq!(pending.effective(Absent), Stage::CleanupPending);
    assert_eq!(pending.effective(Exact), Stage::CleanupPending);
    for done in [Stage::Ready, Stage::Cancelled, Stage::Failed] {
        let r = receipt_for(&a, id, done);
        for rel in [Absent, Other, Exact, Mismatch, Unreadable] {
            assert_eq!(r.effective(rel), done);
            assert!(!r.unresolved(rel));
        }
    }
    let ready = receipt_for(&a, id, Stage::Ready).view(Exact);
    assert_eq!(ready["state"], "ready");
    assert_eq!(ready["connection_revision"], id.to_string());
    assert!(
        receipt_for(&a, id, Stage::AwaitingCode)
            .view(Absent)
            .get("connection_revision")
            .is_none()
    );
}

#[test]
fn a_receipt_belongs_to_the_full_owner_only() {
    let a = alloc(1).allocation;
    let r = receipt_for(&a, Uuid::new_v4(), Stage::Failed);
    assert!(r.owner_is(&a));
    let mut other = a.clone();
    other.tenant_id = Uuid::new_v4().to_string();
    assert!(!r.owner_is(&other));
    other = a.clone();
    other.volume_id = Uuid::new_v4().to_string();
    assert!(!r.owner_is(&other));
    other = a.clone();
    other.account_id = Uuid::new_v4().to_string();
    assert!(!r.owner_is(&other));
    other = a.clone();
    other.generation += 1;
    assert!(!r.owner_is(&other));
    other = a.clone();
    other.account_id = "not-a-uuid".into();
    assert!(!r.owner_is(&other));
}

#[test]
fn receipt_file_is_private_versioned_and_holds_no_secret_or_url() {
    let (_t, p, d) = private_dir();
    let a = alloc(1).allocation;
    let r = receipt_for(&a, Uuid::new_v4(), Stage::AwaitingCode);
    receipt::write_receipt(&d, &r).unwrap();
    let path = p.join("claude-subscription-operation.json");
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
    let keys: std::collections::BTreeSet<String> =
        serde_json::from_slice::<serde_json::Map<String, Value>>(&fs::read(&path).unwrap())
            .unwrap()
            .keys()
            .cloned()
            .collect();
    let expected = [
        "version",
        "request_id",
        "account_id",
        "tenant_id",
        "volume_id",
        "generation",
        "digest",
        "config_sha256",
        "replace_revision",
        "model",
        "effort",
        "state",
        "updated",
    ];
    let expected: std::collections::BTreeSet<String> =
        expected.iter().map(|k| k.to_string()).collect();
    assert_eq!(keys, expected);
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().request_id,
        r.request_id
    );
    // A receipt of another format version is refused, never half-trusted.
    let mut old = r.clone();
    old.version = 1;
    fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    assert!(receipt::read_receipt(&d).is_err());
}

#[test]
fn a_new_setup_never_overwrites_an_unresolved_receipt() {
    let (_t, _p, d) = private_dir();
    let a = alloc(1).allocation;
    let (old, fresh) = (Uuid::new_v4(), Uuid::new_v4());
    receipt::write_receipt(&d, &receipt_for(&a, old, Stage::AwaitingCode)).unwrap();
    let refused = receipt::begin(&d, &receipt_for(&a, fresh, Stage::Preparing)).unwrap_err();
    assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(receipt::read_receipt(&d).unwrap().unwrap().request_id, old);
    // The same request may rewrite its own receipt.
    receipt::begin(&d, &receipt_for(&a, old, Stage::Preparing)).unwrap();
    // Config alone is not enough: the root publication must also complete.
    receipt::commit(&d, &config_for(&a, old), None).unwrap();
    assert!(receipt::begin(&d, &receipt_for(&a, fresh, Stage::Preparing)).is_err());
    receipt::write_receipt(&d, &receipt_for(&a, old, Stage::Ready)).unwrap();
    receipt::begin(&d, &receipt_for(&a, fresh, Stage::Preparing)).unwrap();
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().request_id,
        fresh
    );
    // A terminal receipt never blocks.
    receipt::write_receipt(&d, &receipt_for(&a, fresh, Stage::Cancelled)).unwrap();
    receipt::begin(&d, &receipt_for(&a, Uuid::new_v4(), Stage::Preparing)).unwrap();
}

#[test]
fn unfinished_means_unresolved_not_merely_non_terminal() {
    let (_t, _p, d) = private_dir();
    let a = alloc(1).allocation;
    let id = Uuid::new_v4();
    assert!(!unfinished(&d));
    receipt::write_receipt(&d, &receipt_for(&a, id, Stage::Verifying)).unwrap();
    assert!(unfinished(&d));
    receipt::commit(&d, &config_for(&a, id), None).unwrap();
    assert!(unfinished(&d));
    receipt::write_receipt(&d, &receipt_for(&a, id, Stage::Ready)).unwrap();
    assert!(!unfinished(&d));
    receipt::write_receipt(&d, &receipt_for(&a, Uuid::new_v4(), Stage::Verifying)).unwrap();
    assert!(unfinished(&d));
}

#[test]
fn publication_outcome_follows_the_config_actually_on_disk() {
    let (_t, p, d) = private_dir();
    let a = alloc(1).allocation;
    let (first, second) = (Uuid::new_v4(), Uuid::new_v4());
    let (c1, c2) = (config_for(&a, first), config_for(&a, second));
    // Stale base: nothing is written.
    assert_eq!(
        receipt::settle(&d, &c1, Some(Uuid::new_v4())),
        Settled::Conflict
    );
    assert!(!p.join(CONFIG_FILE).exists());
    assert_eq!(receipt::settle(&d, &c1, None), Settled::Exact);
    assert_eq!(receipt::settle(&d, &c1, None), Settled::Exact);
    assert_eq!(receipt::settle(&d, &c2, None), Settled::Conflict);
    assert_eq!(receipt::settle(&d, &c2, Some(first)), Settled::Exact);
    // An IO error after a durable rename: the exact config exists, so the
    // root mark is still due.
    let eio = || Err(io::Error::from_raw_os_error(libc::EIO));
    assert_eq!(receipt::settle_after(&d, &c2, eio()), Settled::Exact);
    // IO error and a different config: definitely not published.
    assert_eq!(receipt::settle_after(&d, &c1, eio()), Settled::Absent);
    fs::remove_file(p.join(CONFIG_FILE)).unwrap();
    assert_eq!(receipt::settle_after(&d, &c1, eio()), Settled::Absent);
    // IO error and an unreadable config is never reported as absent.
    fs::write(p.join(CONFIG_FILE), &c1).unwrap();
    fs::set_permissions(p.join(CONFIG_FILE), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(receipt::settle_after(&d, &c1, eio()), Settled::Uncertain);
    let Seen { current, relation } =
        receipt::inspect(&d, Some(&receipt_for(&a, first, Stage::Verifying)));
    assert!(current.is_err());
    assert_eq!(relation, Relation::Unreadable);
}

#[test]
fn request_id_is_tied_to_the_full_owner_inputs_and_replace_revision() {
    let a = alloc(1);
    let d = |a: &Allocation, email: &str, model: &str, effort: Option<&str>, r: Option<Uuid>| {
        digest_of(a, email, model, effort, r)
    };
    let base = d(
        &a.allocation,
        "me@example.test",
        "claude-sonnet-5-5",
        Some("medium"),
        None,
    );
    assert_eq!(
        base,
        d(
            &a.allocation,
            "me@example.test",
            "claude-sonnet-5-5",
            Some("medium"),
            None
        )
    );
    let mut tenant = a.allocation.clone();
    tenant.tenant_id = Uuid::new_v4().to_string();
    let mut volume = a.allocation.clone();
    volume.volume_id = Uuid::new_v4().to_string();
    let mut generation = a.allocation.clone();
    generation.generation += 1;
    for other in [
        d(
            &a.allocation,
            "you@example.test",
            "claude-sonnet-5-5",
            Some("medium"),
            None,
        ),
        d(
            &a.allocation,
            "me@example.test",
            "claude-opus-4-1",
            Some("medium"),
            None,
        ),
        d(
            &a.allocation,
            "me@example.test",
            "claude-sonnet-5-5",
            None,
            None,
        ),
        d(
            &a.allocation,
            "me@example.test",
            "claude-sonnet-5-5",
            Some("medium"),
            Some(Uuid::new_v4()),
        ),
        d(
            &tenant,
            "me@example.test",
            "claude-sonnet-5-5",
            Some("medium"),
            None,
        ),
        d(
            &volume,
            "me@example.test",
            "claude-sonnet-5-5",
            Some("medium"),
            None,
        ),
        d(
            &generation,
            "me@example.test",
            "claude-sonnet-5-5",
            Some("medium"),
            None,
        ),
        d(
            &alloc(1).allocation,
            "me@example.test",
            "claude-sonnet-5-5",
            Some("medium"),
            None,
        ),
    ] {
        assert_ne!(base, other);
    }
}

#[test]
fn only_the_exact_initiating_session_sees_the_url_or_controls_the_job() {
    let a = alloc(7);
    let (mut j, _rx) = job(&a, "session-token");
    j.stage = Stage::AwaitingCode;
    j.url = Some("https://claude.com/cai/oauth/authorize?x=y".into());
    assert!(j.owned_by(&a, "session-token"));
    assert!(j.view(true)["authorization_url"].is_string());
    assert!(!j.owned_by(&a, "other-session"));
    let relogin = SessionAllocation {
        allocation: a.allocation.clone(),
        epoch: 8,
    };
    assert!(!j.owned_by(&relogin, "session-token"));
    assert!(j.same_admin(&relogin) && !j.same_admin(&alloc(7)));
    assert!(j.view(false).get("authorization_url").is_none());
    assert!(!j.view(false).to_string().contains("session-token"));
}

#[test]
fn code_is_handed_over_once_and_a_lost_reply_never_resubmits() {
    let a = alloc(1);
    let (mut j, mut rx) = job(&a, "s");
    let code = || Zeroizing::new("abcdefgh1234".to_owned());
    assert!(matches!(j.accept_code(code()), CodeStep::NotReady));
    j.stage = Stage::AwaitingCode;
    assert!(matches!(j.accept_code(code()), CodeStep::Sent));
    assert_eq!(j.stage, Stage::Verifying);
    assert!(matches!(j.accept_code(code()), CodeStep::Replay));
    assert!(rx.try_recv().is_ok() && rx.try_recv().is_err());
    assert!(j.view(true).get("authorization_url").is_none());
}

#[test]
fn an_orphan_cleanup_job_holds_the_setup_slot_and_accepts_no_code() {
    let a = alloc(3);
    let id = Uuid::new_v4();
    let r = receipt_for(&a.allocation, id, Stage::AwaitingCode);
    let mut j = Job::orphan(&r, &a, "recovering-session");
    assert_eq!(
        (j.id, j.stage, j.after),
        (id, Stage::CleanupPending, Stage::Cancelled)
    );
    assert!(j.retiring && j.prepared.is_none() && j.code.is_none());
    assert!(!j.stage.terminal(), "it must block a concurrent start");
    assert!(matches!(
        j.accept_code(Zeroizing::new("abcdefgh1234".into())),
        CodeStep::Replay
    ));
    // Its receipt keeps the full owner and the original digest and inputs.
    let back = j.snapshot();
    assert!(back.owner_is(&a.allocation));
    assert_eq!(
        (back.digest.as_str(), back.state),
        ("digest", Stage::CleanupPending)
    );
    assert_eq!(back.model, r.model);
}

#[test]
fn input_shape_and_terms_are_strict() {
    let parse = |v: Value| serde_json::from_value::<Input>(v);
    let id = Uuid::new_v4();
    assert!(parse(json!({"op":"status","request_id":id,"extra":1})).is_err());
    let status = parse(json!({"op":"status","request_id":id,"code":"abcdefgh"})).unwrap();
    assert!(!status.well_formed());
    let code = parse(json!({"op":"code","request_id":id,"code":"abcdefgh"})).unwrap();
    assert!(code.well_formed());
    let start = parse(json!({"op":"start","request_id":id,"email":"a@b.co","model":"claude-x","hosting_terms_accepted":true})).unwrap();
    assert!(start.well_formed() && valid_email("a@b.co") && !valid_email("a b@c.co"));
    assert!(!valid_model("gpt-5") && !valid_effort("xhigh"));
}

fn start_input(id: Uuid, replace: Option<Uuid>) -> Input {
    Input {
        op: Op::Start,
        request_id: id,
        email: Some("owner@example.test".into()),
        model: Some("claude-sonnet-5-5".into()),
        effort: Some("medium".into()),
        hosting_terms_accepted: Some(true),
        replace_revision: replace,
        code: None,
    }
}
fn digest_for(a: &SessionAllocation, replace: Option<Uuid>) -> String {
    digest_of(
        &a.allocation,
        "owner@example.test",
        "claude-sonnet-5-5",
        Some("medium"),
        replace,
    )
}

#[tokio::test]
async fn start_refuses_bad_terms_models_and_a_stale_or_missing_replace_revision() {
    let (_keep, g, dir, d) = gateway();
    let a = alloc(1);
    let mut bad = start_input(Uuid::new_v4(), None);
    bad.hosting_terms_accepted = Some(false);
    assert_eq!(
        start(&g, &a, "s", bad).await.unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    let mut bad = start_input(Uuid::new_v4(), None);
    bad.model = Some("gpt-5".into());
    assert_eq!(
        start(&g, &a, "s", bad).await.unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    let id = Uuid::new_v4();
    assert_eq!(
        start(&g, &a, "s", start_input(id, Some(id)))
            .await
            .unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    // A connection exists: starting without its revision, or with a stale one,
    // must not replace it, and creates neither a job nor a receipt.
    let published = Uuid::new_v4();
    receipt::commit(&d, &config_for(&a.allocation, published), None).unwrap();
    for replace in [None, Some(Uuid::new_v4())] {
        let err = start(&g, &a, "s", start_input(Uuid::new_v4(), replace))
            .await
            .unwrap_err();
        assert_eq!(err, StatusCode::CONFLICT);
    }
    assert!(g.claude_jobs.lock().unwrap().is_empty());
    assert!(receipt::read_receipt(&d).unwrap().is_none());
    // And with no connection, naming a revision to replace is also a conflict.
    fs::remove_file(dir.join(CONFIG_FILE)).unwrap();
    let err = start(&g, &a, "s", start_input(Uuid::new_v4(), Some(published)))
        .await
        .unwrap_err();
    assert_eq!(err, StatusCode::CONFLICT);
}

#[tokio::test]
async fn duplicate_start_replays_only_for_the_same_session_and_input() {
    let (_keep, g, _dir, _d) = gateway();
    let a = alloc(1);
    let (mut live, _rx) = job(&a, "session-one");
    live.digest = digest_for(&a, None);
    let id = live.id;
    g.claude_jobs.lock().unwrap().push(live);
    let same = start(&g, &a, "session-one", start_input(id, None))
        .await
        .unwrap();
    assert_eq!(same["operation"]["request_id"], id.to_string());
    assert_eq!(same["operation"]["state"], "preparing");
    // Another session, other input, or another owner never replays it.
    assert_eq!(
        start(&g, &a, "session-two", start_input(id, None))
            .await
            .unwrap_err(),
        StatusCode::CONFLICT
    );
    let mut changed = start_input(id, None);
    changed.model = Some("claude-opus-4-1".into());
    assert_eq!(
        start(&g, &a, "session-one", changed).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        start(&g, &alloc(1), "session-one", start_input(id, None))
            .await
            .unwrap_err(),
        StatusCode::CONFLICT
    );
    // A second setup while one is unfinished is refused, even for the owner.
    assert_eq!(
        start(&g, &a, "session-one", start_input(Uuid::new_v4(), None))
            .await
            .unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(g.claude_jobs.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn after_a_restart_an_unresolved_receipt_blocks_start_until_cleanup() {
    let (_keep, g, _dir, d) = gateway();
    let a = alloc(1);
    let orphan = Uuid::new_v4();
    let mut r = receipt_for(&a.allocation, orphan, Stage::AwaitingCode);
    r.digest = digest_for(&a, None);
    receipt::write_receipt(&d, &r).unwrap();
    // A new request id is not allowed to overwrite it.
    assert_eq!(
        start(&g, &a, "s", start_input(Uuid::new_v4(), None))
            .await
            .unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().request_id,
        orphan
    );
    // The lost-reply retry of the same request only reads the interrupted state.
    let replay = start(
        &g,
        &a,
        "new-session-after-restart",
        start_input(orphan, None),
    )
    .await
    .unwrap();
    assert_eq!(state_of(&replay), "interrupted");
    // Another owner, or other input under the same id, conflicts.
    assert_eq!(
        start(&g, &alloc(1), "s", start_input(orphan, None))
            .await
            .unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        start(&g, &a, "s", start_input(orphan, Some(Uuid::new_v4())))
            .await
            .unwrap_err(),
        StatusCode::CONFLICT
    );
    // After a safe cancellation (terminal receipt) it is no longer unresolved,
    // so a new request id may begin (starting one here would reach the host,
    // which this fixture deliberately lacks; `begin` is the receipt gate).
    receipt::write_receipt(&d, &receipt_for(&a.allocation, orphan, Stage::Cancelled)).unwrap();
    assert!(!unfinished(&d));
    receipt::begin(
        &d,
        &receipt_for(&a.allocation, Uuid::new_v4(), Stage::Preparing),
    )
    .unwrap();
}

#[tokio::test]
async fn status_reports_only_to_the_exact_owner_and_reconciles_a_published_request() {
    let (_keep, g, _dir, d) = gateway();
    let a = alloc(1);
    let id = Uuid::new_v4();
    receipt::write_receipt(&d, &receipt_for(&a.allocation, id, Stage::Activating)).unwrap();
    let mine = status(&g, &a, "s", id).await.unwrap();
    assert_eq!(state_of(&mine), "interrupted");
    // Different tenant / volume / generation / account: invisible.
    for other in ["tenant", "volume", "generation", "account"] {
        let mut o = SessionAllocation {
            allocation: a.allocation.clone(),
            epoch: a.epoch,
        };
        match other {
            "tenant" => o.allocation.tenant_id = Uuid::new_v4().to_string(),
            "volume" => o.allocation.volume_id = Uuid::new_v4().to_string(),
            "generation" => o.allocation.generation += 1,
            _ => o.allocation.account_id = Uuid::new_v4().to_string(),
        }
        assert_eq!(
            status(&g, &o, "s", id).await.unwrap_err(),
            StatusCode::NOT_FOUND,
            "{other}"
        );
    }
    assert_eq!(
        status(&g, &a, "s", Uuid::new_v4()).await.unwrap_err(),
        StatusCode::NOT_FOUND
    );
    // A status poll cannot prove root publication or overwrite recovery intent.
    receipt::commit(&d, &config_for(&a.allocation, id), None).unwrap();
    let pending = status(&g, &a, "s", id).await.unwrap();
    assert_eq!(state_of(&pending), "cleanup_pending");
    assert!(pending["operation"].get("connection_revision").is_none());
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().state,
        Stage::Activating
    );
    assert!(unfinished(&d));
}

#[tokio::test]
async fn list_shows_the_connection_and_only_this_owners_reconciled_operation() {
    let (_keep, g, _dir, d) = gateway();
    let a = alloc(1);
    let id = Uuid::new_v4();
    let idle = list(&g, &a, "s").await.unwrap();
    assert_eq!(idle["saved"], false);
    assert!(idle["operation"].is_null());
    receipt::write_receipt(&d, &receipt_for(&a.allocation, id, Stage::Verifying)).unwrap();
    assert_eq!(
        list(&g, &a, "s").await.unwrap()["operation"]["state"],
        "interrupted"
    );
    assert!(list(&g, &alloc(1), "s").await.unwrap()["operation"].is_null());
    receipt::commit(&d, &config_for(&a.allocation, id), None).unwrap();
    let ready = list(&g, &a, "s").await.unwrap();
    assert_eq!(ready["saved"], true);
    assert_eq!(ready["connection_revision"], id.to_string());
    assert_eq!(ready["operation"]["state"], "cleanup_pending");
}

#[tokio::test]
async fn orphan_cleanup_is_refused_when_not_safe_and_never_calls_the_host() {
    let (_keep, g, dir, d) = gateway();
    let a = alloc(1);
    let id = Uuid::new_v4();
    receipt::write_receipt(&d, &receipt_for(&a.allocation, id, Stage::AwaitingCode)).unwrap();
    // Another owner cannot even learn the receipt exists.
    assert_eq!(
        cancel(&g, &alloc(1), "s", id).await.unwrap_err(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        cancel(&g, &a, "s", Uuid::new_v4()).await.unwrap_err(),
        StatusCode::NOT_FOUND
    );
    // A config naming this request with other content: cleanup stays pending.
    let mut mismatch: Value = serde_json::from_slice(&config_for(&a.allocation, id)).unwrap();
    mismatch["model"] = json!("claude-opus-4-1");
    fs::write(
        dir.join(CONFIG_FILE),
        serde_json::to_vec(&mismatch).unwrap(),
    )
    .unwrap();
    fs::set_permissions(dir.join(CONFIG_FILE), fs::Permissions::from_mode(0o600)).unwrap();
    let held = cancel(&g, &a, "s", id).await.unwrap();
    assert_eq!(state_of(&held), "cleanup_pending");
    // An unreadable config is not absent either.
    fs::set_permissions(dir.join(CONFIG_FILE), fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        state_of(&cancel(&g, &a, "s", id).await.unwrap()),
        "cleanup_pending"
    );
    assert!(
        g.claude_jobs.lock().unwrap().is_empty(),
        "no retirement was started"
    );
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().state,
        Stage::AwaitingCode
    );
    // Exact publication remains pending for authenticated root reconciliation.
    fs::remove_file(dir.join(CONFIG_FILE)).unwrap();
    receipt::commit(&d, &config_for(&a.allocation, id), None).unwrap();
    assert_eq!(
        state_of(&status(&g, &a, "s", id).await.unwrap()),
        "cleanup_pending"
    );
    assert!(g.claude_jobs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn orphan_cleanup_waits_for_a_running_setup_and_terminal_receipts_are_idempotent() {
    let (_keep, g, _dir, d) = gateway();
    let a = alloc(1);
    let id = Uuid::new_v4();
    receipt::write_receipt(&d, &receipt_for(&a.allocation, id, Stage::Verifying)).unwrap();
    let (running, _rx) = job(&alloc(9), "someone-else");
    g.claude_jobs.lock().unwrap().push(running);
    // Global exclusion: no cleanup (and so no host call) next to a running setup.
    assert_eq!(
        cancel(&g, &a, "s", id).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(g.claude_jobs.lock().unwrap().len(), 1);
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().state,
        Stage::Verifying
    );
    g.claude_jobs.lock().unwrap().clear();
    // A finished (cancelled) receipt just reports itself.
    receipt::write_receipt(&d, &receipt_for(&a.allocation, id, Stage::Cancelled)).unwrap();
    assert_eq!(
        state_of(&cancel(&g, &a, "s", id).await.unwrap()),
        "cancelled"
    );
    assert!(g.claude_jobs.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cleanup_takeover_is_for_the_exact_owner_and_rebinds_the_session() {
    let (_keep, g, _dir, _d) = gateway();
    let a = alloc(4);
    let (mut pending, _rx) = job(&a, "first-session");
    pending.stage = Stage::CleanupPending;
    pending.retiring = true; // a retirement is already running: nothing is spawned
    let id = pending.id;
    g.claude_jobs.lock().unwrap().push(pending);
    // Another owner learns nothing.
    assert_eq!(
        cancel(&g, &alloc(4), "s", id).await.unwrap_err(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status(&g, &alloc(4), "s", id).await.unwrap_err(),
        StatusCode::NOT_FOUND
    );
    // The same owner on a new session and epoch takes over the cleanup only.
    let relogin = SessionAllocation {
        allocation: a.allocation.clone(),
        epoch: 5,
    };
    let view = cancel(&g, &relogin, "second-session", id).await.unwrap();
    assert_eq!(state_of(&view), "cleanup_pending");
    {
        let jobs = g.claude_jobs.lock().unwrap();
        assert!(jobs[0].owned_by(&relogin, "second-session"));
        assert!(!jobs[0].owned_by(&a, "first-session"));
    }
    // A running (non-cleanup) login cannot be cancelled by a non-initiating session.
    g.claude_jobs.lock().unwrap().clear();
    let (running, _rx2) = job(&a, "first-session");
    let rid = running.id;
    g.claude_jobs.lock().unwrap().push(running);
    assert_eq!(
        cancel(&g, &relogin, "second-session", rid)
            .await
            .unwrap_err(),
        StatusCode::NOT_FOUND
    );
    assert!(!g.claude_jobs.lock().unwrap()[0].cancel.is_cancelled());
    cancel(&g, &a, "first-session", rid).await.unwrap();
    assert!(g.claude_jobs.lock().unwrap()[0].cancel.is_cancelled());
}

#[tokio::test]
async fn a_committing_job_cannot_be_cancelled() {
    let (_keep, g, _dir, _d) = gateway();
    let a = alloc(1);
    let (mut j, _rx) = job(&a, "s");
    j.committing = true;
    let id = j.id;
    g.claude_jobs.lock().unwrap().push(j);
    cancel(&g, &a, "s", id).await.unwrap();
    assert!(!g.claude_jobs.lock().unwrap()[0].cancel.is_cancelled());
}

#[tokio::test]
async fn polls_from_any_owner_and_unknown_request_never_finalize_live_root_pending() {
    let (_keep, g, _dir, d) = gateway();
    let a = alloc(1);
    let (mut live, _rx) = job(&a, "initiator");
    live.stage = Stage::CleanupPending;
    live.config_sha256 = Some(receipt::sha(&config_for(&a.allocation, live.id)));
    let id = live.id;
    receipt::write_receipt(&d, &live.snapshot()).unwrap();
    receipt::commit(&d, &config_for(&a.allocation, id), None).unwrap();
    g.claude_jobs.lock().unwrap().push(live);
    assert!(status(&g, &a, "initiator", Uuid::new_v4()).await.is_err());
    assert!(list(&g, &alloc(2), "foreign").await.unwrap()["operation"].is_null());
    assert_eq!(
        list(&g, &a, "initiator").await.unwrap()["operation"]["state"],
        "cleanup_pending"
    );
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().state,
        Stage::CleanupPending
    );
    assert!(unfinished(&d));
}

#[test]
fn durability_confirmation_requires_the_exact_private_config() {
    let (_t, p, d) = private_dir();
    let a = alloc(1).allocation;
    let bytes = config_for(&a, Uuid::new_v4());
    receipt::commit(&d, &bytes, None).unwrap();
    assert!(receipt::confirm_durable(&d, &bytes).is_ok());
    assert!(receipt::confirm_durable(&d, b"other").is_err());
    fs::set_permissions(p.join(CONFIG_FILE), fs::Permissions::from_mode(0o644)).unwrap();
    assert!(receipt::confirm_durable(&d, &bytes).is_err());
}

#[tokio::test]
async fn terminal_receipt_failure_keeps_the_slot_and_blocks_api_replacement() {
    let (_keep, g, dir, d) = gateway();
    let a = alloc(1);
    let (mut j, _rx) = job(&a, "original");
    j.stage = Stage::Activating;
    let id = j.id;
    receipt::write_receipt(&d, &j.snapshot()).unwrap();
    g.claude_jobs.lock().unwrap().push(j);
    // A directory at the pending filename makes every atomic save fail.
    fs::create_dir(dir.join("claude-subscription-operation.pending")).unwrap();
    finish(&g, id, Some(Stage::Ready)).await;
    assert_eq!(
        g.claude_jobs.lock().unwrap()[0].stage,
        Stage::CleanupPending
    );
    assert!(ensure_settled(&d).is_err());
    let i = super::super::Input {
        request_id: Uuid::new_v4(),
        api_key: Zeroizing::new("synthetic-key".into()),
        model: "claude-synthetic".into(),
        effort: Some("medium".into()),
        replace_revision: None,
    };
    let c = config(&dir, &i).unwrap();
    assert!(
        save(
            &d,
            &c,
            &i.key_name(),
            i.api_key.as_bytes(),
            i.request_id,
            None
        )
        .is_err()
    );
    assert!(!dir.join(CONFIG_FILE).exists());
    fs::remove_dir(dir.join("claude-subscription-operation.pending")).unwrap();
    finish(&g, id, Some(Stage::Failed)).await;
    assert!(ensure_settled(&d).is_ok());
}

#[tokio::test]
async fn late_code_persist_cannot_regress_terminal_receipt_before_memory_update() {
    let (_keep, g, _dir, d) = gateway();
    let a = alloc(1);
    let (mut live, _rx) = job(&a, "initiator");
    live.stage = Stage::Verifying;
    let id = live.id;
    g.claude_jobs.lock().unwrap().push(live);
    receipt::write_receipt(&d, &receipt_for(&a.allocation, id, Stage::Ready)).unwrap();
    assert!(persist(&g, id).await);
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().state,
        Stage::Ready
    );
    g.claude_jobs.lock().unwrap()[0].stage = Stage::Failed;
    assert!(persist(&g, id).await);
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().state,
        Stage::Ready
    );
}

// ---- explicit administrator disconnect -------------------------------------

fn disconnect_receipt(a: &Allocation, id: Uuid, target: Uuid, state: Stage) -> Receipt {
    let mut r = receipt_for(a, id, state);
    r.version = Mode::Disconnect.version();
    r.digest = lifecycle::digest_of(a, target);
    r.config_sha256 = None;
    r.mode = Mode::Disconnect;
    r.target_revision = Some(target);
    r
}
fn disconnect_input(id: Uuid, target: Uuid) -> Input {
    Input {
        op: Op::Disconnect,
        request_id: id,
        email: None,
        model: None,
        effort: None,
        hosting_terms_accepted: None,
        replace_revision: Some(target),
        code: None,
    }
}

#[tokio::test]
async fn delayed_persist_preserves_the_durable_disconnect_intent() {
    let (_keep, g, _dir, d) = gateway();
    let a = alloc(1);
    let target = Uuid::new_v4();
    let j = Job::disconnect(
        Uuid::new_v4(),
        &a,
        "session",
        lifecycle::digest_of(&a.allocation, target),
        target,
        ("claude-sonnet-5-5".into(), Some("medium".into())),
    );
    let id = j.id;
    let mut disk = j.snapshot();
    disk.config_sha256 = Some(receipt::sha(&config_for(&a.allocation, target)));
    receipt::write_receipt(&d, &disk).unwrap();
    // Root has the durable intent, but the drive has not yet mirrored it in RAM.
    g.claude_jobs.lock().unwrap().push(j);
    assert!(persist(&g, id).await);
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().config_sha256,
        disk.config_sha256
    );
    g.claude_jobs.lock().unwrap()[0].config_sha256 = Some("changed".into());
    assert!(!persist(&g, id).await);
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().config_sha256,
        disk.config_sha256
    );
}

#[tokio::test]
async fn broken_config_keeps_only_the_owners_pending_recovery_visible() {
    let (_keep, g, dir, d) = gateway();
    let a = alloc(1);
    let r = disconnect_receipt(
        &a.allocation,
        Uuid::new_v4(),
        Uuid::new_v4(),
        Stage::Disconnecting,
    );
    receipt::write_receipt(&d, &r).unwrap();
    receipt::commit(&d, b"broken config", None).unwrap();
    for unreadable in [false, true] {
        if unreadable {
            fs::set_permissions(dir.join(CONFIG_FILE), fs::Permissions::from_mode(0o644)).unwrap();
        }
        let view = list(&g, &a, "new session").await.unwrap();
        assert_eq!(view["operation"]["request_id"], r.request_id.to_string());
        assert_eq!(view["operation"]["state"], "cleanup_pending");
        assert_eq!(view["saved"], false);
        assert_eq!(
            list(&g, &alloc(1), "stranger").await.unwrap_err(),
            UNAVAILABLE
        );
        assert!(ensure_settled(&d).is_err());
    }
    assert_eq!(
        receipt::read_receipt(&d).unwrap().unwrap().state,
        Stage::Disconnecting
    );
}

#[test]
fn disconnect_persists_intent_then_unpublishes_durably_before_the_root_begins_retirement() {
    use lifecycle::Unpub;
    let (_t, p, d) = private_dir();
    let a = alloc(1).allocation;
    let owner = owner_of(&a).unwrap();
    let (target, op) = (Uuid::new_v4(), Uuid::new_v4());
    let config = config_for(&a, target);
    receipt::commit(&d, &config, None).unwrap();
    let reserved = disconnect_receipt(&a, op, target, Stage::Disconnecting);
    receipt::begin(&d, &reserved).unwrap();
    let at_root = std::cell::RefCell::new(None);
    let report = lifecycle::unpublish(&d, &owner, &reserved, || {
        // What the root sees: config already gone AND the intent already durable.
        *at_root.borrow_mut() = Some((
            p.join(CONFIG_FILE).exists(),
            receipt::read_receipt(&d).unwrap().unwrap().config_sha256,
        ));
        Ok(())
    });
    assert_eq!(report.result.unwrap(), Unpub::Begun);
    let (still_published, intent) = at_root.into_inner().unwrap();
    assert!(!still_published);
    assert_eq!(intent.as_deref(), Some(receipt::sha(&config).as_str()));
    assert_eq!(report.intent, intent);
    // Unpublished but not yet acknowledged: the slot and API replacement stay blocked.
    assert!(unfinished(&d));
    let mut done = receipt::read_receipt(&d).unwrap().unwrap();
    assert_eq!(done.target_revision, Some(target));
    assert_ne!(
        done.request_id, target,
        "operation id is not the connection"
    );
    done.state = Stage::Disconnected;
    receipt::write_receipt(&d, &done).unwrap();
    assert!(!unfinished(&d));
    // The owner may now start a fresh own-subscription setup (receipt gate only).
    receipt::begin(&d, &receipt_for(&a, Uuid::new_v4(), Stage::Preparing)).unwrap();
}

#[test]
fn a_failed_root_begin_or_lost_reply_resumes_from_the_receipt_without_republishing() {
    use lifecycle::Unpub;
    let (_t, p, d) = private_dir();
    let a = alloc(1).allocation;
    let owner = owner_of(&a).unwrap();
    let (target, op) = (Uuid::new_v4(), Uuid::new_v4());
    receipt::commit(&d, &config_for(&a, target), None).unwrap();
    let reserved = disconnect_receipt(&a, op, target, Stage::Disconnecting);
    receipt::begin(&d, &reserved).unwrap();
    // Root busy: the config is already unpublished, the intent durable, nothing ends.
    let first = lifecycle::unpublish(&d, &owner, &reserved, || Err("busy".into()));
    assert!(first.result.is_err() && first.intent.is_some());
    assert!(!p.join(CONFIG_FILE).exists());
    // Process restart: a new session resumes from the receipt as stored.
    let stored = receipt::read_receipt(&d).unwrap().unwrap();
    assert_eq!(stored.config_sha256, first.intent);
    let calls = std::cell::Cell::new(0);
    for _ in 0..2 {
        // The second pass is a terminal-receipt write that failed after the root ack.
        let again = lifecycle::unpublish(&d, &owner, &stored, || {
            calls.set(calls.get() + 1);
            Ok(())
        });
        assert_eq!(again.result.unwrap(), Unpub::Begun);
        assert!(again.intent.is_none(), "the intent is never rewritten");
    }
    assert_eq!(calls.get(), 2);
    assert!(unfinished(&d));
    // Another owner's pin can never drive this receipt.
    let stranger = owner_of(&alloc(1).allocation).unwrap();
    let refused = lifecycle::unpublish(&d, &stranger, &stored, || panic!("never begins"));
    assert!(refused.result.is_err());
}

#[test]
fn disconnect_never_touches_a_config_that_is_not_the_exact_owned_target() {
    use lifecycle::Unpub;
    let a = alloc(1).allocation;
    let owner = owner_of(&a).unwrap();
    let (target, op) = (Uuid::new_v4(), Uuid::new_v4());
    let exact = config_for(&a, target);
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut v: Value = serde_json::from_slice(&exact).unwrap();
        f(&mut v);
        serde_json::to_vec(&v).unwrap()
    };
    let api_key = edit(&|v| v["subscription"] = json!(null));
    let two = edit(&|v| {
        let first = v["bindings"][0].clone();
        v["bindings"].as_array_mut().unwrap().push(first);
    });
    let changed = edit(&|v| v["model"] = json!("claude-opus-4-1"));
    let digest_of_exact = Some(receipt::sha(&exact));
    let cases: Vec<(Vec<u8>, Option<String>)> = vec![
        (config_for(&alloc(1).allocation, target), None), // another owner's subscription
        (api_key, None),
        (two, None),
        (b"not json".to_vec(), None),
        (changed, digest_of_exact), // edited after the intent was recorded
    ];
    for (bytes, intent) in cases {
        let (_t, p, d) = private_dir();
        receipt::commit(&d, &bytes, None).unwrap();
        let mut r = disconnect_receipt(&a, op, target, Stage::Disconnecting);
        r.config_sha256 = intent;
        receipt::begin(&d, &r).unwrap();
        let report = lifecycle::unpublish(&d, &owner, &r, || panic!("must not begin"));
        assert_eq!(report.result.unwrap(), Unpub::Hold);
        assert_eq!(fs::read(p.join(CONFIG_FILE)).unwrap(), bytes);
        assert_eq!(
            receipt::read_receipt(&d).unwrap().unwrap().config_sha256,
            r.config_sha256
        );
    }
    // Unreadable is an error, never absence: nothing is unlinked or retired.
    let (_t, p, d) = private_dir();
    receipt::commit(&d, &exact, None).unwrap();
    fs::set_permissions(p.join(CONFIG_FILE), fs::Permissions::from_mode(0o644)).unwrap();
    let r = disconnect_receipt(&a, op, target, Stage::Disconnecting);
    receipt::begin(&d, &r).unwrap();
    assert!(
        lifecycle::unpublish(&d, &owner, &r, || panic!("must not begin"))
            .result
            .is_err()
    );
    assert!(p.join(CONFIG_FILE).exists());
    assert!(
        receipt::read_receipt(&d)
            .unwrap()
            .unwrap()
            .config_sha256
            .is_none()
    );
}

#[test]
fn an_absent_or_superseded_target_confirms_and_begins_without_deleting_anything_else() {
    use lifecycle::Unpub;
    let a = alloc(1).allocation;
    let owner = owner_of(&a).unwrap();
    let (target, op) = (Uuid::new_v4(), Uuid::new_v4());
    let mut r = disconnect_receipt(&a, op, target, Stage::CleanupPending);
    r.config_sha256 = Some("recorded-intent".into());
    // Definitely absent.
    let (_t, _p, d) = private_dir();
    receipt::begin(&d, &r).unwrap();
    let calls = std::cell::Cell::new(0);
    let report = lifecycle::unpublish(&d, &owner, &r, || {
        calls.set(calls.get() + 1);
        Ok(())
    });
    assert_eq!(report.result.unwrap(), Unpub::Begun);
    assert_eq!(calls.get(), 1);
    // Superseded by a different revision: the root is asked (it verifies the
    // exact owner and request itself) but the other config is left untouched.
    let (_t2, p2, d2) = private_dir();
    let other = config_for(&a, Uuid::new_v4());
    receipt::commit(&d2, &other, None).unwrap();
    receipt::begin(&d2, &r).unwrap();
    let report = lifecycle::unpublish(&d2, &owner, &r, || {
        calls.set(calls.get() + 1);
        Ok(())
    });
    assert_eq!(report.result.unwrap(), Unpub::Begun);
    assert_eq!(fs::read(p2.join(CONFIG_FILE)).unwrap(), other);
    assert_eq!(calls.get(), 2);
    // A reservation without the registry-locked exact-config intent may not
    // infer retirement authority from absence or another revision.
    r.config_sha256 = None;
    for dir in [&d, &d2] {
        let held = lifecycle::unpublish(dir, &owner, &r, || panic!("unproven intent"));
        assert_eq!(held.result.unwrap(), Unpub::Hold);
    }
}

#[test]
fn disconnect_receipts_are_v3_while_v2_connect_receipts_stay_readable_and_strict() {
    let (_t, p, d) = private_dir();
    let a = alloc(1).allocation;
    let path = p.join("claude-subscription-operation.json");
    // A public v2 connect receipt carries none of the new keys and still reads.
    let ready = receipt_for(&a, Uuid::new_v4(), Stage::Ready);
    let raw: Value = serde_json::to_value(&ready).unwrap();
    assert!(raw.get("mode").is_none() && raw.get("target_revision").is_none());
    receipt::write_receipt(&d, &ready).unwrap();
    let back = receipt::read_receipt(&d).unwrap().unwrap();
    assert_eq!(
        (back.version, back.mode, back.target_revision),
        (2, Mode::Connect, None)
    );
    // A disconnect receipt is v3, names its target, and holds no secret or URL.
    let target = Uuid::new_v4();
    let mine = disconnect_receipt(&a, Uuid::new_v4(), target, Stage::Disconnecting);
    receipt::write_receipt(&d, &mine).unwrap();
    let text = String::from_utf8(fs::read(&path).unwrap()).unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(file["version"], 3);
    assert_eq!(file["mode"], "disconnect");
    assert_eq!(file["target_revision"], target.to_string());
    assert!(!text.contains("http") && file.get("email").is_none());
    // Inconsistent combinations are refused on read and on write, never half-trusted.
    let tamper = |f: &dyn Fn(&mut Value)| {
        let mut v = file.clone();
        f(&mut v);
        fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        receipt::read_receipt(&d).is_err()
    };
    assert!(tamper(&|v| v["version"] = json!(2)));
    assert!(tamper(&|v| v["mode"] = json!("connect")));
    assert!(tamper(&|v| v["state"] = json!("ready")));
    assert!(tamper(&|v| v["replace_revision"] = json!(Uuid::new_v4())));
    assert!(tamper(&|v| v["version"] = json!(4)));
    let mut wrong = receipt_for(&a, Uuid::new_v4(), Stage::Disconnected);
    assert!(receipt::write_receipt(&d, &wrong).is_err());
    wrong = disconnect_receipt(&a, Uuid::new_v4(), target, Stage::Ready);
    assert!(receipt::write_receipt(&d, &wrong).is_err());
    // States: a stored disconnect always resumes (never "interrupted"); the terminal one is final.
    for live in [Stage::Disconnecting, Stage::CleanupPending] {
        let r = disconnect_receipt(&a, Uuid::new_v4(), target, live);
        for rel in [
            Relation::Absent,
            Relation::Other,
            Relation::Exact,
            Relation::Unreadable,
        ] {
            assert_eq!(r.effective(rel), Stage::CleanupPending);
        }
    }
    let end =
        disconnect_receipt(&a, Uuid::new_v4(), target, Stage::Disconnected).view(Relation::Absent);
    assert_eq!(
        (end["state"].as_str(), end["kind"].as_str()),
        (Some("disconnected"), Some("disconnect"))
    );
    assert!(end.get("connection_revision").is_none());
}

#[tokio::test]
async fn reservation_needs_the_exact_owned_target_and_replays_only_the_same_operation() {
    let (_keep, g, dir, d) = gateway();
    let a = alloc(1);
    let (target, op) = (Uuid::new_v4(), Uuid::new_v4());
    let go = |a: &SessionAllocation, s: &str, op: Uuid, t: Uuid| {
        let (g, a, s) = (
            g.clone(),
            SessionAllocation {
                allocation: a.allocation.clone(),
                epoch: a.epoch,
            },
            s.to_owned(),
        );
        async move { lifecycle::reserve(&g, &a, &s, disconnect_input(op, t)).await }
    };
    assert_eq!(
        go(&a, "s", op, op).await.unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    // No connection, another owner's subscription, or a stale target: nothing is reserved.
    assert_eq!(
        go(&a, "s", op, target).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    receipt::commit(&d, &config_for(&alloc(1).allocation, target), None).unwrap();
    assert_eq!(
        go(&a, "s", op, target).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    fs::remove_file(dir.join(CONFIG_FILE)).unwrap();
    receipt::commit(&d, &config_for(&a.allocation, target), None).unwrap();
    assert_eq!(
        go(&a, "s", op, Uuid::new_v4()).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    assert!(g.claude_jobs.lock().unwrap().is_empty());
    assert!(receipt::read_receipt(&d).unwrap().is_none());
    // The exact owner and target reserve the slot and a durable receipt; no config changes.
    let before = fs::read(dir.join(CONFIG_FILE)).unwrap();
    let (view, fresh) = go(&a, "s", op, target).await.unwrap();
    assert!(fresh);
    assert_eq!(state_of(&view), "disconnecting");
    assert_eq!(view["operation"]["kind"], "disconnect");
    let r = receipt::read_receipt(&d).unwrap().unwrap();
    assert_eq!(
        (r.mode, r.request_id, r.target_revision),
        (Mode::Disconnect, op, Some(target))
    );
    assert!(r.config_sha256.is_none() && r.replace_revision.is_none());
    assert_eq!(fs::read(dir.join(CONFIG_FILE)).unwrap(), before);
    assert!(unfinished(&d));
    // Exact replay is idempotent; another session, target, owner or a new id is refused.
    let (_, again) = go(&a, "s", op, target).await.unwrap();
    assert!(!again);
    assert_eq!(
        go(&a, "other-session", op, target).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        go(&a, "s", op, Uuid::new_v4()).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        go(&alloc(1), "s", op, target).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        go(&a, "s", Uuid::new_v4(), target).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(g.claude_jobs.lock().unwrap().len(), 1);
    // After a process restart the same owner's replay only reads the receipt
    // (resume is the explicit cleanup request); a new id still may not overwrite it.
    g.claude_jobs.lock().unwrap().clear();
    let (restored, fresh) = go(&a, "new-session", op, target).await.unwrap();
    assert!(!fresh);
    assert_eq!(state_of(&restored), "cleanup_pending");
    assert_eq!(
        go(&alloc(1), "s", op, target).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        go(&a, "s", Uuid::new_v4(), target).await.unwrap_err(),
        StatusCode::CONFLICT
    );
    assert_eq!(receipt::read_receipt(&d).unwrap().unwrap().request_id, op);
}

#[tokio::test]
async fn a_disconnect_is_never_cancelled_and_only_the_exact_owner_resumes_it() {
    let (_keep, g, _dir, _d) = gateway();
    let a = alloc(4);
    let target = Uuid::new_v4();
    let make = |id: Uuid| {
        Job::disconnect(
            id,
            &a,
            "first-session",
            lifecycle::digest_of(&a.allocation, target),
            target,
            ("claude-sonnet-5-5".into(), Some("medium".into())),
        )
    };
    let running = make(Uuid::new_v4());
    let id = running.id;
    g.claude_jobs.lock().unwrap().push(running);
    let relogin = SessionAllocation {
        allocation: a.allocation.clone(),
        epoch: 5,
    };
    // Neither the initiating nor another session can cancel it; a stranger sees nothing.
    for (who, session) in [(&a, "first-session"), (&relogin, "second-session")] {
        let view = cancel(&g, who, session, id).await.unwrap();
        assert_eq!(state_of(&view), "disconnecting");
    }
    assert_eq!(
        cancel(&g, &alloc(4), "s", id).await.unwrap_err(),
        StatusCode::NOT_FOUND
    );
    assert!(!g.claude_jobs.lock().unwrap()[0].cancel.is_cancelled());
    // A stuck one (drive stopped) is taken over by the same owner's new session only.
    {
        let mut jobs = g.claude_jobs.lock().unwrap();
        jobs[0].stage = Stage::CleanupPending;
        jobs[0].retiring = true; // a resume is already running: nothing is spawned here
    }
    let view = cancel(&g, &relogin, "second-session", id).await.unwrap();
    assert_eq!(state_of(&view), "cleanup_pending");
    assert!(g.claude_jobs.lock().unwrap()[0].owned_by(&relogin, "second-session"));
    // A restart's orphan job keeps the OLD target and never adopts the operation id as it.
    let r = disconnect_receipt(&a.allocation, Uuid::new_v4(), target, Stage::Disconnecting);
    let orphan = Job::orphan(&r, &relogin, "recovering");
    let back = orphan.snapshot();
    assert_eq!(
        (orphan.mode, orphan.after, orphan.stage),
        (Mode::Disconnect, Stage::Disconnected, Stage::CleanupPending)
    );
    assert_eq!(
        (back.target_revision, back.request_id),
        (Some(target), r.request_id)
    );
    assert_eq!(back.version, 3);
}

#[tokio::test]
async fn a_lost_terminal_receipt_keeps_the_disconnect_resumable_and_blocks_new_setup() {
    let (_keep, g, dir, d) = gateway();
    let a = alloc(1);
    let target = Uuid::new_v4();
    let mut j = Job::disconnect(
        Uuid::new_v4(),
        &a,
        "s",
        lifecycle::digest_of(&a.allocation, target),
        target,
        ("claude-sonnet-5-5".into(), Some("medium".into())),
    );
    j.config_sha256 = Some("intent".into());
    let id = j.id;
    receipt::write_receipt(&d, &j.snapshot()).unwrap();
    g.claude_jobs.lock().unwrap().push(j);
    // The root acknowledged but the terminal receipt cannot be written.
    fs::create_dir(dir.join("claude-subscription-operation.pending")).unwrap();
    finish(&g, id, Some(Stage::Disconnected)).await;
    assert_eq!(
        g.claude_jobs.lock().unwrap()[0].stage,
        Stage::CleanupPending
    );
    assert!(ensure_settled(&d).is_err());
    let stored = receipt::read_receipt(&d).unwrap().unwrap();
    assert_eq!(
        (stored.state, stored.config_sha256.as_deref()),
        (Stage::Disconnecting, Some("intent"))
    );
    assert_eq!(
        start(&g, &a, "s", start_input(Uuid::new_v4(), None))
            .await
            .unwrap_err(),
        StatusCode::CONFLICT
    );
    // The resumed pass ends it: terminal receipt, then the owner sees the final state.
    fs::remove_dir(dir.join("claude-subscription-operation.pending")).unwrap();
    finish(&g, id, Some(Stage::Disconnected)).await;
    assert_eq!(g.claude_jobs.lock().unwrap()[0].stage, Stage::Disconnected);
    assert!(ensure_settled(&d).is_ok());
    let done = status(&g, &a, "s", id).await.unwrap();
    assert_eq!(state_of(&done), "disconnected");
    assert!(done["operation"].get("connection_revision").is_none());
    assert_eq!(list(&g, &a, "s").await.unwrap()["saved"], false);
}

#[test]
fn input_shape_for_disconnect_takes_only_the_operation_and_target() {
    let id = Uuid::new_v4();
    let parse = |v: Value| serde_json::from_value::<Input>(v).unwrap();
    assert!(
        parse(json!({"op":"disconnect","request_id":id,"replace_revision":Uuid::new_v4()}))
            .well_formed()
    );
    assert!(!parse(json!({"op":"disconnect","request_id":id})).well_formed());
    assert!(!parse(json!({"op":"disconnect","request_id":id,"replace_revision":Uuid::new_v4(),"email":"a@b.co"})).well_formed());
    assert!(!parse(json!({"op":"disconnect","request_id":id,"replace_revision":Uuid::new_v4(),"code":"abcdefgh"})).well_formed());
}

#[test]
fn api_replacement_cannot_abandon_a_published_subscription_worker() {
    let (_keep, dir, d) = private_dir();
    let a = alloc(1);
    let id = Uuid::new_v4();
    let current = config_for(&a.allocation, id);
    receipt::commit(&d, &current, None).unwrap();
    receipt::write_receipt(&d, &receipt_for(&a.allocation, id, Stage::Ready)).unwrap();
    let i = super::super::Input {
        request_id: Uuid::new_v4(),
        api_key: Zeroizing::new("synthetic-key".into()),
        model: "claude-synthetic".into(),
        effort: Some("medium".into()),
        replace_revision: Some(id),
    };
    let next = config(&dir, &i).unwrap();
    assert_eq!(
        save(
            &d,
            &next,
            &i.key_name(),
            i.api_key.as_bytes(),
            i.request_id,
            Some(id)
        )
        .unwrap_err()
        .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(&*read(&d, CONFIG_FILE).unwrap().unwrap(), &current);
    assert!(!dir.join(i.key_name()).exists());
}
