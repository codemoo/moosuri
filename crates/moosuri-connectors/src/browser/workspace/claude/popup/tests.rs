//! Synthetic CDP scenarios for SSO window admission. The scripted browser never
//! navigates anywhere; it only emits attach/detach and paused-request events.
use super::super::tests::{
    CODE, Fixture, Hold, Log, STATE, callback, event, fixture, methods, pump_for_a_while,
};
use super::*;

fn attach(n: &str, opener: Option<&str>, can_access: bool, waiting: bool) -> Value {
    json!({"method":"Target.attachedToTarget","params":{
        "sessionId":format!("popup{n}"),"waitingForDebugger":waiting,
        "targetInfo":{"targetId":format!("ptarget{n}"),"type":"page","openerId":opener,
            "canAccessOpener":can_access,"url":"https://accounts.google.com/"}}})
}
fn detach(n: &str) -> Value {
    json!({"method":"Target.detachedFromTarget","params":{"sessionId":format!("popup{n}")}})
}
fn order(log: &Log) -> Vec<String> {
    log.lock().unwrap().iter().map(|(m, _)| m.clone()).collect()
}
fn closed(log: &Log) -> Vec<String> {
    methods(log, "Target.closeTarget")
        .iter()
        .map(|p| p["targetId"].as_str().unwrap().to_owned())
        .collect()
}
/// A proof-validated click on whatever is displayed; this is what arms a window.
async fn click(workspace: &mut ClaudeWorkspace) -> String {
    let frame = workspace.human_frame().await.unwrap();
    workspace
        .human_input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 })
        .await
        .unwrap();
    frame.frame_token
}
async fn until(workspace: &mut ClaudeWorkspace, done: impl Fn(&ClaudeWorkspace) -> bool) {
    for _ in 0..100 {
        let _ = workspace.watch().await;
        if done(workspace) {
            return;
        }
    }
    panic!("the synthetic window event was not accounted for");
}

fn mouse(log: &Log) -> usize {
    methods(log, "Input.dispatchMouseEvent").len()
}

#[tokio::test]
async fn later_text_scroll_and_tab_cannot_revive_an_expired_click() {
    for input in [
        WorkspaceInput::InsertText {
            text: "synthetic".into(),
        },
        WorkspaceInput::Scroll {
            delta_x: 0,
            delta_y: 20,
        },
        WorkspaceInput::Key {
            key: WorkspaceKey::Tab,
        },
    ] {
        let mut f = fixture().await;
        click(&mut f.workspace).await;
        f.workspace.arm.as_mut().unwrap().until = Instant::now() - Duration::from_secs(1);
        let frame = f.workspace.human_frame().await.unwrap();
        f.workspace
            .human_input(&frame.frame_token, input)
            .await
            .unwrap();
        assert!(f.workspace.arm.is_none());
        f.events
            .send(attach("1", Some("target"), true, true))
            .unwrap();
        pump_for_a_while(&mut f.workspace).await;
        assert!(f.workspace.popups.is_empty());
        assert!(closed(&f.log).contains(&"ptarget1".to_owned()));
        assert!(methods(&f.log, "Runtime.runIfWaitingForDebugger").is_empty());
        f.workspace.shutdown().await;
        f.server.abort();
    }
}
/// Admits window 1 through an ordinary (immediately answered) click.
async fn with_window(f: &mut Fixture) {
    click(&mut f.workspace).await;
    f.events
        .send(attach("1", Some("target"), true, true))
        .unwrap();
    until(&mut f.workspace, |w| w.popups.len() == 1).await;
}
async fn click_result(workspace: &mut ClaudeWorkspace) -> Result<(), BrowserError> {
    let frame = workspace.human_frame().await.unwrap();
    workspace
        .human_input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 })
        .await
}

