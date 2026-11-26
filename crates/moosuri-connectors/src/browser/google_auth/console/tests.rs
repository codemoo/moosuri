//! Synthetic state-machine fixtures. These script the *page script's answers*; they
//! do not prove that the real Google Console DOM produces those answers.
use super::*;
use std::collections::VecDeque;

struct Fake {
    script: VecDeque<&'static str>,
    fallback: &'static str,
    calls: Vec<Call>,
    navigations: Vec<String>,
}

impl Fake {
    fn new(script: &[&'static str], fallback: &'static str) -> Self {
        Self {
            script: script.iter().copied().collect(),
            fallback,
            calls: Vec::new(),
            navigations: Vec::new(),
        }
    }
    fn acts(&self) -> Vec<&'static str> {
        self.calls
            .iter()
            .map(|c| c.expected)
            .filter(|e| !e.is_empty())
            .collect()
    }
}

impl Port for Fake {
    async fn navigate(&mut self, url: &str) -> Result<(), Error> {
        self.navigations.push(url.to_owned());
        Ok(())
    }
    async fn eval(&mut self, _: &Spec, call: &Call) -> Result<String, Error> {
        self.calls.push(*call);
        match self.script.pop_front().unwrap_or(self.fallback) {
            "!fail" => Err(Error::Unavailable),
            other => Ok(other.to_owned()),
        }
    }
    async fn pause(&mut self, _: Duration) {}
}

fn spec() -> Spec {
    Spec::new(
        &ProjectId::parse("moosuri-0123456789abcdef0123").unwrap(),
        &Email::parse("Member@Example.test").unwrap(),
    )
}

async fn go(fake: &mut Fake, op: Op) -> Result<Reply, Error> {
    run(fake, op, &spec(), &CancellationToken::new()).await
}

#[tokio::test]
async fn existing_correct_branding_is_idempotent_and_never_clicks() {
    for op in [Op::InspectBranding, Op::ConfigureBranding] {
        let mut fake = Fake::new(&["branding_match"], "!fail");
        assert_eq!(go(&mut fake, op).await, Ok(Reply::Ready));
        assert!(fake.acts().is_empty());
        assert_eq!(fake.navigations.len(), 1);
    }
}

#[tokio::test]
async fn absence_must_be_stable() {
    let mut fake = Fake::new(&["get_started", "get_started"], "!fail");
    assert_eq!(go(&mut fake, Op::InspectBranding).await, Ok(Reply::Absent));
    let mut flaky = Fake::new(&["get_started", "branding_match"], "!fail");
    assert_eq!(
        go(&mut flaky, Op::InspectBranding).await,
        Err(Error::NeedsHuman)
    );
}

#[tokio::test]
async fn foreign_wrong_account_mismatch_and_ambiguity_are_refused_without_clicks() {
    for (raw, expected) in [
        ("foreign", Error::Blocked),
        ("wrong_account", Error::Blocked),
        ("branding_mismatch", Error::Blocked),
        ("ambiguous", Error::NeedsHuman),
        ("unsupported", Error::NeedsHuman),
        ("surprise", Error::Unavailable),
    ] {
        for op in [Op::InspectBranding, Op::ConfigureBranding] {
            let mut fake = Fake::new(&[], raw);
            assert_eq!(go(&mut fake, op).await, Err(expected_clone(&expected)));
            assert!(fake.acts().is_empty(), "{raw}");
        }
    }
}

fn expected_clone(e: &Error) -> Error {
    match e {
        Error::Blocked => Error::Blocked,
        Error::NeedsHuman => Error::NeedsHuman,
        Error::Unavailable => Error::Unavailable,
        Error::Invalid => Error::Invalid,
    }
}

#[tokio::test]
async fn endless_loading_times_out_to_needs_human() {
    let mut fake = Fake::new(&[], "waiting");
    assert_eq!(
        go(&mut fake, Op::InspectBranding).await,
        Err(Error::NeedsHuman)
    );
    assert_eq!(fake.calls.len(), POLLS);
}

