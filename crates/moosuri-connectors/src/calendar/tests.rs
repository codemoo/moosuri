use super::*;
use axum::{
    Router, body::to_bytes, extract::Request, http::StatusCode, response::IntoResponse,
    routing::any,
};
use std::sync::{Arc, Mutex};
fn secret() -> SecretString {
    SecretString::new("synthetic-calendar-token".into()).unwrap()
}
fn fields() -> CalendarFields {
    CalendarFields {
        title: "Synthetic Calendar event".into(),
        description: "Synthetic only".into(),
        location: "Test room".into(),
        start: "2026-11-15T10:00:00+09:00".into(),
        end: "2026-11-15T11:00:00+09:00".into(),
        time_zone: Some("Asia/Seoul".into()),
    }
}
fn event(id: &str) -> Value {
    json!({"id":id,"etag":"\"synthetic-etag\"","status":"confirmed","summary":"Synthetic Calendar event","description":"Synthetic only","location":"Test room","start":{"dateTime":"2026-11-15T10:00:00+09:00","timeZone":"Asia/Seoul"},"end":{"dateTime":"2026-11-15T11:00:00+09:00","timeZone":"Asia/Seoul"},"organizer":{"self":true,"email":"calendar-owner@example.test"}})
}
fn response(v: Value) -> axum::response::Response {
    (
        [(reqwest::header::CONTENT_TYPE.as_str(), "application/json")],
        v.to_string(),
    )
        .into_response()
}
#[test]
fn scope_is_explicit_and_never_includes_gmail() {
    let u = crate::gmail::GmailClient::authorization_url_scoped(
        "synthetic.apps.googleusercontent.com",
        "https://assistant.example.test/api/v1/mail/oauth/callback",
        &format!("cal.{}", "a".repeat(64)),
        &"b".repeat(64),
        &SCOPES,
    )
    .unwrap();
    let u = url::Url::parse(&u).unwrap();
    let q: std::collections::HashMap<_, _> = u.query_pairs().collect();
    assert_eq!(q["include_granted_scopes"], "false");
    assert_eq!(q["scope"], SCOPES.join(" "));
    assert!(!q["scope"].contains("gmail"));
}
#[test]
fn owner_scope_attendees_recurrence_special_types_and_range_are_closed() {
    let v = event("synthetic1");
    assert!(
        observation(&v, "calendar-owner@example.test")
            .unwrap()
            .event
            .writable
    );
    for update in [
        json!({"attendees":[{"email":"other@example.test"}]}),
        json!({"attendeesOmitted":true}),
        json!({"recurrence":["RRULE:FREQ=DAILY"]}),
        json!({"recurringEventId":"parent"}),
        json!({"eventType":"outOfOffice"}),
        json!({"organizer":{"self":false,"email":"other@example.test"}}),
    ] {
        let mut modified = v.clone();
        for (k, v) in update.as_object().unwrap() {
            modified[k] = v.clone()
        }
        assert!(
            !observation(&modified, "calendar-owner@example.test")
                .unwrap()
                .event
                .writable
        )
    }
    assert!(validate_range("2026-11-01T00:00:00Z", "2026-12-03T00:00:00Z").is_err());
    assert!(validate_range("2026-11-01", "2026-11-02").is_err());
    assert!(event_id("../../other").is_err());
}
#[tokio::test]
async fn actual_fixed_transport_emits_no_invitations_precise_etag_and_delete204() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = seen.clone();
    let app=Router::new().fallback(any(move|request:Request|{let captured=captured.clone();async move{let method=request.method().to_string();let uri=request.uri().to_string();let headers=request.headers().clone();let body=to_bytes(request.into_body(),32768).await.unwrap();let body=if body.is_empty(){Value::Null}else{serde_json::from_slice(&body).unwrap()};captured.lock().unwrap().push((method.clone(),uri.clone(),headers.clone(),body.clone()));assert_eq!(headers["authorization"],"Bearer synthetic-calendar-token");if method=="DELETE"{return StatusCode::NO_CONTENT.into_response()}
if uri.starts_with("/users/me/calendarList/primary"){return response(json!({"id":"calendar-owner@example.test","summary":"Primary","primary":true,"accessRole":"owner","timeZone":"Asia/Seoul","notificationSettings":{"notifications":[]}}))}
if uri.starts_with("/calendars/primary/events?")&&method=="GET"{return response(json!({"items":[event("synthetic1")],"nextPageToken":"synthetic-next-page"}))}
if method=="PATCH"||method=="POST"{let id=body.get("id").and_then(|v|v.as_str()).unwrap_or("synthetic1");let mut v=event(id);for(k,value)in body.as_object().unwrap(){v[k]=value.clone()}return response(v)}response(event("synthetic1"))}}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = CalendarClient::fixture(format!("http://{}", listener.local_addr().unwrap()));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let cancel = CancellationToken::new();
    let p = client.primary(&secret(), &cancel).await.unwrap();
    assert!(p.primary);
    let page = client
        .events(
            &secret(),
            &p.id,
            &CalendarRange {
                connection_id: "synthetic".into(),
                start: "2026-11-15T00:00:00+09:00".into(),
                end: "2026-11-16T00:00:00+09:00".into(),
                page_token: None,
            },
            &cancel,
        )
        .await
        .unwrap();
    assert!(page.partial);
    let op = "00000000-0000-4000-8000-000000000001";
    let digest = "a".repeat(64);
    let create = client
        .mutate(
            &secret(),
            CalendarWrite {
                primary: &p.id,
                id: "m00000000000040008000000000000001",
                change: &CalendarMutation::Create { fields: fields() },
                operation: op,
                digest: &digest,
                prior: None,
            },
            &cancel,
        )
        .await
        .unwrap()
        .event
        .unwrap();
    assert_eq!(create.operation.as_deref(), Some(op));
    let prior = client
        .event(&secret(), &p.id, "synthetic1", &cancel)
        .await
        .unwrap();
    client
        .mutate(
            &secret(),
            CalendarWrite {
                primary: &p.id,
                id: "synthetic1",
                change: &CalendarMutation::Update {
                    event_id: "synthetic1".into(),
                    etag: prior.event.etag.clone(),
                    fields: fields(),
                },
                operation: op,
                digest: &digest,
                prior: Some(&prior),
            },
            &cancel,
        )
        .await
        .unwrap();
    let deleted = client
        .mutate(
            &secret(),
            CalendarWrite {
                primary: &p.id,
                id: "synthetic1",
                change: &CalendarMutation::Delete {
                    event_id: "synthetic1".into(),
                    etag: prior.event.etag.clone(),
                },
                operation: op,
                digest: &digest,
                prior: Some(&prior),
            },
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(deleted.status, 204);
    assert!(deleted.event.is_none());
    let seen = seen.lock().unwrap();
    for (method, path, h, body) in seen.iter().filter(|(m, _, _, _)| m != "GET") {
        assert!(path.contains("sendUpdates=none&sendNotifications=false"));
        assert!(body.get("attendees").is_none());
        if method == "PATCH" || method == "DELETE" {
            assert_eq!(h["if-match"], "\"synthetic-etag\"")
        }
        if method != "DELETE" {
            assert_eq!(body["reminders"]["useDefault"], false);
            assert_eq!(body["reminders"]["overrides"], json!([]));
        }
    }
    task.abort();
}
#[tokio::test]
async fn stale_etag_is_definite_and_transport_never_retries_write() {
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let captured = count.clone();
    let app = Router::new().fallback(any(move || {
        let captured = captured.clone();
        async move {
            captured.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            StatusCode::PRECONDITION_FAILED
        }
    }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let c = CalendarClient::fixture(format!("http://{}", l.local_addr().unwrap()));
    let t = tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    let prior = observation(&event("synthetic1"), "calendar-owner@example.test").unwrap();
    let e = c
        .mutate(
            &secret(),
            CalendarWrite {
                primary: "calendar-owner@example.test",
                id: "synthetic1",
                change: &CalendarMutation::Delete {
                    event_id: "synthetic1".into(),
                    etag: prior.event.etag.clone(),
                },
                operation: "00000000-0000-4000-8000-000000000001",
                digest: &"a".repeat(64),
                prior: Some(&prior),
            },
            &CancellationToken::new(),
        )
        .await;
    assert!(matches!(e, Err(GmailError::Http(412))));
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    t.abort();
}
#[test]
fn exact_lookup_requires_own_id_digest_fields_and_supported_event_shape() {
    let op = "00000000-0000-4000-8000-000000000001";
    let digest = "a".repeat(64);
    let mut v = event("synthetic1");
    v["extendedProperties"] = json!({"private":{"moosuri_operation":op,"moosuri_digest":digest}});
    let mut observed = observation(&v, "calendar-owner@example.test").unwrap();
    let change = CalendarMutation::Create { fields: fields() };
    assert!(observed_change(
        &observed,
        "synthetic1",
        op,
        &digest,
        &change
    ));
    assert!(!observed_change(
        &observed,
        "foreign-id",
        op,
        &digest,
        &change
    ));
    assert!(!observed_change(
        &observed,
        "synthetic1",
        "foreign-op",
        &digest,
        &change
    ));
    assert!(!observed_change(
        &observed,
        "synthetic1",
        op,
        &"b".repeat(64),
        &change
    ));
    observed.event.fields.title = "Peer changed title".into();
    assert!(!observed_change(
        &observed,
        "synthetic1",
        op,
        &digest,
        &change
    ));
}

#[tokio::test]
async fn actual_calendar_chat_page_limit_is_five_and_never_silently_drops_provider_rows() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let app=Router::new().fallback(any(move|request:Request|{let recorded=recorded.clone();async move{assert_eq!(request.method(),"GET");let url=url::Url::parse(&format!("http://fixture{}",request.uri())).unwrap();let q=url.query_pairs().collect::<std::collections::HashMap<_,_>>();assert_eq!(q["maxResults"],"5");assert_eq!(q["showDeleted"],"false");assert_eq!(q["singleEvents"],"true");let token=q.get("pageToken").map(|v|v.to_string());recorded.lock().unwrap().push(token.clone());let count=if token.is_some(){6}else{5};response(json!({"items":(0..count).map(|i|event(&format!("synthetic{i}"))).collect::<Vec<_>>(),"nextPageToken":"actual-page-two"}))}}));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = CalendarClient::fixture(format!("http://{}", listener.local_addr().unwrap()));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let cancel = CancellationToken::new();
    let mut range = CalendarRange {
        connection_id: "synthetic".into(),
        start: "2026-11-15T00:00:00+09:00".into(),
        end: "2026-11-16T00:00:00+09:00".into(),
        page_token: None,
    };
    let page = client
        .events_page(&secret(), "calendar-owner@example.test", &range, 5, &cancel)
        .await
        .unwrap();
    assert_eq!(page.events.len(), 5);
    assert!(page.partial);
    range.page_token = page.next_page_token;
    assert!(matches!(
        client
            .events_page(&secret(), "calendar-owner@example.test", &range, 5, &cancel)
            .await,
        Err(GmailError::LimitExceeded)
    ));
    assert_eq!(
        *seen.lock().unwrap(),
        vec![None, Some("actual-page-two".into())]
    );
    task.abort();
}