#[tokio::test]
async fn delayed_input_reply_arrives_during_fetch_configure_and_resume_without_a_resend() {
    let mut f = fixture().await;
    // The click handler (opening the window) is stalled until the paused window is
    // resumed, so Chrome answers `mouseReleased` only after `runIfWaitingForDebugger`.
    {
        let mut script = f.script.lock().unwrap();
        script.hold = Hold::UntilMethod("Runtime.runIfWaitingForDebugger");
        script.on_release = vec![attach("1", Some("target"), true, true)];
    }
    let frame = f.workspace.human_frame().await.unwrap();
    tokio::time::timeout(
        Duration::from_secs(5),
        f.workspace
            .human_input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 }),
    )
    .await
    .expect("the stalled input must not deadlock the attachment")
    .unwrap();
    assert_eq!(f.workspace.popups.len(), 1);
    assert_eq!(f.workspace.displayed_target().as_deref(), Some("ptarget1"));
    // Exactly one press and one release: the pending command was never resent.
    assert_eq!(mouse(&f.log), 2);
    let calls = order(&f.log);
    let at = |name: &str| calls.iter().position(|m| m == name).unwrap();
    let released = calls
        .iter()
        .rposition(|m| m == "Input.dispatchMouseEvent")
        .unwrap();
    assert!(released < at("Fetch.enable"));
    assert!(at("Emulation.setDeviceMetricsOverride") < at("Runtime.runIfWaitingForDebugger"));
    assert!(at("Fetch.enable") < at("Runtime.runIfWaitingForDebugger"));
    assert!(!f.workspace.inner.session.cdp.input_pending());
    assert!(!f.workspace.inner.session.poisoned);
    f.workspace.shutdown().await;
    f.server.abort();
}

#[tokio::test]
async fn refused_input_creates_no_window_authority() {
    let mut f = fixture().await;
    let frame = f.workspace.human_frame().await.unwrap();
    // Wrong token.
    assert!(matches!(
        f.workspace
            .human_input(&"0".repeat(64), WorkspaceInput::Click { x: 1, y: 1 })
            .await,
        Err(BrowserError::StalePage)
    ));
    assert!(f.workspace.arm.is_none());
    // Accepted once, then the consumed token is stale.
    f.workspace
        .human_input(&frame.frame_token, WorkspaceInput::Click { x: 1, y: 1 })
        .await
        .unwrap();
    f.workspace.arm = None;
    assert!(matches!(
        f.workspace
            .human_input(&frame.frame_token, WorkspaceInput::Click { x: 1, y: 1 })
            .await,
        Err(BrowserError::StalePage)
    ));
    assert!(f.workspace.arm.is_none());
    assert_eq!(mouse(&f.log), 2);
    f.events
        .send(attach("1", Some("target"), true, true))
        .unwrap();
    pump_for_a_while(&mut f.workspace).await;
    assert!(f.workspace.popups.is_empty());
    assert!(closed(&f.log).contains(&"ptarget1".to_owned()));
    assert!(methods(&f.log, "Runtime.runIfWaitingForDebugger").is_empty());
    f.workspace.shutdown().await;
    f.server.abort();
}

#[tokio::test]
async fn unanswered_input_is_terminal_on_timeout_and_cancellation_and_never_replayed() {
    // Bounded original timeout.
    let mut f = fixture().await;
    f.script.lock().unwrap().hold = Hold::Forever;
    f.workspace.inner.session.deadline = Instant::now() + Duration::from_millis(400);
    assert!(matches!(
        click_result(&mut f.workspace).await,
        Err(BrowserError::Timeout)
    ));
    assert_eq!(mouse(&f.log), 2);
    assert!(f.workspace.inner.session.poisoned);
    assert!(!f.workspace.inner.session.cdp.input_pending());
    assert!(f.workspace.arm.is_none());
    f.workspace.shutdown().await;
    f.server.abort();

    // Cancellation.
    let mut f = fixture().await;
    f.script.lock().unwrap().hold = Hold::Forever;
    let cancel = f.workspace.inner.session.cancellation.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        cancel.cancel();
    });
    assert!(matches!(
        click_result(&mut f.workspace).await,
        Err(BrowserError::Cancelled)
    ));
    assert_eq!(mouse(&f.log), 2);
    assert!(f.workspace.inner.session.poisoned);
    assert!(!f.workspace.inner.session.cdp.input_pending());
    assert!(matches!(
        f.workspace.human_frame().await,
        Err(BrowserError::Cancelled)
    ));
    assert_eq!(mouse(&f.log), 2);
    f.workspace.shutdown().await;
    f.server.abort();
}

