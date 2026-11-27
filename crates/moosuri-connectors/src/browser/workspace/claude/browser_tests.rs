//! Actual Google Chrome acceptance for the private Claude callback capture.
//!
//! Run only in the pinned, confined Google-auth image with `--network=none`, exactly like
//! the SDK browser smoke. The first document is a synthetic `data:` page, so no provider
//! request is possible or made. The callback it navigates to is still the exact official
//! `https://platform.claude.com/oauth/code/callback` carrying synthetic values: the real
//! Fetch rule must pause and answer it locally, or Chrome would fail the (absent) network.
//! `MOOSURI_CLAUDE_TEST_CASE` selects one scenario per process (default `valid`).
//! Synthetic values are printed nowhere; failures report only scenario names.
use super::*;

const STATE: &str = "state_VALUE-123~abcdefgh";
const CODE: &str = "CANARYcode123456";
const AUTHORIZE: &str = "https://claude.com/cai/oauth/authorize?response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&state=state_VALUE-123~abcdefgh";
const CASES: [&str; 6] = [
    "valid",
    "wrong_state",
    "iframe",
    "popup",
    "subresource",
    "cancel",
];

fn callback(code: &str, state: &str) -> String {
    format!("{}?code={code}&state={state}", claude_auth::CALLBACK)
}

