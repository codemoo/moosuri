//! Synthetic state-machine fixtures. They script the *page script's answers*; they
//! do not prove that the real Google Console DOM produces those answers.
use super::*;
use std::collections::VecDeque;

const ID: &str = "123456-abc.apps.googleusercontent.com";
const SECRET: &str = "GOCSPX-canary-secret-value";
const STEPS_IN_ORDER: [&str; 7] = [
    "app_type_open",
    "app_type_pick",
    "name_fill",
    "redirect_add",
    "redirect_fill",
    "agent_check",
    "create",
];

struct Fake {
    script: VecDeque<String>,
    fallback: &'static str,
    calls: Vec<(Mode, &'static str)>,
    navigations: Vec<String>,
}

impl Fake {
    fn new(script: Vec<String>, fallback: &'static str) -> Self {
        Self {
            script: script.into(),
            fallback,
            calls: Vec::new(),
            navigations: Vec::new(),
        }
    }
    fn of(script: &[&str], fallback: &'static str) -> Self {
        Self::new(script.iter().map(|s| (*s).to_owned()).collect(), fallback)
    }
    fn acts(&self) -> Vec<&'static str> {
        self.calls
            .iter()
            .map(|(_, e)| *e)
            .filter(|e| !e.is_empty())
            .collect()
    }
}

impl Port for Fake {
    async fn navigate(&mut self, url: &str) -> Result<(), Error> {
        self.navigations.push(url.to_owned());
        Ok(())
    }
    async fn eval(&mut self, _: &Spec, call: &Call<'_>) -> Result<Zeroizing<String>, Error> {
        self.calls.push((call.mode, call.expected));
        let next = self
            .script
            .pop_front()
            .unwrap_or_else(|| self.fallback.to_owned());
        if next == "!fail" {
            Err(Error::Unavailable)
        } else {
            Ok(Zeroizing::new(next))
        }
    }
    async fn pause(&mut self, _: Duration) {}
}

fn request(action: Action) -> Request {
    let binding = super::super::tests::binding();
    Request {
        setup_id: Uuid::new_v4(),
        redirect_uri: Request::callback(&binding),
        action,
    }
}

fn spec(request: &Request) -> Spec {
    Spec::new(
        request,
        &ProjectId::parse(&request.project_id()).unwrap(),
        &Email::parse("Member@Example.test").unwrap(),
    )
}

async fn go(fake: &mut Fake, request: &Request) -> Result<Outcome, Error> {
    let op = Op::from_action(request.action).unwrap();
    run(fake, op, &spec(request), &CancellationToken::new()).await
}

/// Absent proof, the whole form, the visible creation result, then a fresh read-back.
fn create_script() -> Vec<String> {
    let mut script: Vec<String> = vec!["list_none".into(); 2];
    for step in STEPS_IN_ORDER {
        script.push(step.into());
        script.push("acted".into());
    }
    script.push(format!("created\n{ID}\n{SECRET}"));
    let link = format!("link:{ID}");
    script.extend([link.clone(), link]);
    let matched = format!("match:{ID}");
    script.extend([matched.clone(), matched]);
    script
}

#[tokio::test]
async fn inspect_never_mutates_and_never_yields_a_secret() {
    let req = request(Action::InspectClient);
    let mut absent = Fake::of(&["list_none", "list_none"], "!fail");
    let outcome = go(&mut absent, &req).await.unwrap();
    assert!(matches!(outcome, Outcome::Absent));
    assert!(absent.acts().is_empty());
    assert_eq!(absent.navigations.len(), 1);
    assert!(absent.navigations[0].contains("/auth/clients?project=moosuri-"));

    let link = format!("link:{ID}");
    let matched = format!("match:{ID}");
    let mut present = Fake::new(vec![link.clone(), link, matched.clone(), matched], "!fail");
    let outcome = go(&mut present, &req).await.unwrap();
    assert!(present.acts().is_empty());
    assert_eq!(present.navigations.len(), 2);
    assert!(present.navigations[1].contains(&format!("/auth/clients/{ID}?project=")));
    let response = outcome.into_response();
    assert!(response.answers(&req), "{response:?}");
    let Response::ClientPresent(identity) = response else {
        panic!("inspect must answer with a public identity");
    };
    assert_eq!(identity.client_id, ID);
    assert_eq!(identity.setup_id, req.setup_id);
    assert_eq!(identity.redirect_uri, req.redirect_uri);
    // The detail pass is not "stable" if the page changes between the two reads.
    let mut flaky = Fake::new(
        vec![
            format!("link:{ID}"),
            format!("link:{ID}"),
            format!("match:{ID}"),
            "match:999-zzz.apps.googleusercontent.com".into(),
        ],
        "!fail",
    );
    assert_eq!(go(&mut flaky, &req).await.unwrap_err(), Error::NeedsHuman);
}