#[tokio::test]
async fn acknowledged_immediate_close_returns_to_the_original_window() {
    let mut f = fixture().await;
    with_window(&mut f).await;
    // The window closes during the release; Chrome still acknowledges the input.
    f.script.lock().unwrap().on_release = vec![detach("1")];
    click(&mut f.workspace).await;
    assert!(f.workspace.popups.is_empty());
    assert!(matches!(f.workspace.phase, Phase::Waiting));
    assert!(!f.workspace.inner.session.poisoned);
    assert_eq!(f.workspace.displayed_target().as_deref(), Some("target"));
    assert_eq!(mouse(&f.log), 4);
    assert_eq!(
        f.workspace.human_frame().await.unwrap().url,
        "https://claude.com"
    );
    f.workspace.shutdown().await;
    f.server.abort();
}

#[tokio::test]
async fn close_before_acknowledgement_is_reported_uncertain_and_keeps_the_root() {
    for hold in [Hold::Forever, Hold::Error] {
        let mut f = fixture().await;
        with_window(&mut f).await;
        {
            let mut script = f.script.lock().unwrap();
            script.hold = hold;
            script.on_release = vec![detach("1")];
        }
        // Not accepted, not fatal: the confirmed surviving root is displayed again.
        assert!(matches!(
            click_result(&mut f.workspace).await,
            Err(BrowserError::InputUncertain)
        ));
        assert_eq!(mouse(&f.log), 4, "nothing is resent");
        assert!(f.workspace.popups.is_empty());
        assert!(matches!(f.workspace.phase, Phase::Waiting));
        assert!(!f.workspace.inner.session.poisoned);
        assert!(!f.workspace.inner.session.cdp.input_pending());
        assert!(f.workspace.arm.is_none());
        assert_eq!(f.workspace.displayed_target().as_deref(), Some("target"));
        f.workspace.human_frame().await.unwrap();
        f.workspace.shutdown().await;
        f.server.abort();
    }
    // An error reply without any detach receipt is a protocol failure, not uncertainty.
    let mut f = fixture().await;
    f.script.lock().unwrap().hold = Hold::Error;
    assert!(matches!(
        click_result(&mut f.workspace).await,
        Err(BrowserError::Protocol)
    ));
    assert!(f.workspace.inner.session.poisoned);
    f.workspace.shutdown().await;
    f.server.abort();
}

#[tokio::test]
async fn gesture_window_is_fenced_before_resume_displayed_and_returned_on_close() {
    let mut f = fixture().await;
    click(&mut f.workspace).await;
    // A fresh, still-valid root proof held while the window opens.
    let root_token = f.workspace.human_frame().await.unwrap().frame_token;
    f.events
        .send(attach("1", Some("target"), true, true))
        .unwrap();
    until(&mut f.workspace, |w| w.popups.len() == 1).await;
    // The callback rule and page settings exist before the paused window is resumed.
    let calls = order(&f.log);
    let at = |name: &str| calls.iter().position(|m| m == name).unwrap();
    assert!(at("Emulation.setDeviceMetricsOverride") < at("Runtime.runIfWaitingForDebugger"));
    assert!(at("Fetch.enable") < at("Runtime.runIfWaitingForDebugger"));
    assert!(at("Runtime.runIfWaitingForDebugger") < at("Target.activateTarget"));
    assert_eq!(
        methods(&f.log, "Fetch.enable")[0]["patterns"][0]["urlPattern"],
        format!("{}*", claude_auth::CALLBACK)
    );
    // The root's frame proof died with the switch: a stale token dispatches nothing.
    let mouse = methods(&f.log, "Input.dispatchMouseEvent").len();
    assert!(matches!(
        f.workspace
            .human_input(&root_token, WorkspaceInput::Click { x: 1, y: 1 })
            .await,
        Err(BrowserError::StalePage)
    ));
    assert_eq!(methods(&f.log, "Input.dispatchMouseEvent").len(), mouse);
    let shown = f.workspace.human_frame().await.unwrap();
    assert_eq!(shown.url, "https://accounts.google.com");
    // Closing the window returns to the root and invalidates the window's proof.
    f.events.send(detach("1")).unwrap();
    until(&mut f.workspace, |w| w.popups.is_empty()).await;
    assert!(matches!(f.workspace.phase, Phase::Waiting));
    assert!(matches!(
        f.workspace
            .human_input(&shown.frame_token, WorkspaceInput::Click { x: 1, y: 1 })
            .await,
        Err(BrowserError::StalePage)
    ));
    assert_eq!(
        f.workspace.human_frame().await.unwrap().url,
        "https://claude.com"
    );
    let activated = methods(&f.log, "Target.activateTarget");
    assert_eq!(activated.last().unwrap()["targetId"], "target");
    f.workspace.shutdown().await;
    f.server.abort();
}

