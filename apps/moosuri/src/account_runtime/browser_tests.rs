use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
};
use tower::ServiceExt;

struct Member {
    store: Store,
    owner: VolumeOwner,
    token: String,
    thread: String,
}
async fn member(path: &std::path::Path) -> Member {
    fs::create_dir(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    let owner = VolumeOwner {
        account: Uuid::new_v4(),
        tenant: Uuid::new_v4(),
        volume: Uuid::new_v4(),
        generation: 1,
    };
    owner
        .initialize(
            path,
            "Member".into(),
            "member@example.test".into(),
            "Temporary-synthetic-42".into(),
        )
        .await
        .unwrap();
    let store = owner.open_verified(path).await.unwrap();
    let login = store
        .login(
            "member@example.test".into(),
            "Temporary-synthetic-42".into(),
        )
        .await
        .unwrap();
    let login = store
        .change_password(&login.user, "Second-synthetic-password-42".into())
        .await
        .unwrap();
    let thread = store
        .create_thread(&login.user, "Synthetic timetable".into())
        .await
        .unwrap()
        .id;
    Member {
        store,
        owner,
        token: login.token,
        thread,
    }
}
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    owner: Uuid,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "personal.internal")
                .header("origin", "https://personal.internal")
                .header("content-type", "application/json")
                .header("x-moosuri-account", owner.to_string())
                .header("x-moosuri-session-epoch", "3")
                .header("cookie", format!("moosuri_session={token}"))
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let code = resp.status();
    let bytes = to_bytes(resp.into_body(), 1 << 22).await.unwrap();
    (code, serde_json::from_slice(&bytes).unwrap_or_default())
}
async fn wait_state(app: &Router, path: &str, m: &Member, state: &str) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let result = request(app, "GET", path, m.owner.account, &m.token, Value::Null)
                .await
                .1;
            if result["workspace"]["state"] == state {
                return result;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
fn listener(socket: &std::path::Path) -> UnixListener {
    let l = UnixListener::bind(socket).unwrap();
    fs::set_permissions(socket, fs::Permissions::from_mode(0o600)).unwrap();
    l
}
fn browser_fixture(path: &std::path::Path) -> Arc<AtomicUsize> {
    let l = listener(path);
    let counter = Arc::new(AtomicUsize::new(0));
    let seen = counter.clone();
    tokio::spawn(async move {
        let tab = Uuid::new_v4();
        let url = "https://example.org/timetable?worker_token=CANARY#secret";
        let mut filtered = false;
        let detail = Uuid::new_v4();
        let mut active = tab;
        let mut busy_tabs = 0;
        let mut busy_stop = true;
        let mut busy_input = true;
        let mut checks = 0;
        loop {
            let (stream, _) = l.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            let r: Value = serde_json::from_str(&line).unwrap();
            let observation = || json!({"page":{"title":"Timetable","url":url,"text":if filtered{"Morning bus 08:15"}else{"Afternoon bus 14:30"},"warning":"","retrieved_at_unix_ms":1234,"links":[{"text":"Timetable details","url":"https://example.org/detail"}],"truncated":false}});
            if (r["op"] == "stop" && std::mem::take(&mut busy_stop))
                || (r["op"] == "input" && std::mem::take(&mut busy_input))
            {
                stream
                    .write_all(b"{\"ok\":false,\"code\":\"capacity\"}\n")
                    .await
                    .unwrap();
                continue;
            }
            if r["op"] == "tabs" {
                checks += 1;
                if checks == 2 && active == tab {
                    busy_tabs = 2;
                }
                if busy_tabs > 0 {
                    busy_tabs -= 1;
                    stream
                        .write_all(b"{\"ok\":false,\"code\":\"capacity\"}\n")
                        .await
                        .unwrap();
                    continue;
                }
            }
            let value = match r["op"].as_str().unwrap() {
                "open" => json!({"tab_id":tab,"observation":observation()}),
                "follow" => {
                    assert_eq!(r["url"], "https://example.org/detail");
                    assert_eq!(r["tab_id"], tab.to_string());
                    seen.fetch_add(1, Ordering::SeqCst);
                    active = detail;
                    json!({"tab_id":detail,"observation":{"page":{"url":"https://example.org/detail","title":"Details","text":"Platform 7","links":[],"truncated":false,"warning":"","retrieved_at_unix_ms":1235}}})
                }
                "frame" => {
                    json!({"url":url,"width":1280,"height":900,"frame_token":"f".repeat(64),"png_base64":"iVBORw0KGgoAAAANSUhEUgAABQAAAAOECAIAAADDAAAA"})
                }
                "tabs" if active == detail => {
                    json!({"active_tab":active,"tabs":[{"tab_id":tab,"url":url},{"tab_id":detail,"url":"https://example.org/detail"}]})
                }
                "tabs" => json!({"active_tab":tab,"tabs":[{"tab_id":tab,"url":url}]}),
                "input" => {
                    assert_eq!(r["input"]["kind"], "click");
                    filtered = true;
                    json!({"accepted":true})
                }
                "selected" => {
                    assert!(filtered);
                    seen.fetch_add(1, Ordering::SeqCst);
                    observation()
                }
                "stop" => {
                    seen.fetch_add(100, Ordering::SeqCst);
                    json!({"stopped":true})
                }
                _ => panic!("unexpected operation"),
            };
            stream
                .write_all(format!("{}\n", json!({"ok":true,"result":value})).as_bytes())
                .await
                .unwrap();
        }
    });
    counter
}
fn model_fixture(
    path: &std::path::Path,
) -> (moosuri_server::personal_text::Options, Arc<AtomicUsize>) {
    let l = listener(path);
    let count = Arc::new(AtomicUsize::new(0));
    let calls = count.clone();
    tokio::spawn(async move {
        let rev = Uuid::new_v4();
        loop {
            let (stream, _) = l.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            let r: Value = serde_json::from_str(&line).unwrap();
            let value = if r["operation"] == "selection" {
                json!({"v":1,"state":"selection","connection_revision":rev,"selection_epoch":3,"model":"fixture","effort":null,"kind":"compatible"})
            } else {
                calls.fetch_add(1, Ordering::SeqCst);
                let context = r["messages"][1]["text"].as_str().unwrap();
                assert!(!context.contains("CANARY"));
                let answer = if context.contains("Find platform details") {
                    if r["ordinal"] == 0 {
                        assert_eq!(
                            serde_json::from_str::<Value>(context).unwrap()["public_links"],
                            json!([{"index":0,"title":"Timetable details"}])
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        json!({"answer":"Check the time and platform on its details page.","needs_user":false,"follow_index":0})
                    } else {
                        assert!(
                            context.contains("Platform 7")
                                && context.contains("Afternoon bus 14:30")
                        );
                        json!({"answer":"The 14:30 bus leaves from platform 7.","needs_user":false})
                    }
                } else {
                    assert!(
                        context.contains("Morning bus 08:15") && context.contains("morning only")
                    );
                    assert!(
                        !context.contains("14:30")
                            && !context.contains("frame_token")
                            && !context.contains("insert_text")
                    );
                    let data: Value = serde_json::from_str(context).unwrap();
                    assert_eq!(data["public_links"], json!([]));
                    json!({"answer":"The morning bus leaves at 08:15.","needs_user":false,"follow_index":0})
                };
                json!({"v":1,"state":"settled","call_id":r["call_id"],"connection_revision":rev,"model":"fixture","effort":null,"outcome":"completed","output":answer.to_string(),"receipt":{"state":"settled","input_tokens":90,"output_tokens":12,"charged_tokens":102}})
            };
            stream
                .write_all(format!("{value}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    (
        moosuri_server::personal_text::Options {
            socket: path.into(),
            broker_uid: unsafe { libc::geteuid() },
            max_output_tokens: 256,
        },
        count,
    )
}
#[tokio::test]
async fn private_browser_keeps_selected_page_guidance_and_one_answer_on_original_turn() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let apath = root.join("a");
    let bpath = root.join("b");
    let a = member(&apath).await;
    let b = member(&bpath).await;
    let socket = root.join("browser.sock");
    let reads = browser_fixture(&socket);
    let (model, _) = model_fixture(&root.join("model.sock"));
    let app = moosuri_server::personal_service_router(
        a.store.clone(),
        None,
        routes(
            a.store.clone(),
            a.owner,
            Some(socket),
            unsafe { libc::geteuid() },
            Some(model),
        ),
    );
    let bapp = moosuri_server::personal_service_router(
        b.store.clone(),
        None,
        routes(b.store.clone(), b.owner, None, 0, None),
    );
    let base = format!("/api/v1/threads/{}/browser", a.thread);
    assert_eq!(
        request(&bapp, "GET", &base, b.owner.account, &b.token, Value::Null)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&app, "GET", &base, a.owner.account, &b.token, Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &bapp,
            "GET",
            &format!("/api/v1/threads/{}/browser", b.thread),
            b.owner.account,
            &b.token,
            Value::Null
        )
        .await
        .1["configured"],
        false
    );
    let start = json!({"request_id":Uuid::new_v4(),"text":"Read https://example.org/timetable?secret=CANARY#password","url":"https://example.org/timetable?secret=CANARY#password","wait_for_user":true});
    let call = async |token: String, path: String, body: Value| {
        request(&app, "POST", &path, a.owner.account, &token, body).await
    };
    let (ok, admitted) = call(a.token.clone(), base.clone(), start.clone()).await;
    assert_eq!(ok, StatusCode::ACCEPTED);
    assert_eq!(admitted["workspace"]["state"], "starting");
    let opened = wait_state(&app, &base, &a, "ready").await;
    let v = &opened["workspace"];
    assert_eq!(v["state"], "ready");
    assert_eq!(
        call(a.token.clone(), base.clone(), start).await.1["workspace"]["id"],
        v["id"]
    );
    let version = |v: &Value| json!({"workspace_id":v["id"],"revision":v["revision"],"request_id":Uuid::new_v4()});
    let first = call(a.token.clone(), format!("{base}/takeover"), version(v))
        .await
        .1;
    let second = a
        .store
        .login(
            "member@example.test".into(),
            "Second-synthetic-password-42".into(),
        )
        .await
        .unwrap();
    let active = call(
        second.token.clone(),
        format!("{base}/takeover"),
        version(&first["workspace"]),
    )
    .await
    .1;
    let mut input = version(&active["workspace"]);
    input["control_token"] = first["control_token"].clone();
    input["frame_token"] = json!("f".repeat(64));
    input["input"] = json!({"kind":"click","x":50,"y":60});
    assert_eq!(
        call(a.token.clone(), format!("{base}/input"), input.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    input["control_token"] = active["control_token"].clone();
    // A host capture temporarily occupied the mutation slot. Neither rejection consumes
    // this frame/controller nor creates an ambiguous physical effect.
    let stop = version(&active["workspace"]);
    assert_eq!(
        call(second.token.clone(), format!("{base}/close"), stop)
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        call(second.token.clone(), format!("{base}/input"), input.clone())
            .await
            .0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let retained = request(
        &app,
        "GET",
        &base,
        a.owner.account,
        &second.token,
        Value::Null,
    )
    .await
    .1;
    assert_eq!(
        retained["workspace"]["revision"],
        active["workspace"]["revision"]
    );
    assert_eq!(retained["workspace"]["state"], "user_control");
    let (ok, changed) = call(second.token.clone(), format!("{base}/input"), input.clone()).await;
    assert_eq!(ok, StatusCode::OK);
    assert_eq!(
        call(second.token.clone(), format!("{base}/input"), input)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let guide = json!({"request_id":Uuid::new_v4(),"text":"morning only example.org/info?grant=CANARY#secret","workspace_id":v["id"]});
    assert_eq!(
        call(second.token.clone(), base.clone(), guide).await.0,
        StatusCode::OK
    );
    let f = request(
        &app,
        "GET",
        &format!("{base}/frame"),
        a.owner.account,
        &second.token,
        Value::Null,
    )
    .await
    .1;
    assert_eq!(f["url"], "https://example.org/timetable");
    let mut resume = version(&changed["workspace"]);
    resume["control_token"] = active["control_token"].clone();
    resume["frame_token"] = f["frame_token"].clone();
    let settled = call(
        second.token.clone(),
        format!("{base}/resume"),
        resume.clone(),
    )
    .await;
    assert_eq!(settled.0, StatusCode::OK);
    assert_eq!(settled.1["workspace"]["state"], "completed");
    assert_eq!(
        call(second.token.clone(), format!("{base}/resume"), resume)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    let history = a
        .store
        .conversation(&second.user, a.thread.clone(), None)
        .await
        .unwrap();
    assert_eq!(history.turns.len(), 1);
    let turn = &history.turns[0];
    assert!(!turn.user_text.contains("CANARY"));
    assert!(!v["url"].as_str().unwrap().contains("CANARY"));
    assert_eq!(turn.run.id, v["run_id"]);
    assert_eq!(turn.assistant.len(), 1);
    assert!(turn.assistant[0].text.contains("08:15") && !turn.assistant[0].text.contains("14:30"));
    assert_eq!(turn.run.instructions.as_ref().unwrap()[0].state, "applied");
    assert!(
        !turn.run.instructions.as_ref().unwrap()[0]
            .text
            .contains("CANARY")
    );
    a.store.logout(&second.user).await.unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("{base}/frame"),
            a.owner.account,
            &second.token,
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let closed = request(&app, "GET", &base, a.owner.account, &a.token, Value::Null)
        .await
        .1;
    assert_eq!(closed["workspace"]["state"], "closed");
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("{base}/frame"),
            a.owner.account,
            &a.token,
            Value::Null
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    tokio::time::timeout(std::time::Duration::from_secs(13), async {
        while reads.load(Ordering::SeqCst) < 101 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn private_browser_frames_while_model_chooses_observed_public_detail() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let a = member(&root.join("a")).await;
    let socket = root.join("browser.sock");
    let follows = browser_fixture(&socket);
    let (model, model_calls) = model_fixture(&root.join("model.sock"));
    let app = moosuri_server::personal_service_router(
        a.store.clone(),
        None,
        routes(
            a.store.clone(),
            a.owner,
            Some(socket),
            unsafe { libc::geteuid() },
            Some(model),
        ),
    );
    let base = format!("/api/v1/threads/{}/browser", a.thread);
    let body = json!({"request_id":Uuid::new_v4(),"text":"Find platform details","url":"https://example.org/timetable"});
    let open = request(&app, "POST", &base, a.owner.account, &a.token, body.clone());
    let watch = async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let v = request(&app, "GET", &base, a.owner.account, &a.token, Value::Null)
                    .await
                    .1;
                if v["workspace"]["state"] == "reading" {
                    assert_eq!(
                        request(
                            &app,
                            "GET",
                            &format!("{base}/frame"),
                            a.owner.account,
                            &a.token,
                            Value::Null
                        )
                        .await
                        .0,
                        StatusCode::OK
                    );
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
            }
        })
        .await
        .unwrap()
    };
    let ((status, _), ()) = tokio::join!(open, watch);
    assert_eq!(status, StatusCode::ACCEPTED);
    let active = wait_state(&app, &base, &a, "completed").await;
    assert_eq!(active["workspace"]["state"], "completed");
    assert_eq!(active["workspace"]["tabs"].as_array().unwrap().len(), 2);
    assert_eq!(
        request(&app, "POST", &base, a.owner.account, &a.token, body)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(follows.load(Ordering::SeqCst), 1);
    let u = a.store.authenticate(a.token.clone()).await.unwrap();
    let history = a
        .store
        .conversation(&u, a.thread.clone(), None)
        .await
        .unwrap();
    assert_eq!(history.turns.len(), 1);
    assert!(history.turns[0].assistant[0].text.contains("platform 7"));
    // A second task receives human control while a delayed model is proposing its link.
    // The already dispatched result has no authority over the new controller or its tab.
    let next = json!({"request_id":Uuid::new_v4(),"url":"https://example.org/timetable","text":"Find platform details"});
    assert_eq!(
        request(&app, "POST", &base, a.owner.account, &a.token, next)
            .await
            .0,
        StatusCode::ACCEPTED
    );
    let reading = wait_state(&app, &base, &a, "reading").await;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while model_calls.load(Ordering::SeqCst) < 3 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let take = json!({"workspace_id":reading["workspace"]["id"],"revision":reading["workspace"]["revision"],"request_id":Uuid::new_v4()});
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{base}/takeover"),
            a.owner.account,
            &a.token,
            take
        )
        .await
        .0,
        StatusCode::OK
    );
    tokio::time::sleep(std::time::Duration::from_millis(450)).await;
    let held = request(&app, "GET", &base, a.owner.account, &a.token, Value::Null)
        .await
        .1;
    assert_eq!(held["workspace"]["state"], "user_control");
    assert_eq!(follows.load(Ordering::SeqCst) % 100, 1);
    a.store.delete_thread(&u, a.thread).await.unwrap();
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("{base}/frame"),
            a.owner.account,
            &a.token,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    tokio::time::timeout(std::time::Duration::from_secs(13), async {
        while follows.load(Ordering::SeqCst) < 101 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
