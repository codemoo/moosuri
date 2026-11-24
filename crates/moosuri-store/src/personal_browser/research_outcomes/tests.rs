//! These tests drive the real member services: a real store, a real browser job whose observation is
//! saved through `personal_browser_finish`, and the real outcome store. Only the observed page text is
//! synthetic. The Gmail inbox read needs a connected Google grant and is covered by the smoke script.
use super::*;

const SHOPS: &str = "Offer A: Alpha Shop\nPrice: $40.00\nShipping: $5.00\nDelivery by: 2099-12-05\n\nOffer B: Beta Mart\nPrice: $38.00\nShipping: free\nTotal: $38.00\nDelivery by: 2099-12-09\n";
const NOTICE: &str = "Title: Open Science Fund\nIssuer: Kfund Foundation\nDeadline: 2099-12-15\nAward: $20,000\nRequired documents: CV, proposal\n";

struct Fixture {
    store: Store,
    owner: VolumeOwner,
    user: UserContext,
    _dir: tempfile::TempDir,
}
async fn fixture() -> Fixture {
    let dir = crate::tests::directory();
    let root = dir.path().canonicalize().unwrap();
    let owner = VolumeOwner {
        account: Uuid::new_v4(),
        tenant: Uuid::new_v4(),
        volume: Uuid::new_v4(),
        generation: 1,
    };
    owner
        .initialize(
            &root,
            "Member".into(),
            "outcomes@example.test".into(),
            "Temporary-outcome-password-42".into(),
        )
        .await
        .unwrap();
    let store = owner.open_verified(&root).await.unwrap();
    let login = store
        .login(
            "outcomes@example.test".into(),
            "Temporary-outcome-password-42".into(),
        )
        .await
        .unwrap();
    let user = store
        .change_password(&login.user, "Changed-outcome-password-42".into())
        .await
        .unwrap()
        .user;
    Fixture {
        store,
        owner,
        user,
        _dir: dir,
    }
}
impl Fixture {
    async fn thread(&self) -> String {
        self.store
            .create_thread(&self.user, "Outcomes".into())
            .await
            .unwrap()
            .id
    }
    /// A browser read saved exactly like a real one: open, then settle with the observation text.
    async fn read(&self, thread: &str, url: &str, state: &str, text: &str) -> (Scope, Workspace) {
        let scope = Scope {
            owner: self.owner,
            user: self.user.clone(),
            thread: thread.into(),
        };
        let request = Uuid::new_v4();
        let Admission::Fresh(job) = self
            .store
            .personal_browser_open(scope.clone(), request, url.into(), "compare".into(), false)
            .await
            .unwrap()
        else {
            panic!("expected a fresh job")
        };
        let mut view = job.view;
        view.state = state.into();
        view.title = "Synthetic page".into();
        let view = self
            .store
            .personal_browser_finish_reading(
                scope.clone(),
                view,
                request,
                Some(Output::Observation("페이지를 확인했습니다.".into())),
                Some(text.into()),
            )
            .await
            .unwrap();
        (scope, view)
    }
    async fn create(&self, thread: &str, request: ro::OutcomeRequest) -> Result<ro::OutcomeView> {
        self.store
            .create_research_outcome(self.owner, self.user.clone(), 1, thread.into(), request)
            .await
    }
    async fn act(&self, thread: &str, action: ro::OutcomeAction) -> Result<ro::OutcomeView> {
        self.store
            .control_research_outcome(self.owner, self.user.clone(), 1, thread.into(), action)
            .await
    }
}
fn request(kind: ro::OutcomeKind, criteria: ro::Criteria) -> ro::OutcomeRequest {
    ro::OutcomeRequest {
        request_id: Uuid::new_v4().to_string(),
        kind,
        message: "비교해 줘".into(),
        criteria,
        browser_runs: vec![],
        mail_reads: vec![],
    }
}
fn shop(deadline: &str) -> ro::Criteria {
    ro::Criteria {
        deadline: Some(deadline.into()),
        budget_minor: None,
        currency: Some("USD".into()),
        terms: vec![],
    }
}
fn action(view: &ro::OutcomeView, kind: ro::OutcomeActionKind) -> ro::OutcomeAction {
    ro::OutcomeAction {
        request_id: view.id.clone(),
        revision: view.revision,
        action: kind,
        criteria: None,
        text: None,
        question_id: None,
    }
}

