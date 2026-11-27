use super::*;
use crate::runtime_registry::activate_invitation as runtime_registry_operator_activate;
use std::path::PathBuf;

fn root() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    fs::set_permissions(
        d.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let p = d.path().canonicalize().unwrap();
    (d, p)
}
fn view(a: &Allocation, email: &str) -> SessionView {
    SessionView {
        user: moosuri_core::AccountSummary {
            id: a.account_id.clone(),
            name: "Synthetic member".into(),
            email: email.into(),
            role: Role::Member,
            must_change_password: false,
        },
        password_change_required: false,
        chat_draft_scope: None,
    }
}
fn allocate(r: &Path, email: &str) -> Allocation {
    let a = reserve(r, Uuid::new_v4(), email.into()).unwrap();
    set_routing(r, &a, true).unwrap();
    a
}
fn setup() -> (tempfile::TempDir, PathBuf, Allocation, SessionView) {
    let (d, r) = root();
    let a = allocate(&r, "a@example.test");
    let v = view(&a, "a@example.test");
    bootstrap(&r, Uuid::new_v4(), &a, &v).unwrap();
    (d, r, a, v)
}
fn session(r: &Path, a: &Allocation, v: &SessionView) -> AdminSession {
    checked_session(r, a, v).unwrap()
}
fn invite(r: &Path, a: &Allocation, v: &SessionView, email: &str) -> Invitation {
    reserve_invitation(
        r,
        &session(r, a, v),
        Uuid::new_v4(),
        "Guest".into(),
        email.into(),
    )
    .unwrap()
}
fn alloc(i: &Invitation) -> Allocation {
    allocation_of(i)
}
fn raw(n: u8) -> Vec<u8> {
    (0..32u8)
        .map(|i| i.wrapping_mul(7).wrapping_add(n))
        .collect()
}
fn token(n: u8) -> String {
    URL_SAFE_NO_PAD.encode(raw(n))
}
fn uid(a: &Allocation) -> Uuid {
    Uuid::parse_str(&a.account_id).unwrap()
}
fn change(r: &Path, s: &AdminSession, target: &Allocation, c: Change) -> Result<Member> {
    let m = status(r, uid(target)).unwrap();
    change_member(
        r,
        s,
        Uuid::new_v4(),
        uid(target),
        target.generation,
        m.role_revision,
        c,
    )
}
fn add_admin(r: &Path, a: &Allocation, av: &SessionView) -> (Allocation, SessionView) {
    let b = allocate(r, "b@example.test");
    let bv = view(&b, "b@example.test");
    let bm = observe_session(r, &b, &bv).unwrap();
    change_member(
        r,
        &session(r, a, av),
        Uuid::new_v4(),
        uid(&b),
        1,
        bm.role_revision,
        Change::Promote,
    )
    .unwrap();
    (b, bv)
}
fn unauthorized<T>(r: Result<T>) -> bool {
    matches!(r, Err(StoreError::Unauthorized))
}

#[test]
fn issue_redeem_exact_identity_and_ready_replay_is_inert() {
    let (_d, r, a, av) = setup();
    let i = invite(&r, &a, &av, "guest@example.test");
    let t = token(1);
    let link = issue(&r, &session(&r, &a, &av), i.request_id, &t).unwrap();
    assert_eq!(link.invitation, i);
    assert!(!link.ready);
    assert_eq!(inspect(&r, &t).unwrap(), link);
    let al = alloc(&i);
    assert!(requires_link(&r, i.request_id).unwrap());
    assert!(with_unlinked(&r, i.request_id, &al, || panic!("legacy initializer ran")).is_err());
    assert!(runtime_registry_operator_activate(&r, i.request_id, &al).is_err());
    assert_eq!(with_pending(&r, &t, &al, || 7).unwrap(), 7);
    let mut wrong = al.clone();
    wrong.generation = 2;
    let mut called = false;
    assert!(with_pending(&r, &t, &wrong, || called = true).is_err());
    assert!(!called);
    activate(&r, &t, &al).unwrap();
    activate(&r, &t, &al).unwrap();
    assert!(inspect(&r, &t).unwrap().ready);
    assert_eq!(lookup_ready(&r, "guest@example.test".into()).unwrap(), al);
    // A used link can report, but never write a password or be reissued.
    assert!(with_pending(&r, &t, &al, || called = true).is_err());
    assert!(!called);
    assert!(issue(&r, &session(&r, &a, &av), i.request_id, &token(2)).is_err());
    assert!(inspect(&r, &token(2)).is_err());
    // TTL still bounds the ready report.
    let expires = inspect(&r, &t).unwrap().expires_at;
    assert!(unauthorized(inspect_at(&r, &t, || Ok(expires))));
}