const WIZARD: [&str; 22] = [
    "get_started",
    "acted",
    "app_name",
    "acted",
    "support_open",
    "acted",
    "support_pick",
    "acted",
    "app_next",
    "acted",
    "audience_next",
    "acted",
    "contact_fill",
    "acted",
    "contact_next",
    "acted",
    "agree_check",
    "acted",
    "agree_continue",
    "acted",
    "create",
    "acted",
];

#[tokio::test]
async fn configure_branding_walks_each_step_once_then_reads_back() {
    let mut script = vec!["get_started", "get_started"];
    script.extend(WIZARD);
    // First reload still shows the unconfigured page: wait, do not click again.
    script.extend(["get_started", "get_started", "branding_match"]);
    let mut fake = Fake::new(&script, "!fail");
    assert_eq!(go(&mut fake, Op::ConfigureBranding).await, Ok(Reply::Ready));
    let acts = fake.acts();
    assert_eq!(acts.len(), 11);
    assert_eq!(acts.iter().filter(|a| **a == "create").count(), 1);
    // inspect + (none during wizard) + two settle inspections
    assert_eq!(fake.navigations.len(), 3);
    assert!(fake.navigations.iter().all(|u| u.starts_with(
        "https://console.cloud.google.com/auth/branding?project=moosuri-0123456789abcdef0123&hl=en"
    )));
}

#[tokio::test]
async fn page_that_does_not_advance_is_never_clicked_twice() {
    let mut fake = Fake::new(
        &["get_started", "get_started", "get_started", "acted"],
        "get_started",
    );
    assert_eq!(
        go(&mut fake, Op::ConfigureBranding).await,
        Err(Error::NeedsHuman)
    );
    assert_eq!(fake.acts(), vec!["get_started"]);
}

#[tokio::test]
async fn wizard_is_never_adopted_mid_way() {
    let mut fake = Fake::new(&["get_started", "get_started", "app_name"], "!fail");
    assert_eq!(
        go(&mut fake, Op::ConfigureBranding).await,
        Err(Error::NeedsHuman)
    );
    assert!(fake.acts().is_empty());
}

#[tokio::test]
async fn lost_action_reply_is_an_error_not_a_retry() {
    let mut fake = Fake::new(
        &["get_started", "get_started", "get_started", "!fail"],
        "!fail",
    );
    assert_eq!(
        go(&mut fake, Op::ConfigureBranding).await,
        Err(Error::Unavailable)
    );
    assert_eq!(fake.acts(), vec!["get_started"]);
}

#[tokio::test]
async fn changed_page_at_click_time_does_not_count_as_acted() {
    let mut script = vec!["get_started", "get_started", "get_started", "changed"];
    script.extend(["foreign"]);
    let mut fake = Fake::new(&script, "!fail");
    assert_eq!(
        go(&mut fake, Op::ConfigureBranding).await,
        Err(Error::Blocked)
    );
}

#[tokio::test]
async fn partial_wizard_error_states_stop_immediately() {
    for raw in ["wrong_account", "ambiguous", "branding_mismatch"] {
        let mut fake = Fake::new(
            &["get_started", "get_started", "get_started", "acted", raw],
            "!fail",
        );
        assert!(go(&mut fake, Op::ConfigureBranding).await.is_err());
        assert_eq!(fake.acts(), vec!["get_started"]);
    }
}

#[tokio::test]
async fn scopes_inspection_states() {
    let mut fake = Fake::new(&["scopes_match"], "!fail");
    assert_eq!(go(&mut fake, Op::InspectScopes).await, Ok(Reply::Ready));
    assert!(fake.navigations[0].contains("/auth/scopes?project="));
    let mut partial = Fake::new(&["scopes_partial", "scopes_partial"], "!fail");
    assert_eq!(go(&mut partial, Op::InspectScopes).await, Ok(Reply::Absent));
    let mut extra = Fake::new(&["scopes_extra"], "!fail");
    assert_eq!(go(&mut extra, Op::InspectScopes).await, Err(Error::Blocked));
    let mut foreign = Fake::new(&["foreign"], "!fail");
    assert_eq!(
        go(&mut foreign, Op::InspectScopes).await,
        Err(Error::Blocked)
    );
}

