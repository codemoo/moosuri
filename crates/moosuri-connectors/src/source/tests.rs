use super::*;
use super::{gmail::*, slack::*, transport::*};
use crate::gmail::SecretString;
use axum::{
    Router,
    extract::Request,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::any,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

struct Reply {
    status: u16,
    value: Value,
    retry: Option<&'static str>,
}
fn ok(value: Value) -> Reply {
    Reply {
        status: 200,
        value,
        retry: None,
    }
}
async fn fixture(
    replies: Vec<Reply>,
) -> (
    Transport,
    Arc<Mutex<Vec<String>>>,
    tokio::task::JoinHandle<()>,
) {
    let queue = Arc::new(Mutex::new(VecDeque::from(replies)));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = requests.clone();
    let router = Router::new().fallback(any(move |request: Request| {
        let queue = queue.clone();
        let seen = seen.clone();
        async move {
            assert_eq!(
                request.headers().get(header::AUTHORIZATION).unwrap(),
                "Bearer synthetic-private-token"
            );
            if request.uri().path() == "/auth.test" {
                assert_eq!(request.method(), "POST");
            } else {
                assert_eq!(request.method(), "GET");
            }
            seen.lock().unwrap().push(request.uri().to_string());
            let reply = queue
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected fixture request");
            let mut response = (
                StatusCode::from_u16(reply.status).unwrap(),
                [(header::CONTENT_TYPE, "application/json")],
                reply.value.to_string(),
            )
                .into_response();
            if let Some(retry) = reply.retry {
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, retry.parse().unwrap());
            }
            response
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let transport = Transport::fixture(
        format!("http://{}", listener.local_addr().unwrap()),
        1024 * 1024,
    );
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (transport, requests, task)
}
fn token() -> SecretString {
    SecretString::new("synthetic-private-token".into()).unwrap()
}
fn profile() -> Reply {
    ok(json!({"emailAddress":"owner@example.test","historyId":"100"}))
}
fn gmail_scope() -> GmailScope {
    GmailScope {
        account_email: "owner@example.test".into(),
        label_ids: vec!["INBOX".into()],
        after_unix: 100,
    }
}
fn slack_scope() -> SlackScope {
    SlackScope {
        workspace_id: "T1".into(),
        user_id: "U1".into(),
        channel_id: "C1".into(),
        oldest_unix: 100,
        reconciliation_lookback_seconds: 200,
    }
}
fn identity() -> Reply {
    ok(json!({"ok":true,"team_id":"T1","user_id":"U1"}))
}

#[tokio::test]
async fn gmail_backfill_mapping_history_tokens_nochange_and_deletes() {
    let raw = URL_SAFE_NO_PAD.encode(b"From: Sender <sender@example.test>\r\nTo: owner@example.test\r\nSubject: Synthetic mail\r\nContent-Type: text/plain; charset=utf-8\r\n\r\nprivate body\r\n");
    let (transport, requests, task) = fixture(vec![profile(),
        ok(json!({"messages":[{"id":"abc"}]})),
        ok(json!({"id":"abc","threadId":"thread1","historyId":"101","internalDate":"150000","labelIds":["INBOX"],"raw":raw})),
        profile(),ok(json!({"historyId":"900","nextPageToken":"opaque/+=","history":[{"id":"110","messagesDeleted":[{"message":{"id":"abc"}}]}]})),
        profile(),ok(json!({"historyId":"901"})),
    ]).await;
    let client = GmailSyncClient::fixture(transport);
    let cancel = CancellationToken::new();
    let first = client
        .next_page(&token(), &gmail_scope(), None, 200, &cancel)
        .await
        .unwrap();
    assert_eq!(first.checkpoint.history_id, "100");
    assert!(!first.checkpoint.backfill);
    match &first.changes[0] {
        SourceChange::Upsert(item) => {
            assert_eq!(item.subject, "Synthetic mail");
            assert_eq!(item.observed_at, 150);
            assert_eq!(item.external_id, "abc");
            assert!(item.text.contains("private body"));
        }
        _ => panic!(),
    }
    let second = client
        .next_page(
            &token(),
            &gmail_scope(),
            Some(&first.checkpoint),
            200,
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(second.checkpoint.history_id, "100");
    assert_eq!(second.checkpoint.page_token.as_deref(), Some("opaque/+="));
    assert!(
        matches!(&second.changes[0],SourceChange::Delete { external_id,.. } if external_id=="abc")
    );
    let third = client
        .next_page(
            &token(),
            &gmail_scope(),
            Some(&second.checkpoint),
            200,
            &cancel,
        )
        .await
        .unwrap();
    assert!(third.is_no_change());
    assert_eq!(third.coverage, Coverage::GmailHistoryComplete);
    assert_eq!(third.checkpoint.history_id, "901");
    let seen = requests.lock().unwrap();
    assert!(seen[1].contains("q=after%3A100"));
    assert!(seen[1].contains("labelIds=INBOX"));
    assert!(seen.last().unwrap().contains("startHistoryId=100"));
    assert!(seen.last().unwrap().contains("pageToken=opaque%2F%2B%3D"));
    task.abort();
}
#[tokio::test]
async fn gmail_expired_history_and_identity_fail_without_progress() {
    let (transport, _, task) = fixture(vec![
        profile(),
        Reply {
            status: 404,
            value: json!({"error":"private payload"}),
            retry: None,
        },
    ])
    .await;
    let client = GmailSyncClient::fixture(transport);
    let cp = GmailCheckpoint {
        history_id: "1".into(),
        page_token: None,
        backfill: false,
    };
    assert_eq!(
        client
            .next_page(
                &token(),
                &gmail_scope(),
                Some(&cp),
                200,
                &CancellationToken::new()
            )
            .await
            .err(),
        Some(ReadError::ResyncRequired)
    );
    task.abort();
    let (transport, requests, task) = fixture(vec![ok(
        json!({"emailAddress":"other@example.test","historyId":"100"}),
    )])
    .await;
    assert_eq!(
        GmailSyncClient::fixture(transport)
            .next_page(
                &token(),
                &gmail_scope(),
                None,
                200,
                &CancellationToken::new()
            )
            .await
            .err(),
        Some(ReadError::IdentityMismatch)
    );
    assert_eq!(requests.lock().unwrap().len(), 1);
    task.abort();
}
#[tokio::test]
async fn slack_late_replies_require_reconciliation_and_reply_pagination() {
    let (transport,requests,task)=fixture(vec![identity(),ok(json!({"ok":true,"messages":[]})),
        identity(),ok(json!({"ok":true,"messages":[{"ts":"150.000001","user":"U2","text":"parent","reply_count":2}]})),
        identity(),ok(json!({"ok":true,"messages":[{"ts":"150.000001","user":"U2","text":"parent"},{"ts":"250.000002","thread_ts":"150.000001","user":"U2","text":"late reply"}],"has_more":true,"response_metadata":{"next_cursor":"opaque-cursor"}})),
        identity(),ok(json!({"ok":true,"messages":[{"ts":"260.000003","thread_ts":"150.000001","user":"U2","text":"edited reply","edited":{"ts":"270.000001"}}]})),
    ]).await;
    let client = SlackClient::fixture(transport);
    let cancel = CancellationToken::new();
    let scope = slack_scope();
    let cp = SlackCheckpoint {
        high_water_unix: 200,
        phase: SlackPhase::Idle,
    };
    let first = client
        .next_page(&token(), &scope, Some(&cp), 300, &cancel)
        .await
        .unwrap();
    assert!(first.is_no_change());
    assert_eq!(first.coverage, Coverage::MorePages);
    let second = client
        .next_page(&token(), &scope, Some(&first.checkpoint), 300, &cancel)
        .await
        .unwrap();
    let third = client
        .next_page(&token(), &scope, Some(&second.checkpoint), 300, &cancel)
        .await
        .unwrap();
    assert!(third.changes.iter().any(|c| matches!(c,SourceChange::Upsert(i) if i.text=="late reply" && i.parent_id.as_deref()==Some("C1:150.000001"))));
    assert_eq!(third.checkpoint.high_water_unix, 200);
    let final_page = client
        .next_page(&token(), &scope, Some(&third.checkpoint), 300, &cancel)
        .await
        .unwrap();
    assert_eq!(final_page.coverage, Coverage::SlackPollingWindow);
    assert_eq!(final_page.checkpoint.high_water_unix, 300);
    assert!(
        matches!(&final_page.changes[0],SourceChange::Upsert(i) if i.remote_version=="270.000001")
    );
    let seen = requests.lock().unwrap();
    assert!(seen[1].contains("oldest=199"));
    assert!(seen[3].contains("oldest=100"));
    assert!(seen[7].contains("cursor=opaque-cursor"));
    task.abort();
}
#[tokio::test]
async fn typed_rate_limit_auth_cursor_errors_and_bounded_safe_errors() {
    let (transport, _, task) = fixture(vec![Reply {
        status: 429,
        value: json!({"private":"synthetic-private-token"}),
        retry: Some("120"),
    }])
    .await;
    let err = transport
        .request(
            Endpoint::Slack,
            "auth.test",
            Some(&token()),
            &CancellationToken::new(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(
        err,
        ReadError::RateLimited {
            retry_after_seconds: 120
        }
    );
    assert!(!err.to_string().contains("synthetic-private-token"));
    task.abort();
    for (code, expected) in [
        ("token_revoked", ReadError::AuthenticationRequired),
        ("invalid_cursor", ReadError::ResyncRequired),
        ("missing_scope", ReadError::ScopeDenied),
    ] {
        let (transport, _, task) = fixture(vec![ok(json!({"ok":false,"error":code}))]).await;
        assert_eq!(
            SlackClient::fixture(transport)
                .identity(&token(), &CancellationToken::new())
                .await
                .err(),
            Some(expected)
        );
        task.abort();
    }
    let (transport, _, task) = fixture(vec![ok(
        json!({"ok":true,"team_id":"T1","user_id":"U1","bot_id":"B1"}),
    )])
    .await;
    assert_eq!(
        SlackClient::fixture(transport)
            .identity(&token(), &CancellationToken::new())
            .await
            .err(),
        Some(ReadError::ScopeDenied)
    );
    task.abort();
}
#[tokio::test]
async fn cancellation_limits_redirects_and_repeated_cursor_cannot_advance() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        Transport::fixed()
            .request(Endpoint::Slack, "auth.test", Some(&token()), &cancel)
            .await
            .err(),
        Some(ReadError::Cancelled)
    );
    let (mut transport, _, task) =
        fixture(vec![ok(json!({"ok":true,"large":"x".repeat(500)}))]).await;
    transport.test_limit = Some(32);
    assert_eq!(
        transport
            .request(
                Endpoint::Slack,
                "auth.test",
                Some(&token()),
                &CancellationToken::new()
            )
            .await
            .err(),
        Some(ReadError::LimitExceeded)
    );
    task.abort();
    let (transport, _, task) = fixture(vec![Reply {
        status: 302,
        value: json!({}),
        retry: None,
    }])
    .await;
    assert_eq!(
        transport
            .request(
                Endpoint::Slack,
                "auth.test",
                Some(&token()),
                &CancellationToken::new()
            )
            .await
            .err(),
        Some(ReadError::Http(302))
    );
    task.abort();
    let (transport,_,task)=fixture(vec![identity(),ok(json!({"ok":true,"messages":[],"has_more":true,"response_metadata":{"next_cursor":"same"}}))]).await;
    let cp = SlackCheckpoint {
        high_water_unix: 200,
        phase: SlackPhase::History {
            window: SlackWindow {
                oldest_unix: 100,
                latest_unix: 300,
                cursor: Some("same".into()),
                reconciliation: true,
            },
        },
    };
    assert_eq!(
        SlackClient::fixture(transport)
            .next_page(
                &token(),
                &slack_scope(),
                Some(&cp),
                300,
                &CancellationToken::new()
            )
            .await
            .err(),
        Some(ReadError::InvalidResponse)
    );
    task.abort();
}
#[test]
fn fixed_origins_scope_validation_and_aggregate_bound() {
    let transport = Transport::fixed();
    assert_eq!(
        transport
            .target(&Endpoint::Gmail, "history")
            .unwrap()
            .origin()
            .ascii_serialization(),
        "https://gmail.googleapis.com"
    );
    assert_eq!(
        transport
            .target(&Endpoint::Slack, "auth.test")
            .unwrap()
            .origin()
            .ascii_serialization(),
        "https://slack.com"
    );
    for path in ["/evil", "https://evil.test", "../evil"] {
        assert!(transport.target(&Endpoint::Slack, path).is_err());
    }
    let mut scope = gmail_scope();
    scope.label_ids = vec!["INBOX OR any".into()];
    assert!(scope.validate(200).is_err());
    scope.label_ids.clear();
    assert!(scope.validate(200).is_err());
    let page = SyncPage {
        changes: (0..33)
            .map(|i| SourceChange::Delete {
                external_id: i.to_string(),
                remote_version: "v".into(),
            })
            .collect(),
        checkpoint: (),
        coverage: Coverage::MorePages,
    };
    assert_eq!(bounded(page).err(), Some(ReadError::LimitExceeded));
}
#[tokio::test]
async fn cancellation_interrupts_inflight_body_and_google_quota_is_typed() {
    let started = Arc::new(tokio::sync::Notify::new());
    let notify = started.clone();
    let router = Router::new().fallback(any(move || {
        let notify = notify.clone();
        async move {
            notify.notify_one();
            let stream = futures_util::stream::once(async {
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                Ok::<_, std::io::Error>("{}")
            });
            (
                [(header::CONTENT_TYPE, "application/json")],
                axum::body::Body::from_stream(stream),
            )
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let transport = Transport::fixture(format!("http://{}", listener.local_addr().unwrap()), 1024);
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let cancel = CancellationToken::new();
    let child = cancel.clone();
    let request = tokio::spawn(async move {
        transport
            .request(Endpoint::Slack, "auth.test", Some(&token()), &child)
            .await
    });
    started.notified().await;
    cancel.cancel();
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(1), request)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(outcome.err(), Some(ReadError::Cancelled));
    task.abort();
    let (transport,_,task)=fixture(vec![Reply {status:403,value:json!({"error":{"message":"private-canary","errors":[{"reason":"userRateLimitExceeded"}]}}),retry:None}]).await;
    assert_eq!(
        transport
            .request(
                Endpoint::Gmail,
                "history",
                Some(&token()),
                &CancellationToken::new()
            )
            .await
            .err(),
        Some(ReadError::RateLimited {
            retry_after_seconds: 60
        })
    );
    task.abort();
}
#[tokio::test]
async fn gmail_label_departure_is_explicit_and_omission_is_never_a_delete() {
    let (transport,_,task)=fixture(vec![profile(),ok(json!({"historyId":"120","history":[{"id":"110","labelsRemoved":[{"message":{"id":"abc"}}]}]})),
        ok(json!({"id":"abc","threadId":"thread1","historyId":"110","internalDate":"150000","labelIds":[],"raw":""}))]).await;
    let cp = GmailCheckpoint {
        history_id: "100".into(),
        page_token: None,
        backfill: false,
    };
    let result = GmailSyncClient::fixture(transport)
        .next_page(
            &token(),
            &gmail_scope(),
            Some(&cp),
            200,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(result.changes.len(), 1);
    assert!(
        matches!(&result.changes[0],SourceChange::Delete {external_id,..} if external_id=="abc")
    );
    task.abort();
}

#[tokio::test]
async fn gmail_backfill_includes_accepted_spam_trash_and_custom_label_scopes() {
    let raw=URL_SAFE_NO_PAD.encode(b"From: sender@example.test\r\nTo: owner@example.test\r\nSubject: Scoped mail\r\nContent-Type: text/plain\r\n\r\nSynthetic scoped body");
    for label in ["SPAM", "TRASH", "Label_custom"] {
        let (transport,requests,task)=fixture(vec![profile(),ok(json!({"messages":[{"id":"abc"}]})),
            ok(json!({"id":"abc","threadId":"thread1","historyId":"101","internalDate":"150000","labelIds":[label,"SPAM"],"raw":raw})),
            profile(),ok(json!({"historyId":"101"}))]).await;
        let client = GmailSyncClient::fixture(transport);
        let scope = GmailScope {
            label_ids: vec![label.into()],
            ..gmail_scope()
        };
        let page = client
            .next_page(&token(), &scope, None, 200, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(page.changes.len(), 1);
        assert!(matches!(&page.changes[0],SourceChange::Upsert(item) if item.external_id=="abc"));
        let end = client
            .next_page(
                &token(),
                &scope,
                Some(&page.checkpoint),
                200,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(end.coverage, Coverage::GmailHistoryComplete);
        let seen = requests.lock().unwrap();
        assert!(seen[1].contains("includeSpamTrash=true"));
        assert!(seen[1].contains(&format!("labelIds={label}")));
        task.abort();
    }
}