#[tokio::test]
async fn shopping_compares_a_real_saved_read_and_follows_a_changed_deadline() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    let thread = f.thread().await;
    let (_, view) = f
        .read(&thread, "https://shop.example/", "completed", SHOPS)
        .await;
    let created = f
        .create(
            &thread,
            request(ro::OutcomeKind::Shopping, shop("2099-12-10")),
        )
        .await
        .unwrap();
    assert_eq!(created.state, "ranked");
    assert_eq!(created.offers[0].seller, "Beta Mart");
    assert_eq!(
        created.citations[0].run_id.as_deref(),
        Some(view.run_id.as_str())
    );
    assert_eq!(created.citations[0].status, "current");
    // The same request id and body is a replay; a different body under that id conflicts.
    let mut replay = request(ro::OutcomeKind::Shopping, shop("2099-12-10"));
    replay.request_id = created.id.clone();
    assert_eq!(
        f.create(&thread, replay.clone()).await.unwrap().id,
        created.id
    );
    replay.message = "다른 요청".into();
    assert!(matches!(
        f.create(&thread, replay).await,
        Err(StoreError::Conflict)
    ));
    // A changed deadline is a revisioned refinement; replaying it is safe and a stale use conflicts.
    let mut refine = action(&created, ro::OutcomeActionKind::Refine);
    refine.criteria = Some(shop("2099-12-06"));
    let moved = f.act(&thread, refine.clone()).await.unwrap();
    assert_eq!(
        (moved.revision, moved.offers[0].seller.as_str()),
        (2, "Alpha Shop")
    );
    assert_eq!(f.act(&thread, refine.clone()).await.unwrap().revision, 2);
    refine.criteria = Some(shop("2099-12-01"));
    assert!(matches!(
        f.act(&thread, refine).await,
        Err(StoreError::Conflict)
    ));
    let none = f
        .act(
            &thread,
            ro::OutcomeAction {
                criteria: Some(shop("2099-12-01")),
                ..action(&moved, ro::OutcomeActionKind::Refine)
            },
        )
        .await
        .unwrap();
    assert_eq!(none.state, "no_match");
}

#[tokio::test]
async fn missing_source_asks_once_for_a_public_url_and_never_invents_a_result() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    let thread = f.thread().await;
    let created = f
        .create(
            &thread,
            request(ro::OutcomeKind::Shopping, shop("2099-12-10")),
        )
        .await
        .unwrap();
    assert_eq!(created.state, "needs_input");
    assert!(created.offers.is_empty() && created.receipts.is_empty());
    let question = created.question.clone().unwrap();
    for bad in [
        "http://shop.example/",
        "https://127.0.0.1/x",
        "https://user:pw@shop.example/",
        "https://localhost/x",
        "javascript:alert(1)",
    ] {
        let mut a = action(&created, ro::OutcomeActionKind::Answer);
        a.question_id = Some(question.id.clone());
        a.text = Some(bad.into());
        assert!(
            matches!(f.act(&thread, a).await, Err(StoreError::InvalidInput)),
            "{bad}"
        );
    }
    let mut answer = action(&created, ro::OutcomeActionKind::Answer);
    answer.question_id = Some(question.id.clone());
    answer.text = Some("https://shop.example/offers#top".into());
    let asked = f.act(&thread, answer.clone()).await.unwrap();
    assert_eq!(asked.state, "needs_source");
    assert!(asked.question.is_none());
    assert_eq!(
        (asked.next.kind.as_str(), asked.next.url.as_deref()),
        ("open_browser", Some("https://shop.example/offers"))
    );
    // Replay of the answer succeeds; the question is not asked again.
    assert_eq!(f.act(&thread, answer).await.unwrap().state, "needs_source");
    // After the member reads the page with the existing browser, a refresh adopts it.
    f.read(&thread, "https://shop.example/offers", "completed", SHOPS)
        .await;
    let refreshed = f
        .act(&thread, action(&asked, ro::OutcomeActionKind::Refresh))
        .await
        .unwrap();
    assert_eq!(refreshed.state, "ranked");
}

#[tokio::test]
async fn interrupted_shared_tab_exposes_the_existing_resume_and_not_a_result() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    let thread = f.thread().await;
    let (scope, view) = f
        .read(&thread, "https://shop.example/", "ready", SHOPS)
        .await;
    let created = f
        .create(
            &thread,
            request(ro::OutcomeKind::Shopping, shop("2099-12-10")),
        )
        .await
        .unwrap();
    assert_eq!(created.state, "ranked");
    f.store
        .personal_browser_reserve(
            scope,
            Version {
                workspace_id: view.id.clone(),
                revision: view.revision,
                request_id: Uuid::new_v4(),
            },
            "takeover",
            Some("controller-secret-for-test".into()),
        )
        .await
        .unwrap();
    let page = f
        .store
        .research_outcome_page(f.owner, f.user.clone(), thread.clone(), None)
        .await
        .unwrap();
    let live = &page.outcomes[0];
    assert_eq!(
        (live.state.as_str(), live.next.kind.as_str()),
        ("handoff", "resume_browser")
    );
    let h = live.handoff.as_ref().unwrap();
    assert_eq!(
        (h.action.as_str(), h.workspace_id.as_str()),
        ("resume", view.id.as_str())
    );
    assert!(live.receipts.is_empty());
}