#[tokio::test]
async fn configure_scopes_flow_passes_context_and_reads_back() {
    let mut fake = Fake::new(
        &[
            "scopes_partial",
            "scopes_partial", // inspect
            "scopes_partial",
            "acted",   // open
            "waiting", // dialog not yet visible
            "scopes_input",
            "acted",
            "scopes_add_table",
            "acted",
            "scopes_update",
            "acted",
            "scopes_save",
            "acted", // save
            "scopes_partial",
            "scopes_partial", // not persisted yet: wait, no click
            "scopes_match",
        ],
        "!fail",
    );
    assert_eq!(go(&mut fake, Op::ConfigureScopes).await, Ok(Reply::Ready));
    assert_eq!(
        fake.acts(),
        vec![
            "scopes_partial",
            "scopes_input",
            "scopes_add_table",
            "scopes_update",
            "scopes_save"
        ]
    );
    let peeks: Vec<_> = fake
        .calls
        .iter()
        .filter(|c| c.expected.is_empty())
        .map(|c| c.last)
        .collect();
    assert_eq!(
        peeks[2..8],
        [
            "",
            "scopes_partial",
            "scopes_partial",
            "scopes_input",
            "scopes_add_table",
            "scopes_update"
        ]
    );
}

#[tokio::test]
async fn existing_exact_scopes_do_not_click_and_extra_scopes_are_never_changed() {
    let mut fake = Fake::new(&["scopes_match"], "!fail");
    assert_eq!(go(&mut fake, Op::ConfigureScopes).await, Ok(Reply::Ready));
    assert!(fake.acts().is_empty());
    let mut extra = Fake::new(&[], "scopes_extra");
    assert_eq!(
        go(&mut extra, Op::ConfigureScopes).await,
        Err(Error::Blocked)
    );
    assert!(extra.acts().is_empty());
}

#[tokio::test]
async fn scopes_match_in_the_middle_of_a_change_is_not_trusted() {
    let mut fake = Fake::new(
        &["scopes_partial", "scopes_partial", "scopes_match"],
        "!fail",
    );
    assert_eq!(
        go(&mut fake, Op::ConfigureScopes).await,
        Err(Error::NeedsHuman)
    );
    assert!(fake.acts().is_empty());
}

#[tokio::test]
async fn cancellation_stops_before_any_page_call() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    for op in [
        Op::InspectBranding,
        Op::ConfigureBranding,
        Op::InspectScopes,
        Op::ConfigureScopes,
    ] {
        let mut fake = Fake::new(&[], "get_started");
        assert_eq!(
            run(&mut fake, op, &spec(), &cancel).await,
            Err(Error::Unavailable)
        );
        assert!(fake.calls.is_empty() && fake.navigations.is_empty());
    }
    // Cancelled mid-walk: no further click after the token fires.
    struct Cut(Fake, CancellationToken);
    impl Port for Cut {
        async fn navigate(&mut self, u: &str) -> Result<(), Error> {
            self.0.navigate(u).await
        }
        async fn eval(&mut self, s: &Spec, c: &Call) -> Result<String, Error> {
            let out = self.0.eval(s, c).await;
            if !c.expected.is_empty() {
                self.1.cancel();
            }
            out
        }
        async fn pause(&mut self, _: Duration) {}
    }
    let token = CancellationToken::new();
    let mut cut = Cut(
        Fake::new(
            &["get_started", "get_started", "get_started", "acted"],
            "get_started",
        ),
        token.clone(),
    );
    assert_eq!(
        run(&mut cut, Op::ConfigureBranding, &spec(), &token).await,
        Err(Error::Unavailable)
    );
    assert_eq!(cut.0.acts(), vec!["get_started"]);
}