fn data_url(html: &str) -> String {
    let mut out = String::from("data:text/html;charset=utf-8,");
    for byte in html.bytes() {
        if byte.is_ascii_alphanumeric() {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn page_for(case: &str) -> String {
    let good = callback(CODE, STATE);
    let wrong = callback(CODE, "another_state_value");
    let json = |url: &str| serde_json::to_string(url).unwrap();
    let escaped = |url: &str| url.replace('&', "&amp;");
    match case {
        "valid" => format!("<script>location.replace({})</script>", json(&good)),
        "wrong_state" => format!("<script>location.replace({})</script>", json(&wrong)),
        // A *correct* callback in a subframe/popup/subresource must never be adopted.
        "iframe" => format!("<iframe src='{}'></iframe>", escaped(&good)),
        "popup" => format!("<script>window.open({})</script>", json(&good)),
        "subresource" => format!(
            "<script>fetch({},{{mode:'no-cors'}}).catch(()=>0)</script>",
            json(&good)
        ),
        // Nothing ever navigates: used for cancellation.
        _ => "<p>waiting</p>".to_owned(),
    }
}

/// Same steps as `ClaudeWorkspace::launch`, except that the initial document is the
/// synthetic page. The production constructor and URL validation are not weakened.
async fn launch_synthetic(
    html: &str,
    cancel: CancellationToken,
) -> Result<ClaudeWorkspace, BrowserError> {
    let tenant: Uuid = std::env::var("MOOSURI_BROWSER_TENANT")
        .expect("MOOSURI_BROWSER_TENANT is required")
        .parse()
        .expect("tenant must be a UUID");
    let config = BrowserConfig {
        tenant,
        chromium: std::path::PathBuf::from("/usr/bin/google-chrome-stable"),
        profile_root: std::path::PathBuf::from("/profiles"),
        proxy: private_proxy(
            &std::env::var("MOOSURI_BROWSER_PROXY_IP")
                .expect("MOOSURI_BROWSER_PROXY_IP is required"),
        )?,
        javascript_enabled: true,
    };
    capsule_health_mode(&config, true)?;
    let mut display = Display::start().await.map_err(|_| BrowserError::Setup)?;
    let session =
        match BrowserSession::launch_mode(config, cancel, Duration::from_secs(90), true).await {
            Ok(session) => session,
            Err(error) => {
                display.stop().await;
                return Err(error);
            }
        };
    let mut this = ClaudeWorkspace {
        inner: BrowserWorkspace {
            session,
            current: None,
            proof: None,
            prior_proof: None,
            ignored_targets: HashSet::new(),
            last_links: None,
            human_interacted: false,
            private_auth: true,
        },
        display: Some(display),
        authorize: AuthorizeUrl::parse(AUTHORIZE).unwrap(),
        fetch_session: String::new(),
        top_frame: String::new(),
        phase: Phase::Waiting,
    };
    if let Err(error) = open_synthetic(&mut this, html).await {
        this.shutdown().await;
        return Err(error);
    }
    Ok(this)
}

async fn open_synthetic(this: &mut ClaudeWorkspace, html: &str) -> Result<(), BrowserError> {
    this.inner.blank().await?;
    let identity = this.inner.refresh().await?;
    this.top_frame = identity.frame;
    let sid = this.inner.sid()?;
    // Identical Fetch rule to production, enabled before the first navigation.
    this.inner
        .session
        .cdp
        .command_timeout(
            "Fetch.enable",
            json!({"patterns":[{
                "urlPattern":format!("{}*", claude_auth::CALLBACK),
                "resourceType":"Document",
                "requestStage":"Request",
            }]}),
            Some(&sid),
            STEP,
        )
        .await?;
    drop(this.inner.session.cdp.take_paused());
    this.fetch_session = sid.clone();
    this.inner.session.cdp.before_navigation(&sid);
    let navigation = this
        .inner
        .session
        .cdp
        .command_timeout(
            "Page.navigate",
            json!({"url":data_url(html)}),
            Some(&sid),
            Duration::from_secs(15),
        )
        .await?;
    if navigation.get("errorText").is_some() {
        return Err(BrowserError::Network);
    }
    Ok(())
}

/// Watch until `done` holds or the time is up; returns every watch result seen.
async fn watch_until(
    workspace: &mut ClaudeWorkspace,
    seconds: u64,
    done: impl Fn(&Phase) -> bool,
) -> Vec<Result<(), BrowserError>> {
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut results = Vec::new();
    while Instant::now() < end {
        results.push(workspace.watch().await);
        if done(&workspace.phase) {
            break;
        }
    }
    results
}

fn phase_name(phase: &Phase) -> &'static str {
    match phase {
        Phase::Waiting => "waiting",
        Phase::Captured(_) => "captured",
        Phase::Delivered => "delivered",
        Phase::Failed => "failed",
    }
}

async fn assert_blank_and_free_of_the_code(workspace: &mut ClaudeWorkspace) {
    let identity = workspace
        .inner
        .refresh()
        .await
        .expect("the replacement page must be readable");
    assert_eq!(identity.url, "about:blank", "page was not replaced");
    assert!(!identity.url.contains(CODE) && !identity.url.contains(STATE));
}

/// Run only in the confined pinned Google-auth image with no external network.
#[tokio::test]
#[ignore = "requires the confined pinned Google-auth image; run with MOOSURI_CLAUDE_TEST_CASE"]
async fn actual_chrome_binds_private_claude_callback() {
    let case = std::env::var("MOOSURI_CLAUDE_TEST_CASE").unwrap_or_else(|_| "valid".into());
    assert!(CASES.contains(&case.as_str()), "unknown synthetic scenario");
    let case = case.as_str();
    let cancel = CancellationToken::new();
    let mut workspace = tokio::time::timeout(
        Duration::from_secs(60),
        launch_synthetic(&page_for(case), cancel.clone()),
    )
    .await
    .expect("launch timed out")
    .expect("confined Chrome and private X did not start");

    match case {
        "valid" => {
            let results =
                watch_until(&mut workspace, 20, |p| matches!(p, Phase::Captured(_))).await;
            assert!(
                matches!(workspace.phase, Phase::Captured(_)),
                "exact top-level callback was not captured (phase {}, {} watches, errors {})",
                phase_name(&workspace.phase),
                results.len(),
                results.iter().filter(|r| r.is_err()).count()
            );
            assert!(results.iter().all(Result::is_ok));
            // While a code is held no frame can be taken, so nothing renders it.
            assert!(matches!(
                workspace.human_frame().await,
                Err(BrowserError::Expired)
            ));
            let code = workspace
                .captured_code()
                .await
                .expect("captured code was not handed out");
            assert_eq!(code.as_str(), format!("{CODE}#{STATE}"));
            assert!(workspace.captured_code().await.is_none(), "code replayed");
            assert!(matches!(workspace.phase, Phase::Delivered));
            assert_blank_and_free_of_the_code(&mut workspace).await;
            assert!(matches!(
                workspace.human_frame().await,
                Err(BrowserError::Expired)
            ));
            assert!(matches!(
                workspace
                    .human_input(
                        &"a".repeat(64),
                        WorkspaceInput::Key {
                            key: WorkspaceKey::Enter
                        }
                    )
                    .await,
                Err(BrowserError::Expired)
            ));
            // Further watching stays clean and never produces another code.
            assert!(
                watch_until(&mut workspace, 2, |_| false)
                    .await
                    .iter()
                    .all(Result::is_ok)
            );
            assert!(workspace.captured_code().await.is_none());
        }
        "wrong_state" => {
            let results = watch_until(&mut workspace, 20, |p| matches!(p, Phase::Failed)).await;
            assert!(
                matches!(workspace.phase, Phase::Failed),
                "wrong state was not rejected (phase {})",
                phase_name(&workspace.phase)
            );
            assert!(
                results
                    .iter()
                    .any(|r| matches!(r, Err(BrowserError::Protocol)))
            );
            assert!(workspace.captured_code().await.is_none());
            assert!(matches!(
                workspace.watch().await,
                Err(BrowserError::Expired)
            ));
            assert!(matches!(
                workspace.human_frame().await,
                Err(BrowserError::Expired)
            ));
        }
        "iframe" | "popup" | "subresource" => {
            let results = watch_until(&mut workspace, 6, |p| !matches!(p, Phase::Waiting)).await;
            assert!(
                !results.is_empty(),
                "the callback watcher never ran for {case}"
            );
            // A callback that is not our top-level document is never adopted.
            assert!(!matches!(
                workspace.phase,
                Phase::Captured(_) | Phase::Delivered
            ));
            assert!(workspace.captured_code().await.is_none());
            if case != "popup" {
                // Iframe requests are aborted and subresources are never paused at all.
                assert!(matches!(workspace.phase, Phase::Waiting), "{case}");
                assert!(results.iter().all(Result::is_ok));
            }
        }
        _ => {
            // cancel: the page never calls back; cancellation must stop all use.
            let results = watch_until(&mut workspace, 3, |p| !matches!(p, Phase::Waiting)).await;
            assert!(matches!(workspace.phase, Phase::Waiting));
            assert!(results.iter().all(Result::is_ok));
            cancel.cancel();
            assert!(matches!(
                workspace.human_frame().await,
                Err(BrowserError::Cancelled)
            ));
            assert!(workspace.captured_code().await.is_none());
        }
    }
    // Awaited teardown of Chrome, its profile and the private X server, in every case.
    tokio::time::timeout(Duration::from_secs(30), workspace.shutdown())
        .await
        .expect("shutdown did not finish");
}