#[tokio::test]
async fn new_client_is_created_once_in_order_and_secret_is_redacted() {
    let req = request(Action::CreateClient);
    let mut fake = Fake::new(create_script(), "!fail");
    let outcome = go(&mut fake, &req).await.unwrap();
    assert_eq!(fake.acts(), STEPS_IN_ORDER);
    assert_eq!(fake.acts().iter().filter(|a| **a == "create").count(), 1);
    // list, create form, list, detail.
    assert_eq!(fake.navigations.len(), 4);
    assert!(fake.navigations[1].contains("/auth/clients/create?project="));
    assert!(!format!("{outcome:?}").contains("canary"));
    let response = outcome.into_response();
    assert!(!format!("{response:?}").contains("canary"));
    assert!(response.answers(&req));
    let Response::Client(registration) = response else {
        panic!("create must answer with the registration");
    };
    assert_eq!(registration.client_id, ID);
    assert_eq!(registration.secret.expose(), SECRET);
    assert_eq!(registration.setup_id, req.setup_id);
    assert_eq!(registration.redirect_uri, req.redirect_uri);
}

#[tokio::test]
async fn duplicates_mismatch_and_foreign_pages_are_refused_without_mutation() {
    for (raw, expected) in [
        ("client_duplicate", Error::NeedsHuman),
        ("ambiguous", Error::NeedsHuman),
        ("unsupported", Error::NeedsHuman),
        ("client_mismatch", Error::Blocked),
        ("foreign", Error::Blocked),
        ("wrong_account", Error::Blocked),
        ("surprise", Error::Unavailable),
    ] {
        for action in [Action::InspectClient, Action::CreateClient] {
            let req = request(action);
            let mut fake = Fake::of(&[], raw);
            assert_eq!(go(&mut fake, &req).await.unwrap_err(), expected, "{raw}");
            assert!(fake.acts().is_empty(), "{raw}");
            assert_eq!(fake.navigations.len(), 1, "{raw}: only the list was opened");
        }
    }
    // A present client (or an invalid persisted one) never leads to a create.
    let req = request(Action::CreateClient);
    let mut existing = Fake::new(
        vec![
            format!("link:{ID}"),
            format!("link:{ID}"),
            format!("match:{ID}"),
            format!("match:{ID}"),
        ],
        "!fail",
    );
    assert_eq!(go(&mut existing, &req).await.unwrap_err(), Error::Blocked);
    assert!(existing.acts().is_empty());
    assert!(existing.navigations.iter().all(|u| !u.contains("/create")));
    let mut bad_settings = Fake::new(
        vec![
            format!("link:{ID}"),
            format!("link:{ID}"),
            "client_mismatch".into(),
        ],
        "!fail",
    );
    assert_eq!(
        go(&mut bad_settings, &request(Action::InspectClient))
            .await
            .unwrap_err(),
        Error::Blocked
    );
}

#[tokio::test]
async fn uncertain_create_is_never_replayed_and_secret_is_not_invented() {
    let req = request(Action::CreateClient);
    let base = create_script();
    // Response lost right after the click: exactly one create attempt, no retry.
    let mut lost = base[..2 + 14].to_vec();
    lost.push("!fail".into());
    let mut fake = Fake::new(lost, "!fail");
    assert_eq!(go(&mut fake, &req).await.unwrap_err(), Error::Unavailable);
    assert_eq!(fake.acts().iter().filter(|a| **a == "create").count(), 1);
    assert_eq!(fake.navigations.len(), 2, "no second form load or list");
    // The result never appears: bounded polling, then NeedsHuman, no second click.
    let mut silent = Fake::new(base[..2 + 14].to_vec(), "waiting");
    assert_eq!(go(&mut silent, &req).await.unwrap_err(), Error::NeedsHuman);
    assert_eq!(silent.acts().iter().filter(|a| **a == "create").count(), 1);
    assert_eq!(
        silent
            .calls
            .iter()
            .filter(|(mode, _)| *mode == Mode::Result)
            .count(),
        RESULT_POLLS
    );
    // The secret is not visible (masked): the client may exist but nothing is returned.
    let mut hidden = base[..2 + 14].to_vec();
    hidden.push("secret_hidden".into());
    let mut masked = Fake::new(hidden, "!fail");
    assert_eq!(go(&mut masked, &req).await.unwrap_err(), Error::NeedsHuman);
    assert_eq!(masked.navigations.len(), 2);
    // A form that is already submittable on load is not adopted.
    let mut prefilled = Fake::of(&["list_none", "list_none", "create"], "!fail");
    assert_eq!(
        go(&mut prefilled, &req).await.unwrap_err(),
        Error::NeedsHuman
    );
    assert!(prefilled.acts().is_empty());
}