#[test]
fn spec_expression_and_script_are_fixed_and_narrow() {
    assert_eq!(SCOPES.len(), 7);
    assert!(!SCOPES.contains(&"https://mail.google.com/"));
    let s = spec();
    let call = Call {
        mode: Mode::Scopes,
        last: "",
        expected: "",
    };
    let expr = expression(&s, &call).unwrap();
    assert!(
        expr.contains(r#"...["scopes","moosuri-0123456789abcdef0123","member@example.test","",["#)
    );
    assert!(expr.contains("gmail.readonly") && expr.contains("\"openid\""));
    // The page script has no network, storage, client-creation or billing surface.
    for forbidden in [
        "fetch(",
        "XMLHttpRequest",
        "localStorage",
        "cookie",
        "billing",
        "credentials",
        "client_secret",
        "eval(",
        "Function(",
        "location.href",
        "location.assign",
        "window.open",
    ] {
        assert!(!SCRIPT.contains(forbidden), "{forbidden}");
    }
    assert!(SCRIPT.contains("https://console.cloud.google.com"));
    assert!(SCRIPT.contains("params.get(\"project\") !== project"));
    assert!(
        s.url(Mode::Branding)
            .ends_with("?project=moosuri-0123456789abcdef0123&hl=en")
    );
}

#[test]
fn op_mapping_covers_only_console_actions() {
    assert_eq!(
        Op::from_action(Action::ConfigureScopes),
        Some(Op::ConfigureScopes)
    );
    assert_eq!(Op::from_action(Action::CreateProject), None);
    assert_eq!(Op::from_action(Action::Finish), None);
}

#[tokio::test]
async fn audience_readback_and_eligible_user_are_required_before_success() {
    let mut ready = Fake::new(&["audience_match"], "!fail");
    assert_eq!(
        go(&mut ready, Op::ConfigureAudience).await,
        Ok(Reply::Ready)
    );
    assert!(ready.acts().is_empty());
    let mut missing = Fake::new(&["audience_partial", "audience_partial"], "!fail");
    assert_eq!(
        go(&mut missing, Op::InspectAudience).await,
        Ok(Reply::Absent)
    );
    let mut wrong = Fake::new(&["audience_mismatch"], "!fail");
    assert_eq!(
        go(&mut wrong, Op::ConfigureAudience).await,
        Err(Error::Blocked)
    );
    assert!(wrong.acts().is_empty());
}

#[tokio::test]
async fn audience_adds_the_bound_user_once_then_reloads_the_persisted_table() {
    let mut fake = Fake::new(
        &[
            "audience_partial",
            "audience_partial",
            "audience_partial",
            "acted",
            "audience_input",
            "acted",
            "audience_save",
            "acted",
            "audience_match",
        ],
        "!fail",
    );
    assert_eq!(go(&mut fake, Op::ConfigureAudience).await, Ok(Reply::Ready));
    assert_eq!(
        fake.acts(),
        vec!["audience_partial", "audience_input", "audience_save"]
    );
    assert_eq!(fake.navigations.len(), 2);
}

const SITE: &str = "https://assistant.example.test";

fn site_spec() -> Spec {
    Spec::with_site(
        &ProjectId::parse("moosuri-0123456789abcdef0123").unwrap(),
        &Email::parse("Member@Example.test").unwrap(),
        SITE,
    )
    .unwrap()
}

async fn go_site(fake: &mut Fake, op: Op) -> Result<Reply, Error> {
    run(fake, op, &site_spec(), &CancellationToken::new()).await
}

#[test]
fn public_information_origin_is_validated_and_passed_to_the_page() {
    let project = ProjectId::parse("moosuri-0123456789abcdef0123").unwrap();
    let email = Email::parse("Member@Example.test").unwrap();
    for bad in [
        "http://assistant.example.test",
        "https://assistant.example.test/",
        "https://assistant.example.test/about",
        "https://user@assistant.example.test",
        "https://localhost",
        "https://192.0.2.7",
        "https://[2001:db8::1]",
        "https://assistant.example.test?x=1",
        "",
    ] {
        assert_eq!(
            Spec::with_site(&project, &email, bad).err(),
            Some(Error::NeedsHuman),
            "{bad}"
        );
    }
    assert!(Spec::with_site(&project, &email, "https://assistant.example.test:8443").is_ok());
    let call = Call {
        mode: Mode::Domains,
        last: "",
        expected: "",
    };
    let expr = expression(&site_spec(), &call).unwrap();
    assert!(expr.ends_with(r#","Moosuri","https://assistant.example.test","assistant.example.test"])"#));
    assert!(
        expression(&spec(), &call)
            .unwrap()
            .ends_with(r#","Moosuri","",""])"#)
    );
    assert!(site_spec().url(Mode::Domains).contains("/auth/branding?project="));
    assert!(site_spec().url(Mode::Publication).contains("/auth/audience?project="));
    // The labels and the exact confirmation are fixed in the page script, never supplied.
    for fixed in [
        "application home page",
        "application privacy policy link",
        "application terms of service link",
        "origin + \"/about\"",
        "origin + \"/privacy\"",
        "origin + \"/terms\"",
        "push to production?",
    ] {
        assert!(SCRIPT.contains(fixed), "{fixed}");
    }
    assert_eq!(
        Op::from_action(Action::ConfigurePublication),
        Some(Op::ConfigurePublication)
    );
    assert_eq!(
        Op::from_action(Action::InspectDomains),
        Some(Op::InspectDomains)
    );
}

#[tokio::test]
async fn blank_public_urls_are_filled_once_saved_once_and_read_back() {
    let mut fake = Fake::new(
        &[
            "domains_fill",
            "domains_fill", // inspect: blank, stable
            "domains_fill",
            "acted", // fill blanks
            "waiting",
            "domains_save",
            "acted", // Save
            "domains_fill",
            "domains_fill", // first reload not yet persisted: wait, no click
            "domains_match",
        ],
        "!fail",
    );
    assert_eq!(
        go_site(&mut fake, Op::ConfigureDomains).await,
        Ok(Reply::Ready)
    );
    assert_eq!(fake.acts(), vec!["domains_fill", "domains_save"]);
    assert_eq!(fake.navigations.len(), 3);
    assert!(
        fake.navigations
            .iter()
            .all(|u| u.contains("/auth/branding?project=moosuri-0123456789abcdef0123&hl=en"))
    );
    // Context-dependent script: the Save peek knows the fill was the last act.
    assert!(
        fake.calls
            .iter()
            .any(|c| c.expected.is_empty() && c.last == "domains_fill")
    );
    // Already persisted: inspect only, never a click.
    let mut done = Fake::new(&["domains_match"], "!fail");
    assert_eq!(
        go_site(&mut done, Op::ConfigureDomains).await,
        Ok(Reply::Ready)
    );
    assert!(done.acts().is_empty());
    let mut blank = Fake::new(&["domains_fill", "domains_fill"], "!fail");
    assert_eq!(
        go_site(&mut blank, Op::InspectDomains).await,
        Ok(Reply::Absent)
    );
}

#[tokio::test]
async fn conflicting_url_account_or_unproven_domain_never_writes() {
    for (raw, expected) in [
        ("domains_mismatch", Error::Blocked),
        ("branding_mismatch", Error::Blocked),
        ("wrong_account", Error::Blocked),
        ("foreign", Error::Blocked),
        ("domain_unproven", Error::NeedsHuman),
        ("ambiguous", Error::NeedsHuman),
        ("unsupported", Error::NeedsHuman),
    ] {
        for op in [Op::InspectDomains, Op::ConfigureDomains] {
            let mut fake = Fake::new(&[], raw);
            assert_eq!(go_site(&mut fake, op).await, Err(expected_clone(&expected)));
            assert!(fake.acts().is_empty(), "{raw}");
        }
    }
    // A conflict appearing after the fill started stops before Save.
    let mut midway = Fake::new(
        &[
            "domains_fill",
            "domains_fill",
            "domains_fill",
            "acted",
            "domains_mismatch",
        ],
        "!fail",
    );
    assert_eq!(
        go_site(&mut midway, Op::ConfigureDomains).await,
        Err(Error::Blocked)
    );
    assert_eq!(midway.acts(), vec!["domains_fill"]);
    // Without a validated origin nothing is even opened.
    let mut none = Fake::new(&[], "domains_fill");
    assert_eq!(
        go(&mut none, Op::ConfigureDomains).await,
        Err(Error::NeedsHuman)
    );
    assert!(none.calls.is_empty() && none.navigations.is_empty());
}

#[tokio::test]
async fn publication_is_never_ready_from_a_disabled_action_and_production_is_idempotent() {
    // Disabled prerequisite / unknown status: a human, no click, never Ready.
    for raw in ["publish_blocked", "unsupported", "ambiguous"] {
        for op in [Op::InspectPublication, Op::ConfigurePublication] {
            let mut fake = Fake::new(&[], raw);
            assert_eq!(go(&mut fake, op).await, Err(Error::NeedsHuman));
            assert!(fake.acts().is_empty(), "{raw}");
        }
    }
    let mut external = Fake::new(&[], "publication_mismatch");
    assert_eq!(
        go(&mut external, Op::ConfigurePublication).await,
        Err(Error::Blocked)
    );
    // In production (exact field) on a resume: Ready without any click.
    for op in [Op::InspectPublication, Op::ConfigurePublication] {
        let mut fake = Fake::new(&["publication_match"], "!fail");
        assert_eq!(go(&mut fake, op).await, Ok(Reply::Ready));
        assert!(fake.acts().is_empty());
        assert!(fake.navigations[0].contains("/auth/audience?project="));
    }
    // Testing with an enabled action is the only absence.
    let mut testing = Fake::new(&["publish_open", "publish_open"], "!fail");
    assert_eq!(
        go(&mut testing, Op::InspectPublication).await,
        Ok(Reply::Absent)
    );
}

#[tokio::test]
async fn publish_clicks_open_then_the_exact_confirm_once_and_reloads() {
    let mut fake = Fake::new(
        &[
            "publish_open",
            "publish_open", // inspect
            "publish_open",
            "acted", // Publish app
            "waiting",
            "publish_confirm",
            "acted", // Confirm in the exact dialog
            "publish_open",
            "publish_open", // reload still Testing: wait, no click
            "publication_match",
        ],
        "!fail",
    );
    assert_eq!(
        go(&mut fake, Op::ConfigurePublication).await,
        Ok(Reply::Ready)
    );
    assert_eq!(fake.acts(), vec!["publish_open", "publish_confirm"]);
    // A page that never reaches production is NeedsHuman, not another click.
    let mut stuck = Fake::new(
        &[
            "publish_open",
            "publish_open",
            "publish_open",
            "acted",
            "publish_confirm",
            "acted",
        ],
        "publish_open",
    );
    assert_eq!(
        go(&mut stuck, Op::ConfigurePublication).await,
        Err(Error::NeedsHuman)
    );
    assert_eq!(stuck.acts(), vec!["publish_open", "publish_confirm"]);
}

#[tokio::test]
async fn lost_publish_or_save_reply_is_an_error_and_never_resubmitted() {
    let mut lost = Fake::new(
        &[
            "publish_open",
            "publish_open",
            "publish_open",
            "acted",
            "publish_confirm",
            "!fail",
        ],
        "!fail",
    );
    assert_eq!(
        go(&mut lost, Op::ConfigurePublication).await,
        Err(Error::Unavailable)
    );
    assert_eq!(lost.acts(), vec!["publish_open", "publish_confirm"]);
    let mut save = Fake::new(
        &[
            "domains_fill",
            "domains_fill",
            "domains_fill",
            "acted",
            "domains_save",
            "!fail",
        ],
        "!fail",
    );
    assert_eq!(
        go_site(&mut save, Op::ConfigureDomains).await,
        Err(Error::Unavailable)
    );
    assert_eq!(save.acts(), vec!["domains_fill", "domains_save"]);
    // Cancellation between the inspection and any click performs no click.
    let cancel = CancellationToken::new();
    cancel.cancel();
    for op in [
        Op::InspectDomains,
        Op::ConfigureDomains,
        Op::InspectPublication,
        Op::ConfigurePublication,
    ] {
        let mut fake = Fake::new(&[], "publish_open");
        assert_eq!(
            run(&mut fake, op, &site_spec(), &cancel).await,
            Err(Error::Unavailable)
        );
        assert!(fake.calls.is_empty() && fake.navigations.is_empty());
    }
}

#[test]
fn step_wire_names_round_trip_and_are_ordered() {
    for (step, wire) in STEPS {
        assert_eq!(step.wire(), wire);
        assert_eq!(Seen::parse(wire), Ok(Seen::Step(step)));
    }
    assert!(STEPS.windows(2).all(|w| w[0].0 < w[1].0));
}