#[tokio::test]
async fn background_root_callback_completes_while_a_window_is_displayed() {
    let mut f = fixture().await;
    click(&mut f.workspace).await;
    f.events
        .send(attach("1", Some("target"), true, true))
        .unwrap();
    until(&mut f.workspace, |w| w.popups.len() == 1).await;
    f.events.send(event("r1", &callback(CODE, STATE))).unwrap();
    until(&mut f.workspace, |w| matches!(w.phase, Phase::Captured(_))).await;
    assert_eq!(f.workspace.displayed_target().as_deref(), Some("ptarget1"));
    assert_eq!(methods(&f.log, "Fetch.fulfillRequest").len(), 1);
    let code = f.workspace.captured_code().await.unwrap();
    assert_eq!(code.as_str(), format!("{CODE}#{STATE}"));
    // Root and window are both gone; auto-attach was stopped before the replacement.
    let gone = closed(&f.log);
    assert!(gone.contains(&"target".to_owned()) && gone.contains(&"ptarget1".to_owned()));
    assert_eq!(
        methods(&f.log, "Target.setAutoAttach").last().unwrap()["autoAttach"],
        false
    );
    assert!(f.workspace.popups.is_empty());
    assert!(
        !serde_json::to_string(&*f.log.lock().unwrap())
            .unwrap()
            .contains(CODE)
    );
    assert!(f.workspace.captured_code().await.is_none());
    f.workspace.shutdown().await;
    f.server.abort();
}

#[tokio::test]
async fn unprompted_foreign_noopener_stale_and_replayed_windows_are_never_resumed() {
    let mut f = fixture().await;
    // No input yet: a page-initiated window has no authority.
    f.events
        .send(attach("1", Some("target"), true, true))
        .unwrap();
    pump_for_a_while(&mut f.workspace).await;
    click(&mut f.workspace).await;
    for event in [
        attach("2", Some("other"), true, true),
        attach("3", None, false, true),
        attach("4", Some("target"), false, true),
        attach("5", Some("target"), true, false),
    ] {
        f.events.send(event).unwrap();
    }
    f.events.send(attach("6", None, false, false)).unwrap();
    pump_for_a_while(&mut f.workspace).await;
    assert!(matches!(f.workspace.phase, Phase::Waiting));
    assert!(f.workspace.popups.is_empty());
    assert_eq!(
        closed(&f.log),
        ["ptarget1", "ptarget2", "ptarget3", "ptarget4", "ptarget5"]
    );
    // A page we never paused is only detached from, not closed or owned.
    assert!(
        methods(&f.log, "Target.detachFromTarget")
            .iter()
            .any(|p| p["sessionId"] == "popup6")
    );
    assert!(methods(&f.log, "Runtime.runIfWaitingForDebugger").is_empty());
    // Refusals did not burn the authority: the real popup is still admitted once.
    f.events
        .send(attach("7", Some("target"), true, true))
        .unwrap();
    until(&mut f.workspace, |w| w.popups.len() == 1).await;
    // Replay: the single admission consumed it, so a second window needs a new input.
    f.events
        .send(attach("8", Some("ptarget7"), true, true))
        .unwrap();
    pump_for_a_while(&mut f.workspace).await;
    assert_eq!(f.workspace.popups.len(), 1);
    assert!(closed(&f.log).contains(&"ptarget8".to_owned()));
    assert_eq!(methods(&f.log, "Runtime.runIfWaitingForDebugger").len(), 1);
    f.workspace.shutdown().await;
    f.server.abort();
}