#[test]
fn malformed_wrong_expired_and_non_web_requests_are_rejected() {
    let (_d, r, a, av) = setup();
    let i = invite(&r, &a, &av, "guest@example.test");
    let t = token(3);
    let s = session(&r, &a, &av);
    let mut noncanonical = "A".repeat(42);
    noncanonical.push('B');
    let bad_tokens: Vec<String> = vec![
        String::new(),
        "short".into(),
        "A".repeat(44),
        format!("{}=", "A".repeat(43)),
        "+".repeat(43),
        noncanonical,
    ];
    for bad in &bad_tokens {
        assert!(matches!(
            issue(&r, &s, i.request_id, bad),
            Err(StoreError::InvalidInput)
        ));
        assert!(unauthorized(inspect(&r, bad)));
    }
    let link = issue_at(&r, &s, i.request_id, &t, || Ok(1_000)).unwrap();
    assert_eq!(link.expires_at, 1_000 + LINK_TTL_SECONDS);
    assert!(unauthorized(inspect_at(&r, &token(4), || Ok(1_001))));
    assert!(unauthorized(inspect_at(&r, &"A".repeat(43), || Ok(1_001))));
    inspect_at(&r, &t, || Ok(link.expires_at - 1)).unwrap();
    assert!(unauthorized(inspect_at(&r, &t, || Ok(link.expires_at))));
    assert!(unauthorized(with_pending_at(
        &r,
        &t,
        &alloc(&i),
        || Ok(link.expires_at),
        || ()
    )));
    assert!(unauthorized(activate_at(&r, &t, &alloc(&i), || Ok(
        link.expires_at
    ))));
    // Raw operator reservations, unknown requests, and nil ids are not web invitations.
    let raw_request = Uuid::new_v4();
    reserve(&r, raw_request, "raw@example.test".into()).unwrap();
    assert!(issue(&r, &s, raw_request, &token(5)).is_err());
    assert!(issue(&r, &s, Uuid::new_v4(), &token(5)).is_err());
    assert!(matches!(
        issue(&r, &s, Uuid::nil(), &token(5)),
        Err(StoreError::InvalidInput)
    ));
}

