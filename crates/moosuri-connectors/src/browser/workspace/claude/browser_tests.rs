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
    let mut this = ClaudeWorkspace::assemble(
        BrowserWorkspace {
            session,
            current: None,
            proof: None,
            prior_proof: None,
            ignored_targets: HashSet::new(),
            last_links: None,
            human_interacted: false,
            private_auth: true,
        },
        Some(display),
        AuthorizeUrl::parse(AUTHORIZE).unwrap(),
    );
    // The production initialization (Fetch rule, auto-attach, baseline), then only the
    // first navigation differs: a synthetic document instead of the authorize URL.
    let opened = async {
        this.prepare().await?;
        this.navigate(&data_url(html)).await
    }
    .await;
    if let Err(error) = opened {
        this.shutdown().await;
        return Err(error);
    }
    Ok(this)
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

const SSO_CASES: [&str; 4] = [
    "return",
    "callback_popup",
    "root_completes",
    "cancel_with_popup",
];

/// The root shows one full-viewport button. Its click handler (a real user gesture,
/// delivered through the proof-bound human input) opens a same-origin `about:blank`
/// window whose only control immediately closes it; or opens the exact
/// callback URL in a window; and optionally completes the root callback in the
/// background while that window is displayed.
fn sso_page(case: &str) -> String {
    let good = serde_json::to_string(&callback(CODE, STATE)).unwrap();
    let window = if case == "callback_popup" {
        format!("window.open({good});")
    } else {
        "const w=window.open('about:blank');w.document.write(\"<input id='entry' style='position:fixed;left:440px;top:150px;width:400px;height:40px;z-index:2'><button style='position:fixed;left:0;top:0;width:100%;height:100%' onclick='window.close()'>close</button>\");w.document.close();".to_owned()
    };
    let later = if case == "root_completes" {
        format!("setTimeout(()=>location.replace({good}),800);")
    } else {
        String::new()
    };
    format!(
        "<button style='position:fixed;left:0;top:0;width:100%;height:100%' onclick='go()'>go</button><script>function go(){{{window}{later}}}</script>"
    )
}

async fn wait_for(
    workspace: &mut ClaudeWorkspace,
    seconds: u64,
    done: impl Fn(&ClaudeWorkspace) -> bool,
) -> bool {
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        let _ = workspace.watch().await;
        if done(workspace) {
            return true;
        }
    }
    false
}

async fn click_displayed(workspace: &mut ClaudeWorkspace) -> String {
    let frame = workspace
        .human_frame()
        .await
        .expect("the displayed page must be capturable");
    workspace
        .human_input(&frame.frame_token, WorkspaceInput::Click { x: 640, y: 450 })
        .await
        .expect("proof-bound input was refused");
    frame.frame_token
}

