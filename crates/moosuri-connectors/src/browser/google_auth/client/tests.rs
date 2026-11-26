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
    /// Cancelled right after the (single) Add click was evaluated.
    cancel_on_add: Option<CancellationToken>,
}

impl Fake {
    fn new(script: Vec<String>, fallback: &'static str) -> Self {
        Self {
            script: script.into(),
            fallback,
            calls: Vec::new(),
            navigations: Vec::new(),
            cancel_on_add: None,
        }
    }
    fn count(&self, mode: Mode) -> usize {
        self.calls.iter().filter(|(m, _)| *m == mode).count()
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
        if call.mode == Mode::Add {
            if let Some(token) = &self.cancel_on_add {
                token.cancel();
            }
        }
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
        fingerprint: "",
    };
    let expression = expression(&spec, &call).unwrap();
    assert!(expression.contains(&format!("Moosuri {}", req.setup_id.simple())));
    assert!(expression.contains(&req.redirect_uri));
    assert!(expression.contains("\"create\""));
    assert_eq!(Op::from_action(Action::InspectClient), Some(Op::Inspect));
    assert_eq!(Op::from_action(Action::CreateClient), Some(Op::Create));
    assert_eq!(
        Op::from_action(Action::RecoverClientSecret),
        Some(Op::Recover)
    );
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

// ---- Same-client secret recovery (`RecoverClientSecret`) ----------------------------
// These script the page script's answers; the live NEW-card markup is NOT proven.

const RSECRET: &str = "GOCSPX-recovered-canary-wxyz";
const OLD_ROW: &str = "abcd\tNov 1, 2026\tenabled\told";
const NEW_ROW: &str = "wxyz\tNov 2, 2026\tenabled\tnew";
const NEW_SAVED: &str = "wxyz\tNov 2, 2026\tenabled\told";
const OTHER_ID: &str = "999-zzz.apps.googleusercontent.com";

fn secrets(rows: &[&str]) -> String {
    format!("secrets\n{}", rows.join("\n"))
}

/// Two list reads, then two detail reads of the one client.
fn find_script() -> Vec<String> {
    let link = format!("link:{ID}");
    let matched = format!("match:{ID}");
    vec![link.clone(), link, matched.clone(), matched]
}

/// find (0-3), inventory x2 (4-5), Add (6), capture (7), find (8-11), inventory x2 (12-13).
fn recover_script() -> Vec<String> {
    let mut script = find_script();
    script.extend([secrets(&[OLD_ROW]), secrets(&[OLD_ROW])]);
    script.push("acted".into());
    script.push(format!("captured\n{ID}\n{RSECRET}"));
    script.extend(find_script());
    script.extend([
        secrets(&[OLD_ROW, NEW_SAVED]),
        secrets(&[OLD_ROW, NEW_SAVED]),
    ]);
    script
}

#[tokio::test]
async fn existing_single_secret_yields_one_add_and_a_redacted_verified_client() {
    let req = request(Action::RecoverClientSecret);
    let mut fake = Fake::new(recover_script(), "!fail");
    let outcome = go(&mut fake, &req).await.unwrap();
    assert_eq!(fake.acts(), ["add"]);
    assert_eq!(fake.count(Mode::Add), 1);
    assert_eq!(fake.count(Mode::Capture), 1);
    // list, detail (before the write), list, detail (after): nothing navigated in between.
    assert_eq!(fake.navigations.len(), 4);
    assert!(fake.navigations.iter().all(|u| !u.contains("/create")));
    assert!(!format!("{outcome:?}").contains("canary"));
    let response = outcome.into_response();
    assert!(!format!("{response:?}").contains("canary"));
    assert!(response.answers(&req));
    assert!(!response.answers(&request(Action::InspectClient)));
    let Response::Client(registration) = response else {
        panic!("recovery must answer with the private registration");
    };
    assert_eq!(registration.client_id, ID);
    assert_eq!(registration.secret.expose(), RSECRET);
    assert_eq!(registration.setup_id, req.setup_id);
    assert_eq!(registration.redirect_uri, req.redirect_uri);
}

#[tokio::test]
async fn any_inventory_but_one_unmarked_secret_refuses_with_zero_writes() {
    let req = request(Action::RecoverClientSecret);
    let two = secrets(&[OLD_ROW, NEW_SAVED]);
    let cases: Vec<(Vec<String>, Error)> = vec![
        // Count two (the Add button may still be enabled): no write.
        (vec![two.clone(), two.clone()], Error::NeedsHuman),
        // The lone row already carries a NEW marker.
        (vec![secrets(&[NEW_ROW]); 2], Error::NeedsHuman),
        // Not stable between the two reads.
        (
            vec![secrets(&[OLD_ROW]), secrets(&[OLD_ROW, NEW_SAVED])],
            Error::NeedsHuman,
        ),
        (
            vec![
                secrets(&[OLD_ROW]),
                secrets(&["abcd\tNov 9, 2026\tenabled\told"]),
            ],
            Error::NeedsHuman,
        ),
        // The page script refused the markup / status / duplicates.
        (vec!["unsupported".into()], Error::NeedsHuman),
        (vec!["ambiguous".into()], Error::NeedsHuman),
        (vec!["wrong_account".into()], Error::Blocked),
        (vec!["client_mismatch".into()], Error::Blocked),
        // Malformed rows never parse.
        (vec!["secrets\n".into()], Error::Unavailable),
        (
            vec!["secrets\nabcd\tNov 1\tdisabled\told".into()],
            Error::Unavailable,
        ),
    ];
    for (inventory, expected) in cases {
        let mut script = find_script();
        script.extend(inventory);
        let mut fake = Fake::new(script, "!fail");
        assert_eq!(go(&mut fake, &req).await.unwrap_err(), expected);
        assert!(fake.acts().is_empty());
        assert_eq!(fake.count(Mode::Add), 0);
    }
    // Never-ready inventory is bounded and writes nothing.
    let mut loading = Fake::new(find_script(), "waiting");
    assert_eq!(go(&mut loading, &req).await.unwrap_err(), Error::NeedsHuman);
    assert_eq!(loading.count(Mode::Add), 0);
    assert_eq!(loading.count(Mode::Secrets), POLLS);
}

#[tokio::test]
async fn wrong_account_client_callback_or_missing_client_refuses_before_any_write() {
    let req = request(Action::RecoverClientSecret);
    for (raw, expected) in [
        ("wrong_account", Error::Blocked),
        ("client_mismatch", Error::Blocked),
        ("foreign", Error::Blocked),
        ("client_duplicate", Error::NeedsHuman),
    ] {
        // At the list, and at the detail page of an otherwise matching row.
        let mut at_list = Fake::of(&[], raw);
        assert_eq!(go(&mut at_list, &req).await.unwrap_err(), expected, "{raw}");
        let link = format!("link:{ID}");
        let mut at_detail = Fake::new(vec![link.clone(), link, raw.into()], "!fail");
        assert_eq!(
            go(&mut at_detail, &req).await.unwrap_err(),
            expected,
            "{raw}"
        );
        for fake in [&at_list, &at_detail] {
            assert!(fake.acts().is_empty(), "{raw}");
            assert_eq!(fake.count(Mode::Secrets), 0, "{raw}");
            assert_eq!(fake.count(Mode::Add), 0, "{raw}");
        }
    }
    // No such client: this operation is never a create.
    let mut absent = Fake::of(&["list_none", "list_none"], "!fail");
    assert_eq!(go(&mut absent, &req).await.unwrap_err(), Error::Blocked);
    assert!(absent.navigations.iter().all(|u| !u.contains("/create")));
    // The path segment must itself be the verified client id.
    let other = format!("link:{OTHER_ID}");
    let matched = format!("match:{ID}");
    let mut moved = Fake::new(
        vec![other.clone(), other, matched.clone(), matched],
        "!fail",
    );
    assert_eq!(go(&mut moved, &req).await.unwrap_err(), Error::NeedsHuman);
    assert_eq!(moved.count(Mode::Secrets), 0);
}

#[tokio::test]
async fn lost_or_refused_add_is_never_replayed() {
    let req = request(Action::RecoverClientSecret);
    let base = recover_script();
    for (answer, expected) in [
        ("!fail", Error::Unavailable),
        ("changed", Error::NeedsHuman),
        ("waiting", Error::NeedsHuman),
        ("ambiguous", Error::NeedsHuman),
        ("surprise", Error::Unavailable),
    ] {
        let mut script = base[..6].to_vec();
        script.push(answer.into());
        let mut fake = Fake::new(script, "!fail");
        assert_eq!(go(&mut fake, &req).await.unwrap_err(), expected, "{answer}");
        // Exactly one Add evaluation, no capture, no reload, no second attempt.
        assert_eq!(fake.count(Mode::Add), 1, "{answer}");
        assert_eq!(fake.count(Mode::Capture), 0, "{answer}");
        assert_eq!(fake.navigations.len(), 2, "{answer}");
    }
}

#[tokio::test]
async fn lost_hidden_or_ambiguous_new_value_needs_a_human_without_second_write() {
    let req = request(Action::RecoverClientSecret);
    let base = recover_script();
    // Never appears (hidden / not rendered): bounded polls, one Add, NeedsHuman.
    let mut silent = Fake::new(base[..7].to_vec(), "waiting");
    assert_eq!(go(&mut silent, &req).await.unwrap_err(), Error::NeedsHuman);
    assert_eq!(silent.count(Mode::Add), 1);
    assert_eq!(silent.count(Mode::Capture), RESULT_POLLS);
    for (raw, expected) in [
        ("secret_hidden", Error::NeedsHuman),
        ("ambiguous", Error::NeedsHuman),
        ("unsupported", Error::NeedsHuman),
        ("client_mismatch", Error::Blocked),
        ("wrong_account", Error::Blocked),
        // A malformed/masked/short value never parses into a secret.
        (
            "captured\nnot-a-client\nGOCSPX-abcdefgh",
            Error::Unavailable,
        ),
        (&*format!("captured\n{ID}\nGOCSPX-****"), Error::Unavailable),
        (
            &*format!("captured\n{ID}\nsecret-without-prefix"),
            Error::Unavailable,
        ),
        (&*format!("created\n{ID}\n{RSECRET}"), Error::Unavailable),
    ] {
        let mut script = base[..7].to_vec();
        script.push(raw.into());
        let mut fake = Fake::new(script, "!fail");
        assert_eq!(go(&mut fake, &req).await.unwrap_err(), expected, "{raw}");
        assert_eq!(fake.count(Mode::Add), 1, "{raw}");
        assert_eq!(fake.navigations.len(), 2, "{raw}");
    }
}

#[tokio::test]
async fn secret_is_released_only_if_the_same_client_and_both_rows_read_back() {
    let req = request(Action::RecoverClientSecret);
    let base = recover_script();
    let other_match = format!("match:{OTHER_ID}");
    let changed_old = "abcd\tNov 9, 2026\tenabled\told";
    let wrong_new = "zzzz\tNov 2, 2026\tenabled\told";
    type Edit = Box<dyn Fn(&mut Vec<String>)>;
    fn edit(f: impl Fn(&mut Vec<String>) + 'static) -> Edit {
        Box::new(f)
    }
    let cases: Vec<(&str, Edit, Error)> = vec![
        (
            "captured a different client",
            edit(|s| s[7] = format!("captured\n{OTHER_ID}\n{RSECRET}")),
            Error::Blocked,
        ),
        (
            "reloaded page is another client",
            edit(move |s| {
                s[10] = other_match.clone();
                s[11] = other_match.clone();
            }),
            Error::Blocked,
        ),
        (
            "old row changed",
            edit(move |s| {
                s[12] = secrets(&[changed_old, NEW_SAVED]);
                s[13] = secrets(&[changed_old, NEW_SAVED]);
            }),
            Error::NeedsHuman,
        ),
        (
            "new row does not carry the captured suffix",
            edit(move |s| {
                s[12] = secrets(&[OLD_ROW, wrong_new]);
                s[13] = secrets(&[OLD_ROW, wrong_new]);
            }),
            Error::NeedsHuman,
        ),
        (
            "new secret not persisted",
            edit(|s| {
                s[12] = secrets(&[OLD_ROW]);
                s[13] = secrets(&[OLD_ROW]);
            }),
            Error::NeedsHuman,
        ),
        (
            "a third secret appeared",
            edit(|s| {
                let rows = secrets(&[OLD_ROW, NEW_SAVED, "qqqq\tNov 3, 2026\tenabled\told"]);
                s[12] = rows.clone();
                s[13] = rows;
            }),
            Error::NeedsHuman,
        ),
        (
            "reload unstable",
            edit(|s| s[13] = secrets(&[OLD_ROW])),
            Error::NeedsHuman,
        ),
    ];
    for (label, edit, expected) in cases {
        let mut script = base.clone();
        edit(&mut script);
        let mut fake = Fake::new(script, "!fail");
        let result = go(&mut fake, &req).await;
        assert_eq!(result.unwrap_err(), expected, "{label}");
        assert_eq!(fake.count(Mode::Add), 1, "{label}");
    }
    // The client never reads back again: bounded reloads, still one Add.
    let mut missing = Fake::new(base[..8].to_vec(), "list_none");
    assert_eq!(go(&mut missing, &req).await.unwrap_err(), Error::NeedsHuman);
    assert_eq!(missing.count(Mode::Add), 1);
}

#[tokio::test]
async fn cancellation_never_writes_late_and_never_replays() {
    let req = request(Action::RecoverClientSecret);
    let spec = spec(&req);
    // Cancelled before anything: no navigation, no evaluation.
    let token = CancellationToken::new();
    token.cancel();
    let mut early = Fake::new(recover_script(), "!fail");
    assert_eq!(
        run(&mut early, Op::Recover, &spec, &token)
            .await
            .unwrap_err(),
        Error::Unavailable
    );
    assert!(early.calls.is_empty() && early.navigations.is_empty());
    // Cancelled right after the click was evaluated: the reply is not trusted, no
    // capture, no second click.
    let token = CancellationToken::new();
    let mut late = Fake::new(recover_script(), "!fail");
    late.cancel_on_add = Some(token.clone());
    assert_eq!(
        run(&mut late, Op::Recover, &spec, &token)
            .await
            .unwrap_err(),
        Error::Unavailable
    );
    assert_eq!(late.count(Mode::Add), 1);
    assert_eq!(late.count(Mode::Capture), 0);
}

#[test]
fn secret_inventory_and_capture_parse_strictly_and_redact() {
    assert!(matches!(
        Seen::parse(&secrets(&[OLD_ROW, NEW_ROW])),
        Ok(Seen::Secrets(rows)) if rows.len() == 2 && rows[1].fresh && !rows[0].fresh
    ));
    let debug = format!("{:?}", Seen::parse(&secrets(&[OLD_ROW])));
    assert!(
        !debug.contains("abcd") && !debug.contains("2026"),
        "{debug}"
    );
    let captured = format!("{:?}", Seen::parse(&format!("captured\n{ID}\n{RSECRET}")));
    assert!(!captured.contains("canary"), "{captured}");
    let too_many: Vec<String> = ["a1b2", "c3d4", "e5f6", "g7h8", "i9j0"]
        .iter()
        .map(|s| format!("{s}\tNov 1, 2026\tenabled\told"))
        .collect();
    for bad in [
        "secrets\n".to_owned(),
        secrets(&[OLD_ROW, OLD_ROW]),
        secrets(&["abc\tNov 1\tenabled\told"]),
        secrets(&["abcd\t\tenabled\told"]),
        secrets(&["abcd\tNov 1\tdisabled\told"]),
        secrets(&["abcd\tNov 1\tenabled\tmaybe"]),
        secrets(&["abcd\tNov 1\tenabled"]),
        secrets(&["abcd\tNov 1\tenabled\told\textra"]),
        secrets(&["ab*d\tNov 1\tenabled\told"]),
        format!("secrets\n{}", too_many.join("\n")),
    ] {
        assert_eq!(Seen::parse(&bad).unwrap_err(), Error::Unavailable, "{bad}");
    }
    let row = Row::parse(OLD_ROW).unwrap();
    assert_eq!(row.fingerprint(), "abcd\tNov 1, 2026\tenabled");
}

#[test]
fn recovery_script_uses_fixed_values_and_one_labelled_click() {
    // Only the fixed add label can be clicked in the secrets section, and it is gated
    // on the explicit `add` step; the copy button is only ever read, never clicked.
    assert_eq!(SCRIPT.matches("adds[0].click()").count(), 1);
    assert!(SCRIPT.contains(r#""add client secret""#));
    assert!(SCRIPT.contains(r#"if (expected !== "add") return "unsupported";"#));
    assert!(!SCRIPT.contains("copies[0].click"));
    assert!(!SCRIPT.contains("clipboard.read"));
    assert!(!SCRIPT.contains("navigator.clipboard"));
    let req = request(Action::RecoverClientSecret);
    let call = Call {
        mode: Mode::Add,
        last: "",
        expected: "add",
        segment: ID,
        fingerprint: "abcd\tNov 1, 2026\tenabled",
    };
    let expression = expression(&spec(&req), &call).unwrap();
    assert!(expression.contains("\"add\""));
    assert!(expression.contains(&req.redirect_uri));
}