#[test]
fn reissue_rotates_digest_and_extends_ttl() {
    let (_d, r, a, av) = setup();
    let i = invite(&r, &a, &av, "guest@example.test");
    let other = invite(&r, &a, &av, "other@example.test");
    let first = issue_at(&r, &session(&r, &a, &av), i.request_id, &token(1), || {
        Ok(100)
    })
    .unwrap();
    let second = issue_at(&r, &session(&r, &a, &av), i.request_id, &token(2), || {
        Ok(5_000)
    })
    .unwrap();
    assert_eq!(second.expires_at, 5_000 + LINK_TTL_SECONDS);
    assert!(second.expires_at > first.expires_at);
    assert!(unauthorized(inspect_at(&r, &token(1), || Ok(5_001))));
    assert_eq!(inspect_at(&r, &token(2), || Ok(5_001)).unwrap(), second);
    // One capability can never name two invitations.
    assert!(matches!(
        issue_at(
            &r,
            &session(&r, &a, &av),
            other.request_id,
            &token(2),
            || Ok(5_002)
        ),
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        inspect_at(&r, &token(2), || Ok(5_003)).unwrap().invitation,
        i
    );
}

#[test]
fn issuer_demotion_is_denied_and_another_admin_can_reissue() {
    let (_d, r, a, av) = setup();
    let (b, bv) = add_admin(&r, &a, &av);
    let i = invite(&r, &a, &av, "guest@example.test");
    issue(&r, &session(&r, &a, &av), i.request_id, &token(1)).unwrap();
    inspect(&r, &token(1)).unwrap();
    // Logging out/in (a fresh checked session) does not disturb the link.
    session(&r, &a, &av);
    inspect(&r, &token(1)).unwrap();
    change(&r, &session(&r, &b, &bv), &a, Change::Demote).unwrap();
    assert!(unauthorized(inspect(&r, &token(1))));
    assert!(unauthorized(with_pending(&r, &token(1), &alloc(&i), || ())));
    assert!(unauthorized(activate(&r, &token(1), &alloc(&i))));
    assert!(checked_session(&r, &a, &av).is_err());
    issue(&r, &session(&r, &b, &bv), i.request_id, &token(2)).unwrap();
    inspect(&r, &token(2)).unwrap();
    assert!(unauthorized(inspect(&r, &token(1))));
}

#[test]
fn non_admin_and_stale_sessions_cannot_issue_and_suspension_invalidates() {
    let (_d, r, a, av) = setup();
    let (b, bv) = add_admin(&r, &a, &av);
    let i = invite(&r, &a, &av, "guest@example.test");
    // A plain member never obtains an admin pin.
    let m = allocate(&r, "member@example.test");
    assert!(checked_session(&r, &m, &view(&m, "member@example.test")).is_err());
    assert!(matches!(
        session(&r, &a, &av).at_epoch(99),
        Err(StoreError::Unauthorized)
    ));
    issue(&r, &session(&r, &a, &av), i.request_id, &token(1)).unwrap();
    let stale = session(&r, &a, &av);
    change(&r, &session(&r, &b, &bv), &a, Change::Suspend).unwrap();
    assert!(issue(&r, &stale, i.request_id, &token(2)).is_err());
    assert!(unauthorized(inspect(&r, &token(1))));
    // Resuming changes the role revision, so the old link stays dead.
    change(&r, &session(&r, &b, &bv), &a, Change::Resume).unwrap();
    assert!(unauthorized(inspect(&r, &token(1))));
    issue(&r, &session(&r, &a, &av), i.request_id, &token(3)).unwrap();
    inspect(&r, &token(3)).unwrap();
}

#[test]
fn invitee_suspension_fence_and_replaced_generation_are_denied() {
    let (_d, r, a, av) = setup();
    let i = invite(&r, &a, &av, "guest@example.test");
    let al = alloc(&i);
    let t = token(1);
    issue(&r, &session(&r, &a, &av), i.request_id, &t).unwrap();
    change(&r, &session(&r, &a, &av), &al, Change::Suspend).unwrap();
    let mut called = false;
    assert!(unauthorized(inspect(&r, &t)));
    assert!(with_pending(&r, &t, &al, || called = true).is_err());
    assert!(activate(&r, &t, &al).is_err());
    assert!(!called);
    change(&r, &session(&r, &a, &av), &al, Change::Resume).unwrap();
    assert!(unauthorized(inspect(&r, &token(1))));
    issue(&r, &session(&r, &a, &av), i.request_id, &token(1)).unwrap();
    inspect(&r, &t).unwrap();
    set_routing(&r, &al, false).unwrap();
    assert!(unauthorized(inspect(&r, &t)));
    assert!(with_pending(&r, &t, &al, || called = true).is_err());
    assert!(activate(&r, &t, &al).is_err());
    assert!(!called);

    let j = invite(&r, &a, &av, "second@example.test");
    let t2 = token(2);
    issue(&r, &session(&r, &a, &av), j.request_id, &t2).unwrap();
    {
        let (_lock, db) = open(&r, false).unwrap();
        db.execute(
            "UPDATE allocations SET generation=2 WHERE account_id=?1",
            [&j.account_id],
        )
        .unwrap();
    }
    assert!(unauthorized(inspect(&r, &t2)));
    assert!(activate(&r, &t2, &alloc(&j)).is_err());
}

#[test]
fn callback_holds_registry_lock_against_concurrent_revocation() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    let (_d, r, a, av) = setup();
    let (b, bv) = add_admin(&r, &a, &av);
    let i = invite(&r, &a, &av, "guest@example.test");
    let t = token(1);
    issue(&r, &session(&r, &a, &av), i.request_id, &t).unwrap();
    let revoked = AtomicBool::new(false);
    let (tx, rx) = mpsc::channel();
    let (rr, bb, bvv, aa, flag) = (&r, &b, &bv, &a, &revoked);
    let seen = std::thread::scope(|scope| {
        scope.spawn(move || {
            rx.recv().unwrap();
            change(rr, &session(rr, bb, bvv), aa, Change::Demote).unwrap();
            flag.store(true, Ordering::SeqCst);
        });
        with_pending(&r, &t, &alloc(&i), || {
            tx.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(300));
            revoked.load(Ordering::SeqCst)
        })
        .unwrap()
    });
    assert!(!seen, "revocation completed inside the callback");
    assert!(revoked.load(Ordering::SeqCst));
    assert!(unauthorized(inspect(&r, &t)));
}