/// Real user-gesture `window.open` in the confined Chrome (one scenario per process,
/// `MOOSURI_CLAUDE_SSO_CASE`). Same confinement and synthetic-only documents as above.
#[tokio::test]
#[ignore = "requires the confined pinned Google-auth image; run with MOOSURI_CLAUDE_SSO_CASE"]
async fn actual_chrome_follows_sso_window_and_keeps_root_callback_contract() {
    let case = std::env::var("MOOSURI_CLAUDE_SSO_CASE").unwrap_or_else(|_| "return".into());
    assert!(SSO_CASES.contains(&case.as_str()), "unknown SSO scenario");
    let case = case.as_str();
    let cancel = CancellationToken::new();
    let mut workspace = tokio::time::timeout(
        Duration::from_secs(60),
        launch_synthetic("", cancel.clone()),
    )
    .await
    .expect("launch timed out")
    .expect("confined Chrome and private X did not start");
    let root = workspace.root_page.expect("root page");
    // Human frames deliberately reject data: URLs. Keep the synthetic document in
    // the permitted blank root; production URL validation stays unchanged.
    workspace.navigate("about:blank").await.unwrap();
    let sid = workspace.inner.sid().unwrap();
    workspace
        .inner
        .session
        .cdp
        .command(
            "Page.setDocumentContent",
            json!({"frameId":workspace.top_frame,"html":sso_page(case)}),
            Some(&sid),
        )
        .await
        .unwrap();
    // Let the synthetic document load, then click it like the owner would.
    watch_until(&mut workspace, 2, |_| false).await;
    click_displayed(&mut workspace).await;
    assert!(
        wait_for(&mut workspace, 15, |w| if case == "callback_popup" {
            w.nonroot_intercepts >= 1
        } else {
            w.popups.len() == 1
        })
        .await,
        "the user-gesture window was not admitted"
    );
    assert!(matches!(
        workspace.phase,
        Phase::Waiting | Phase::Captured(_)
    ));
    if case != "callback_popup" {
        assert_ne!(
            workspace.inner.current.as_ref().map(|h| h.page),
            Some(root),
            "the admitted window must be the displayed page"
        );
    }

    match case {
        "return" => {
            let previous_popup = workspace.inner.current.as_ref().unwrap().page;
            let frame = workspace.human_frame().await.unwrap();
            workspace
                .human_input(&frame.frame_token, WorkspaceInput::Click { x: 640, y: 170 })
                .await
                .unwrap();
            let frame = workspace.human_frame().await.unwrap();
            workspace
                .human_input(
                    &frame.frame_token,
                    WorkspaceInput::InsertText {
                        text: "synthetic-login".into(),
                    },
                )
                .await
                .unwrap();
            let sid = workspace.inner.sid().unwrap();
            let value = workspace.inner.session.cdp.command("Runtime.evaluate",
                json!({"expression":"document.getElementById('entry')?.value === 'synthetic-login'","returnByValue":true}), Some(&sid)).await.unwrap();
            assert_eq!(
                value.pointer("/result/value"),
                Some(&json!(true)),
                "input reached the shown popup"
            );
            let frame = workspace.human_frame().await.unwrap();
            let result = workspace
                .human_input(&frame.frame_token, WorkspaceInput::Click { x: 640, y: 450 })
                .await;
            assert!(
                matches!(result, Ok(()) | Err(BrowserError::InputUncertain)),
                "closing input must be acknowledged or explicitly uncertain"
            );
            let token = frame.frame_token;
            assert!(
                wait_for(&mut workspace, 15, |w| w.popups.is_empty()).await,
                "closing the window was not noticed"
            );
            assert!(matches!(workspace.phase, Phase::Waiting));
            assert_eq!(workspace.inner.current.as_ref().map(|h| h.page), Some(root));
            // Returning to the root invalidated the window's frame proof.
            assert!(matches!(
                workspace
                    .human_input(&token, WorkspaceInput::Click { x: 1, y: 1 })
                    .await,
                Err(BrowserError::StalePage)
            ));
            workspace.human_frame().await.expect("root is shown again");
            click_displayed(&mut workspace).await;
            assert!(wait_for(&mut workspace, 15, |w| w.popups.len() == 1).await);
            assert_ne!(
                workspace.inner.current.as_ref().unwrap().page,
                previous_popup
            );
            let sid = workspace.inner.sid().unwrap();
            let value = workspace.inner.session.cdp.command("Runtime.evaluate",
                json!({"expression":"document.getElementById('entry')?.value === ''","returnByValue":true}), Some(&sid)).await.unwrap();
            assert_eq!(
                value.pointer("/result/value"),
                Some(&json!(true)),
                "reopened window contains no previous input"
            );
        }
        "callback_popup" => {
            // The popup callback is intercepted before network, answered blank and
            // closed so a browser error page cannot print the callback address.
            // Proof of interception: Chrome paused the window's callback document in
            // its own session and the Fetch abort was acknowledged (counted only).
            assert!(
                wait_for(&mut workspace, 10, |w| w.nonroot_intercepts >= 1).await,
                "the window's callback request was not aborted before the network"
            );
            assert!(matches!(workspace.phase, Phase::Waiting));
            assert!(workspace.popups.is_empty());
            assert_eq!(workspace.inner.current.as_ref().map(|h| h.page), Some(root));
            assert!(workspace.captured_code().await.is_none());
            workspace
                .human_frame()
                .await
                .expect("the window shows no code and stays inspectable");
        }
        "root_completes" => {
            assert!(
                wait_for(&mut workspace, 20, |w| matches!(
                    w.phase,
                    Phase::Captured(_)
                ))
                .await,
                "the background root callback was not captured"
            );
            assert_eq!(workspace.popups.len(), 1, "window was shown meanwhile");
            let code = workspace
                .captured_code()
                .await
                .expect("captured code was not handed out");
            assert_eq!(code.as_str(), format!("{CODE}#{STATE}"));
            assert!(workspace.captured_code().await.is_none(), "code replayed");
            assert!(workspace.popups.is_empty());
            assert_eq!(workspace.inner.session.targets.len(), 1);
            assert_blank_and_free_of_the_code(&mut workspace).await;
        }
        _ => {
            cancel.cancel();
            assert!(matches!(
                workspace.human_frame().await,
                Err(BrowserError::Cancelled)
            ));
            assert!(workspace.captured_code().await.is_none());
        }
    }
    tokio::time::timeout(Duration::from_secs(30), workspace.shutdown())
        .await
        .expect("shutdown did not finish");
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
