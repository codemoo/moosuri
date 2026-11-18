use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
use tower::ServiceExt;

fn bridge(script: &str) -> Arc<Bridge> {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("gws");
    fs::write(&executable, script).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    Arc::new(Bridge {
        executable,
        mailbox: "owner@example.invalid".into(),
        key_digest: Sha256::digest("a".repeat(64).as_bytes()).into(),
        gate: Arc::new(Semaphore::new(1)),
        directory,
    })
}
fn request(body: &str) -> Request {
    Request::builder()
        .method("POST")
        .uri("/v1/inbox/latest")
        .header("authorization", format!("Bearer {}", "a".repeat(64)))
        .body(axum::body::Body::from(body.to_owned()))
        .unwrap()
}
#[tokio::test]
async fn fixed_mailbox_metadata_only_commands_and_sorted_literal_projection() {
    // Independent fake CLI fails if a mailbox, selector, operation or format flag
    // changes, including accidental use of `me`, raw/full bodies, or writes.
    let bridge = bridge(
        r##"#!/usr/bin/env python3
import sys,json
assert sys.argv[-2:]==['--format','json']
p=json.loads(sys.argv[-3])
op=sys.argv[1:-4]
if op==['gmail','users','getProfile']:
 assert p=={'userId':'me','fields':'emailAddress'}
 print(json.dumps({'emailAddress':'owner@example.invalid'}))
elif op==['gmail','users','messages','list']:
 assert p=={'userId':'owner@example.invalid','labelIds':['INBOX'],'maxResults':20,'includeSpamTrash':False,'fields':'messages(id)'}
 print(json.dumps({'messages':[{'id':'a1'},{'id':'b2'}]}))
elif op==['gmail','users','messages','get']:
 assert p=={'userId':'owner@example.invalid','id':p['id'],'format':'metadata','metadataHeaders':['From','Subject','Date'],'fields':'id,labelIds,internalDate,payload(headers)'}
 print(json.dumps({'id':p['id'],'labelIds':['INBOX'],'internalDate':'200' if p['id']=='b2' else '100','payload':{'headers':[{'name':'Subject','value':'literal <tag>\r\n\u202efinish'},{'name':'From','value':'Example <a@example.invalid>'},{'name':'Date','value':'synthetic'}]},'snippet':'SECRET-NOT-PROJECTED','token':'SECRET-NOT-PROJECTED'}))
else: raise Exception('unexpected command')
"##,
    );
    let response = router(bridge).oneshot(request("{}")).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let bytes = to_bytes(response.into_body(), OUTPUT_LIMIT).await.unwrap();
    let result: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        result,
        json!({"mailbox":"owner@example.invalid", "messages":[
        {"id":"b2","from":"Example <a@example.invalid>","subject":"literal <tag>finish","date":"synthetic","internal_date":200},
        {"id":"a1","from":"Example <a@example.invalid>","subject":"literal <tag>finish","date":"synthetic","internal_date":100}]})
    );
}
#[tokio::test]
async fn auth_browser_arguments_concurrency_and_mismatch_fail_closed() {
    let bridge = bridge("#!/bin/sh\nprintf '%s' '{\"emailAddress\":\"other@example.invalid\"}'\n");
    let app = router(bridge.clone());
    let mut no_auth = request("{}");
    no_auth.headers_mut().remove(header::AUTHORIZATION);
    assert_eq!(
        app.clone().oneshot(no_auth).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    for header in [header::ORIGIN, header::COOKIE] {
        let mut req = request("{}");
        req.headers_mut()
            .insert(header, "synthetic".parse().unwrap());
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    for body in [
        "{\"mailbox\":\"other@example.invalid\"}",
        "[]",
        "null",
        "",
        "{\"args\":[\"send\"]}",
    ] {
        assert_eq!(
            app.clone().oneshot(request(body)).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut req = request("{}");
    *req.uri_mut() = "/v1/inbox/latest?userId=other".parse().unwrap();
    assert_eq!(
        app.clone().oneshot(req).await.unwrap().status(),
        StatusCode::NOT_FOUND
    );
    let permit = bridge.gate.acquire().await.unwrap();
    assert_eq!(
        app.clone().oneshot(request("{}")).await.unwrap().status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    drop(permit);
    let response = app.oneshot(request("{}")).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert!(
        to_bytes(response.into_body(), 1000)
            .await
            .unwrap()
            .is_empty()
    );
}
#[tokio::test]
async fn oversized_provider_output_and_missing_inbox_are_rejected() {
    let bridge = bridge("#!/usr/bin/env python3\nprint('x'*131073)\n");
    assert!(bridge.command(&["--version"], None).await.is_err());
    let value =
        json!({"id":"a1","internalDate":"100","labelIds":["SENT"],"payload":{"headers":[]}});
    assert!(parse_message(&value, "a1").is_err());
}
#[test]
fn shared_key_requires_private_owned_regular_file_without_aliases() {
    let directory = tempfile::tempdir().unwrap();
    let key = directory.path().join("key");
    fs::write(&key, format!("{}\n", "a".repeat(64))).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(read_key(&key).is_ok());
    let link = directory.path().join("link");
    symlink(&key, &link).unwrap();
    assert!(read_key(&link).is_err());
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(read_key(&key).is_err());
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&key, directory.path().join("hardlink")).unwrap();
    assert!(read_key(&key).is_err());
}

#[tokio::test]
async fn profile_probe_is_signed_nonce_bound_and_never_lists_mail() {
    let bridge = bridge(
        r##"#!/usr/bin/env python3
import json,sys
assert sys.argv[1:4]==['gmail','users','getProfile']
assert json.loads(sys.argv[-3])=={'userId':'me','fields':'emailAddress'}
print('{"emailAddress":"owner@example.invalid"}')
"##,
    );
    let nonce = uuid::Uuid::new_v4().to_string();
    let mut req = request(&json!({"nonce":nonce}).to_string());
    *req.uri_mut() = "/v1/profile/check".parse().unwrap();
    let response = router(bridge.clone()).oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let reply: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply["state"], "ready");
    assert_eq!(reply["mailbox"], "owner@example.invalid");
    let signed = serde_json::to_vec(&(
        "moosuri/gws-profile/v1",
        nonce,
        "ready",
        "owner@example.invalid",
    ))
    .unwrap();
    let expected: String = ring::hmac::sign(
        &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &bridge.key_digest),
        &signed,
    )
    .as_ref()
    .iter()
    .map(|b| format!("{b:02x}"))
    .collect();
    assert_eq!(reply["signature"], expected);
    assert!(reply.get("messages").is_none());
}

#[tokio::test]
async fn explicit_login_runs_only_official_fixed_read_only_command() {
    let bridge = bridge(
        r##"#!/usr/bin/env python3
import sys
assert sys.argv[1:]==['auth','login','--readonly','--services','gmail']
"##,
    );
    let mut req = request("{}");
    *req.uri_mut() = "/v1/auth/login".parse().unwrap();
    let response = router(bridge.clone()).oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let reply: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(reply, json!({"state":"login_started"}));
    let permit = tokio::time::timeout(Duration::from_secs(5), bridge.gate.acquire())
        .await
        .unwrap()
        .unwrap();
    drop(permit);
    let mut malicious = request("{\"args\":[\"send\"]}");
    *malicious.uri_mut() = "/v1/auth/login".parse().unwrap();
    assert_eq!(
        router(bridge).oneshot(malicious).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}