#[tokio::test]
async fn a_window_callback_is_blank_and_closed_and_only_the_root_may_deliver_the_code() {
    let mut f = fixture().await;
    click(&mut f.workspace).await;
    f.events
        .send(attach("1", Some("target"), true, true))
        .unwrap();
    until(&mut f.workspace, |w| w.popups.len() == 1).await;
    let mut popup_callback = event("p1", &callback(CODE, STATE));
    popup_callback["sessionId"] = json!("popup1");
    popup_callback["params"]["frameId"] = json!("popup1-frame");
    f.events.send(popup_callback).unwrap();
    pump_for_a_while(&mut f.workspace).await;
    assert!(matches!(f.workspace.phase, Phase::Waiting));
    assert!(f.workspace.popups.is_empty());
    assert!(methods(&f.log, "Fetch.continueRequest").is_empty());
    assert!(
        methods(&f.log, "Fetch.fulfillRequest")
            .iter()
            .any(|p| p["requestId"] == "p1")
    );
    assert!(f.workspace.captured_code().await.is_none());
    // The root's exact callback still works, once.
    f.events.send(event("r1", &callback(CODE, STATE))).unwrap();
    until(&mut f.workspace, |w| matches!(w.phase, Phase::Captured(_))).await;
    assert!(f.workspace.captured_code().await.is_some());
    assert!(f.workspace.captured_code().await.is_none());
    f.workspace.shutdown().await;
    f.server.abort();
}

#[tokio::test]
async fn depth_overflow_fails_closed_and_cancellation_stops_everything() {
    let mut f = fixture().await;
    click(&mut f.workspace).await;
    f.events
        .send(attach("1", Some("target"), true, true))
        .unwrap();
    until(&mut f.workspace, |w| w.popups.len() == 1).await;
    click(&mut f.workspace).await;
    // The displayed window is the opener now; the hidden root cannot spawn one.
    f.events
        .send(attach("9", Some("target"), true, true))
        .unwrap();
    f.events
        .send(attach("2", Some("ptarget1"), true, true))
        .unwrap();
    until(&mut f.workspace, |w| w.popups.len() == 2).await;
    assert!(closed(&f.log).contains(&"ptarget9".to_owned()));
    click(&mut f.workspace).await;
    f.events
        .send(attach("3", Some("ptarget2"), true, true))
        .unwrap();
    pump_for_a_while(&mut f.workspace).await;
    assert!(matches!(f.workspace.phase, Phase::Failed));
    assert!(closed(&f.log).contains(&"ptarget3".to_owned()));
    assert!(matches!(
        f.workspace.watch().await,
        Err(BrowserError::Expired)
    ));
    assert!(f.workspace.captured_code().await.is_none());
    f.workspace.shutdown().await;
    f.server.abort();

    // Cancellation with a window open: no further frame, input or code.
    let mut f = fixture().await;
    click(&mut f.workspace).await;
    f.events
        .send(attach("1", Some("target"), true, true))
        .unwrap();
    until(&mut f.workspace, |w| w.popups.len() == 1).await;
    f.workspace.inner.session.cancellation.cancel();
    assert!(matches!(
        f.workspace.human_frame().await,
        Err(BrowserError::Cancelled)
    ));
    assert!(f.workspace.captured_code().await.is_none());
    f.workspace.shutdown().await;
    f.server.abort();
}