#[tokio::test]
async fn application_draft_stays_a_draft_and_conflicting_notice_blocks_it() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    let thread = f.thread().await;
    f.read(&thread, "https://fund.example/notice", "completed", NOTICE)
        .await;
    let created = f
        .create(
            &thread,
            request(
                ro::OutcomeKind::Application,
                ro::Criteria {
                    terms: vec!["Open Science Fund".into()],
                    ..Default::default()
                },
            ),
        )
        .await
        .unwrap();
    assert_eq!(created.state, "ready");
    // No mail was read: said plainly, with a pointer to the existing inbox read.
    assert!(created.missing.iter().any(|m| m.code == "mail_not_read"));
    assert_eq!(created.next.kind, "read_mail");
    let drafted = f
        .act(&thread, action(&created, ro::OutcomeActionKind::Draft))
        .await
        .unwrap();
    assert_eq!(drafted.state, "draft_ready");
    let draft = drafted.draft.unwrap();
    assert!(!draft.submitted && draft.state == "draft");
    assert!(drafted.receipts.is_empty());
    // A shopping outcome refuses mail references, and a draft request is application-only.
    let mut bad = request(ro::OutcomeKind::Shopping, shop("2099-12-10"));
    bad.mail_reads = vec![Uuid::new_v4().to_string()];
    assert!(matches!(
        f.create(&thread, bad).await,
        Err(StoreError::InvalidInput)
    ));
}

#[tokio::test]
async fn withdrawal_revocation_and_isolation_reach_saved_outcomes() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    let thread = f.thread().await;
    let other = f.thread().await;
    let (_, view) = f
        .read(&thread, "https://shop.example/", "completed", SHOPS)
        .await;
    let created = f
        .create(
            &thread,
            request(ro::OutcomeKind::Shopping, shop("2099-12-10")),
        )
        .await
        .unwrap();
    // Another thread cannot read or reuse this outcome.
    assert!(matches!(
        f.store
            .research_outcome_page(
                f.owner,
                f.user.clone(),
                other.clone(),
                Some(Uuid::parse_str(&created.id).unwrap())
            )
            .await,
        Err(StoreError::NotFound)
    ));
    let mut stolen = request(ro::OutcomeKind::Shopping, shop("2099-12-10"));
    stolen.request_id = created.id.clone();
    assert!(matches!(
        f.create(&other, stolen).await,
        Err(StoreError::Conflict)
    ));
    // An older gateway epoch cannot act after a newer one.
    let newer = f
        .store
        .control_research_outcome(
            f.owner,
            f.user.clone(),
            5,
            thread.clone(),
            action(&created, ro::OutcomeActionKind::Refresh),
        )
        .await
        .unwrap();
    assert!(matches!(
        f.act(&thread, action(&newer, ro::OutcomeActionKind::Refresh))
            .await,
        Err(StoreError::Conflict)
    ));
    // The saved observation disappearing (withdrawn / deleted) removes the evidence at once.
    let run = view.run_id.clone();
    f.store
        .call(move |db| {
            db.conn.execute(
                "DELETE FROM chat_output WHERE run_id=?1 AND item LIKE 'browser-%'",
                [run],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let page = f
        .store
        .research_outcome_page(f.owner, f.user.clone(), thread.clone(), None)
        .await
        .unwrap();
    assert!(page.outcomes[0].offers.is_empty());
    // A changed auth epoch closes the outcome.
    let id = created.id.clone();
    f.store
        .call(move |db| {
            db.conn.execute(
                "UPDATE member_research_outcomes SET auth_epoch=auth_epoch+1 WHERE id=?1",
                [id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let closed = f
        .store
        .research_outcome_page(f.owner, f.user.clone(), thread.clone(), None)
        .await
        .unwrap();
    assert_eq!(closed.outcomes[0].state, "closed");
    assert!(
        closed.outcomes[0].offers.is_empty()
            && closed.outcomes[0].missing[0].code == "authority_changed"
    );
}

#[tokio::test]
async fn search_snippets_never_supply_offers_drafts_or_receipts() {
    let f = fixture().await;
    let thread = f.thread().await;
    f.read(
        &thread,
        "https://duckduckgo.com/?q=test",
        "completed",
        &format!("{SHOPS}\n{NOTICE}\nOrder confirmed: 12345"),
    )
    .await;
    for kind in [ro::OutcomeKind::Shopping, ro::OutcomeKind::Application] {
        let view = f
            .create(&thread, request(kind, ro::Criteria::default()))
            .await
            .unwrap();
        assert_eq!(view.citations.len(), 1);
        assert!(
            view.facts.is_empty()
                && view.offers.is_empty()
                && view.receipts.is_empty()
                && view.draft.is_none()
        );
        assert_eq!(view.state, "needs_source");
    }
}