#[test]
fn database_holds_only_a_digest_never_the_token() {
    let (_d, r, a, av) = setup();
    let i = invite(&r, &a, &av, "guest@example.test");
    let t = token(9);
    issue(&r, &session(&r, &a, &av), i.request_id, &t).unwrap();
    let bytes = raw(9);
    for entry in fs::read_dir(&r).unwrap() {
        let data = fs::read(entry.unwrap().path()).unwrap();
        assert!(!data.windows(t.len()).any(|w| w == t.as_bytes()));
        assert!(!data.windows(bytes.len()).any(|w| w == bytes.as_slice()));
    }
    let json = serde_json::to_string(&inspect(&r, &t).unwrap()).unwrap();
    assert!(!json.contains(&t) && !json.contains("digest"));
    let (_lock, db) = open(&r, false).unwrap();
    let (n, len): (i64, i64) = db
        .query_row(
            "SELECT count(*),max(length(digest)) FROM installation_invitation_links",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((n, len), (1, 32));
}

#[test]
fn version_eleven_registry_upgrades_and_preserves_existing_tables() {
    let (_d, r, a, av) = setup();
    {
        let (_lock, db) = open(&r, false).unwrap();
        db.execute_batch("DROP TABLE installation_invitation_links; PRAGMA user_version=11;")
            .unwrap();
    }
    assert_eq!(lookup_ready(&r, "a@example.test".into()).unwrap(), a);
    {
        let (_lock, db) = open(&r, false).unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 12);
        let members: i64 = db
            .query_row("SELECT count(*) FROM installation_members", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(members, 1);
    }
    let i = invite(&r, &a, &av, "guest@example.test");
    issue(&r, &session(&r, &a, &av), i.request_id, &token(1)).unwrap();
    inspect(&r, &token(1)).unwrap();
}

#[test]
fn expiry_is_sampled_after_waiting_for_the_registry_lock() {
    use std::sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
        mpsc,
    };
    let (_d, r, a, av) = setup();
    let i = invite(&r, &a, &av, "guest@example.test");
    let t = token(1);
    let link = issue(&r, &session(&r, &a, &av), i.request_id, &t).unwrap();
    let (held, _db) = open(&r, false).unwrap();
    let clock = Arc::new(AtomicI64::new(link.expires_at - 1));
    let advanced = clock.clone();
    let path = r.clone();
    let (called, received) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        with_pending_at(
            &path,
            &t,
            &alloc(&i),
            || {
                called.send(()).unwrap();
                Ok(clock.load(Ordering::SeqCst))
            },
            || panic!("expired callback ran"),
        )
    });
    assert!(received.recv_timeout(Duration::from_millis(50)).is_err());
    advanced.store(link.expires_at, Ordering::SeqCst);
    drop(held);
    received.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(unauthorized(thread.join().unwrap()));
}