#[tokio::test]
async fn secret_is_released_only_after_exact_persisted_readback() {
    let req = request(Action::CreateClient);
    // Persisted client has a different id than the captured one.
    let mut script = create_script();
    let n = script.len();
    script[n - 2] = "match:999-zzz.apps.googleusercontent.com".into();
    script[n - 1] = "match:999-zzz.apps.googleusercontent.com".into();
    let mut other = Fake::new(script, "!fail");
    assert_eq!(go(&mut other, &req).await.unwrap_err(), Error::Blocked);
    // Persisted settings do not match the requested callback / flags.
    let mut script = create_script();
    let n = script.len();
    script[n - 2] = "client_mismatch".into();
    let mut mismatch = Fake::new(script, "!fail");
    assert_eq!(go(&mut mismatch, &req).await.unwrap_err(), Error::Blocked);
    // The client never shows up in the list again: bounded reloads, no second create.
    let mut script = create_script();
    script.truncate(2 + 14 + 1);
    let mut missing = Fake::new(script, "list_none");
    assert_eq!(go(&mut missing, &req).await.unwrap_err(), Error::NeedsHuman);
    assert_eq!(missing.acts().iter().filter(|a| **a == "create").count(), 1);
}

#[test]
fn page_answers_and_private_captures_parse_strictly() {
    assert!(matches!(
        Seen::parse(&format!("created\n{ID}\n{SECRET}")),
        Ok(Seen::Created(_))
    ));
    let debug = format!("{:?}", Seen::parse(&format!("created\n{ID}\n{SECRET}")));
    assert!(!debug.contains("canary"), "{debug}");
    let bad_cases: Vec<String> = vec![
        "created\nnot-a-client\nGOCSPX-abcdefgh".into(),
        format!("created\n{ID}\nGOCSPX-****"),
        format!("created\n{ID}\nshort"),
        format!("created\n{ID}\nGOCSPX-abc def-ghi"),
        format!("created\n{ID}"),
        "link:".into(),
        "link:../etc".into(),
        "link:create".into(),
        "link:a/b".into(),
        "match:evil.example".into(),
        String::new(),
        "ready".into(),
    ];
    for bad in &bad_cases {
        assert_eq!(Seen::parse(bad).unwrap_err(), Error::Unavailable);
    }
    assert!(matches!(
        Seen::parse(&format!("link:{ID}")),
        Ok(Seen::ListOne(_))
    ));
}

#[test]
fn observed_client_id_path_segment_is_accepted_and_traversal_is_not() {
    assert!(segment_ok("1234-abc.apps.googleusercontent.com"));
    assert!(!segment_ok("a?b"));
    assert!(!segment_ok("%2e%2e"));
}

#[test]
fn only_fixed_values_reach_the_page_and_script_has_no_side_channels() {
    let req = request(Action::CreateClient);
    let spec = spec(&req);
    let call = Call {
        mode: Mode::Create,
        last: "",
        expected: "",
        segment: "",
    };
    let expression = expression(&spec, &call).unwrap();
    assert!(expression.contains(&format!("Moosuri {}", req.setup_id.simple())));
    assert!(expression.contains(&req.redirect_uri));
    assert!(expression.contains("\"create\""));
    assert_eq!(Op::from_action(Action::InspectClient), Some(Op::Inspect));
    assert_eq!(Op::from_action(Action::CreateClient), Some(Op::Create));
    assert_eq!(Op::from_action(Action::Finish), None);
    for forbidden in [
        "fetch(",
        "XMLHttpRequest",
        "localStorage",
        "sessionStorage",
        "document.cookie",
        "eval(",
        "Function(",
        "WebSocket",
        "download",
    ] {
        assert!(!SCRIPT.contains(forbidden), "{forbidden}");
    }
    // Every step name is unique and ordered.
    let wires: Vec<_> = STEPS.iter().map(|(_, w)| *w).collect();
    assert_eq!(wires, STEPS_IN_ORDER);
    assert!(STEPS.windows(2).all(|w| w[0].0 < w[1].0));
}
