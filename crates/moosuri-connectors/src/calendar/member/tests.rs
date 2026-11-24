use super::*;
use axum::{
    Router, body::to_bytes, extract::Request, http::StatusCode, response::IntoResponse,
    routing::any,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
const PRIMARY: &str = "member@example.test";
const OP: &str = "00000000-0000-4000-8000-0000000000aa";
const DIGEST: &str = "d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0d0";

#[derive(Default)]
struct Srv {
    events: HashMap<String, Value>,
    posts: usize,
    patches: usize,
    seq: u32,
    /// Apply the write, then answer 500: the acknowledgement is lost.
    drop_ack: bool,
    /// A concurrent writer wins between our GET and PATCH.
    race_412: bool,
    log: Vec<(String, String, Option<String>)>,
}
fn token() -> SecretString {
    SecretString::new("synthetic-member-calendar-token".into()).unwrap()
}
fn fields(title: &str, start: &str, end: &str) -> CalendarFields {
    CalendarFields {
        title: title.into(),
        description: String::new(),
        location: String::new(),
        start: start.into(),
        end: end.into(),
        time_zone: Some("Asia/Seoul".into()),
    }
}
fn json_response(v: Value) -> axum::response::Response {
    (
        [(reqwest::header::CONTENT_TYPE.as_str(), "application/json")],
        v.to_string(),
    )
        .into_response()
}
fn seed(srv: &Arc<Mutex<Srv>>, id: &str, title: &str, start: &str, end: &str) {
    let mut s = srv.lock().unwrap();
    s.seq += 1;
    let etag = format!("\"v{}\"", s.seq);
    s.events.insert(id.into(), json!({"id":id,"etag":etag,"status":"confirmed","summary":title,"description":"","location":"","start":{"dateTime":start,"timeZone":"Asia/Seoul"},"end":{"dateTime":end,"timeZone":"Asia/Seoul"},"organizer":{"self":true,"email":PRIMARY}}));
}
async fn serve(srv: Arc<Mutex<Srv>>) -> (MemberCalendar, tokio::task::JoinHandle<()>) {
    let state = srv.clone();
    let app = Router::new().fallback(any(move |request: Request| {
        let srv = state.clone();
        async move {
            let method = request.method().to_string();
            let uri = request.uri().to_string();
            let if_match = request
                .headers()
                .get("if-match")
                .map(|v| v.to_str().unwrap().to_owned());
            assert_eq!(
                request.headers()["authorization"],
                "Bearer synthetic-member-calendar-token"
            );
            let body = to_bytes(request.into_body(), 65536).await.unwrap();
            let body: Value = if body.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&body).unwrap()
            };
            let mut s = srv.lock().unwrap();
            s.log.push((method.clone(), uri.clone(), if_match.clone()));
            let path = uri.split('?').next().unwrap().to_owned();
            let id = path.rsplit('/').next().unwrap().to_owned();
            match method.as_str() {
                "GET" if path == "/calendars/primary/events" => {
                    let mut items: Vec<Value> = s.events.values().cloned().collect();
                    items.sort_by_key(|v| v["start"]["dateTime"].to_string());
                    json_response(json!({"items":items}))
                }
                "GET" => match s.events.get(&id) {
                    Some(v) => json_response(v.clone()),
                    None => StatusCode::NOT_FOUND.into_response(),
                },
                "POST" => {
                    s.posts += 1;
                    let id = body["id"].as_str().unwrap().to_owned();
                    if s.events.contains_key(&id) {
                        return StatusCode::CONFLICT.into_response();
                    }
                    s.seq += 1;
                    let mut v = json!({"status":"confirmed","etag":format!("\"v{}\"",s.seq),"organizer":{"self":true,"email":PRIMARY}});
                    for (k, x) in body.as_object().unwrap() {
                        v[k] = x.clone();
                    }
                    s.events.insert(id, v.clone());
                    if s.drop_ack {
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                    json_response(v)
                }
                "PATCH" => {
                    s.patches += 1;
                    let current = s.events.get(&id).cloned();
                    let Some(mut v) = current else {
                        return StatusCode::NOT_FOUND.into_response();
                    };
                    if s.race_412 || if_match.as_deref() != v["etag"].as_str() {
                        return StatusCode::PRECONDITION_FAILED.into_response();
                    }
                    s.seq += 1;
                    v["etag"] = json!(format!("\"v{}\"", s.seq));
                    for (k, x) in body.as_object().unwrap() {
                        v[k] = x.clone();
                    }
                    s.events.insert(id, v.clone());
                    if s.drop_ack {
                        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
                    }
                    json_response(v)
                }
                _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = MemberCalendar::fixture(format!("http://{}", listener.local_addr().unwrap()));
    (
        client,
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() }),
    )
}

#[test]
fn stable_event_id_is_deterministic_google_legal_and_request_specific() {
    let a = stable_event_id("00000000-0000-4000-8000-000000000001");
    assert_eq!(a, stable_event_id("00000000-0000-4000-8000-000000000001"));
    assert_ne!(a, stable_event_id("00000000-0000-4000-8000-000000000002"));
    assert!((5..=1024).contains(&a.len()));
    assert!(
        a.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'v').contains(&c))
    );
    assert!(event_id(&a).is_ok());
}

#[tokio::test]
async fn create_sends_one_stable_id_without_invitations_and_provider_response_matches() {
    let srv = Arc::new(Mutex::new(Srv::default()));
    let (c, task) = serve(srv.clone()).await;
    let id = stable_event_id(OP);
    let f = fields(
        "팀 회의",
        "2026-11-26T15:00:00+09:00",
        "2026-11-26T16:00:00+09:00",
    );
    let e = c
        .create(
            &token(),
            PRIMARY,
            &id,
            &f,
            OP,
            DIGEST,
            &CancellationToken::new(),
        )
        .await
        .ok()
        .unwrap();
    assert_eq!(e.id, id);
    assert_eq!(e.fields.title, "팀 회의");
    let s = srv.lock().unwrap();
    assert_eq!(s.posts, 1);
    let (_, uri, _) = &s.log[0];
    assert!(uri.contains("sendUpdates=none&sendNotifications=false"));
    assert!(s.events[&id].get("attendees").is_none());
    task.abort();
}

#[tokio::test]
async fn lost_ack_is_reconciled_by_readback_and_never_posts_twice() {
    let srv = Arc::new(Mutex::new(Srv::default()));
    srv.lock().unwrap().drop_ack = true;
    let (c, task) = serve(srv.clone()).await;
    let id = stable_event_id(OP);
    let f = fields(
        "팀 회의",
        "2026-11-26T15:00:00+09:00",
        "2026-11-26T16:00:00+09:00",
    );
    let cancel = CancellationToken::new();
    let err = c
        .create(&token(), PRIMARY, &id, &f, OP, DIGEST, &cancel)
        .await
        .err()
        .unwrap();
    assert!(matches!(
        err,
        MemberWriteError::Write(GmailError::Http(500))
    ));
    srv.lock().unwrap().drop_ack = false;
    match c
        .readback(&token(), PRIMARY, &id, OP, DIGEST, &f, &cancel)
        .await
        .unwrap()
    {
        Readback::Applied(e) => assert_eq!(e.id, id),
        Readback::Different(_) => panic!("marker must match"),
    }
    // A different request's marker must not be claimed as ours.
    let other = "00000000-0000-4000-8000-0000000000bb";
    assert!(matches!(
        c.readback(&token(), PRIMARY, &id, other, DIGEST, &f, &cancel)
            .await
            .unwrap(),
        Readback::Different(_)
    ));
    assert_eq!(srv.lock().unwrap().posts, 1);
    // Absent event: readback reports the provider 404; it never writes.
    assert!(matches!(
        c.readback(&token(), PRIMARY, "v0v0v0v0v0", OP, DIGEST, &f, &cancel)
            .await,
        Err(GmailError::Http(404))
    ));
    assert_eq!(srv.lock().unwrap().posts, 1);
    task.abort();
}

#[tokio::test]
async fn move_uses_if_match_and_preserves_unmodified_fields() {
    let srv = Arc::new(Mutex::new(Srv::default()));
    seed(
        &srv,
        "evt1aaaaaa",
        "팀 회의",
        "2026-11-26T10:00:00+09:00",
        "2026-11-26T11:00:00+09:00",
    );
    let (c, task) = serve(srv.clone()).await;
    let cancel = CancellationToken::new();
    let before = c
        .read(&token(), PRIMARY, "evt1aaaaaa", &cancel)
        .await
        .unwrap();
    let f = fields(
        "팀 회의",
        "2026-11-27T15:00:00+09:00",
        "2026-11-27T16:00:00+09:00",
    );
    let moved = c
        .move_event(
            &token(),
            PRIMARY,
            "evt1aaaaaa",
            &before.etag,
            &f,
            OP,
            DIGEST,
            &cancel,
        )
        .await
        .ok()
        .unwrap();
    assert_eq!(moved.fields.start, "2026-11-27T15:00:00+09:00");
    assert_ne!(moved.etag, before.etag);
    let s = srv.lock().unwrap();
    assert_eq!(s.patches, 1);
    let patch = s.log.iter().find(|(m, _, _)| m == "PATCH").unwrap();
    assert_eq!(patch.2.as_deref(), Some(before.etag.as_str()));
    task.abort();
}

#[tokio::test]
async fn stale_etag_is_definite_before_and_at_the_provider_and_sends_no_second_write() {
    let srv = Arc::new(Mutex::new(Srv::default()));
    seed(
        &srv,
        "evt1aaaaaa",
        "팀 회의",
        "2026-11-26T10:00:00+09:00",
        "2026-11-26T11:00:00+09:00",
    );
    let (c, task) = serve(srv.clone()).await;
    let cancel = CancellationToken::new();
    let f = fields(
        "팀 회의",
        "2026-11-27T15:00:00+09:00",
        "2026-11-27T16:00:00+09:00",
    );
    // 1. Concurrent edit between listing and apply: caught by the fresh GET compare.
    let r = c
        .move_event(
            &token(),
            PRIMARY,
            "evt1aaaaaa",
            "\"older\"",
            &f,
            OP,
            DIGEST,
            &cancel,
        )
        .await;
    assert!(matches!(r, Err(MemberWriteError::Stale(_))));
    assert_eq!(srv.lock().unwrap().patches, 0);
    // 2. Race after the GET: the provider's 412 is a definite no-write too.
    let current = c
        .read(&token(), PRIMARY, "evt1aaaaaa", &cancel)
        .await
        .unwrap();
    srv.lock().unwrap().race_412 = true;
    let raced = c
        .move_event(
            &token(),
            PRIMARY,
            "evt1aaaaaa",
            &current.etag,
            &f,
            OP,
            DIGEST,
            &cancel,
        )
        .await;
    assert!(matches!(raced, Err(MemberWriteError::Stale(_))));
    {
        let s = srv.lock().unwrap();
        assert_eq!(s.patches, 1, "exactly one PATCH reached the provider");
        assert_eq!(s.events["evt1aaaaaa"]["summary"], "팀 회의");
        assert_eq!(
            s.events["evt1aaaaaa"]["start"]["dateTime"],
            "2026-11-26T10:00:00+09:00"
        );
    }
    task.abort();
}

#[tokio::test]
async fn all_day_and_untimed_shapes_are_refused_before_any_request() {
    let srv = Arc::new(Mutex::new(Srv::default()));
    let (c, task) = serve(srv.clone()).await;
    let cancel = CancellationToken::new();
    let mut all_day = fields("종일", "2026-11-26", "2026-11-27");
    all_day.time_zone = None;
    assert!(matches!(
        c.create(
            &token(),
            PRIMARY,
            "abcde12345",
            &all_day,
            OP,
            DIGEST,
            &cancel
        )
        .await,
        Err(MemberWriteError::BeforeWrite(GmailError::InvalidInput))
    ));
    let mut no_zone = fields(
        "a",
        "2026-11-26T15:00:00+09:00",
        "2026-11-26T16:00:00+09:00",
    );
    no_zone.time_zone = None;
    assert!(matches!(
        c.create(
            &token(),
            PRIMARY,
            "abcde12345",
            &no_zone,
            OP,
            DIGEST,
            &cancel
        )
        .await,
        Err(MemberWriteError::BeforeWrite(GmailError::InvalidInput))
    ));
    assert!(srv.lock().unwrap().log.is_empty());
    task.abort();
}
