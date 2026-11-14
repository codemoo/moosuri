use super::*;
const PAIR: &str = "/api/v1/credential-broker/pair";
#[cfg(not(feature = "credential-broker-foundation"))]
#[tokio::test]
async fn credential_broker_routes_are_absent_by_default() {
    let _g = TEST_LOCK.lock().await;
    let (_d, _s, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    assert_eq!(
        send(
            &app,
            "POST",
            PAIR,
            Some(&cookie),
            json!({"public_key":vec![0;32]})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}
#[cfg(feature = "credential-broker-foundation")]
async fn device_post(app: &Router, path: &str, body: Value) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header(header::HOST, HOST)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
#[cfg(feature = "credential-broker-foundation")]
#[tokio::test]
async fn credential_broker_http_authentication_and_replay() {
    use moosuri_credential_broker::auth::{SigningKey, authenticated_bytes};
    use moosuri_vault_protocol::broker::*;
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let c = send(
        &app,
        "POST",
        PAIR,
        Some(&cookie),
        json!({"public_key":key.public_key()}),
    )
    .await;
    assert_eq!(c.status(), StatusCode::OK);
    let c: PairChallenge = serde_json::from_value(value(c).await).unwrap();
    let p = PairProof {
        challenge: c.nonce,
        signature: key.sign(&pairing_bytes(&c).unwrap()),
    };
    let body = serde_json::to_value(&p).unwrap();
    // Cookie/Origin authority is forbidden on every device endpoint.
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/credential-broker/device/pair",
            Some(&cookie),
            body.clone()
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let response = device_post(&app, "/api/v1/credential-broker/device/pair", body.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let d: Device = serde_json::from_value(value(response).await).unwrap();
    assert_eq!(
        device_post(&app, "/api/v1/credential-broker/device/pair", body)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let grant = moosuri_credential_broker::policy::LocalGrant::select_fixed_probe(
        d.clone(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        1,
        1,
        9_000_000_000,
    )
    .unwrap();
    let cmd = Command::Register {
        binding: grant.binding.clone(),
    };
    let response = device_post(
        &app,
        "/api/v1/credential-broker/device/challenge",
        json!({"device":d.id}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let challenge: AuthChallenge = serde_json::from_value(value(response).await).unwrap();
    let mut signed = SignedCommand {
        challenge: challenge.nonce,
        signature: key.sign(&authenticated_bytes(&challenge, "GET", DEVICE_PATH, &cmd).unwrap()),
        command: cmd,
    };
    assert_eq!(
        device_post(&app, DEVICE_PATH, serde_json::to_value(&signed).unwrap())
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    signed.signature =
        key.sign(&authenticated_bytes(&challenge, "POST", DEVICE_PATH, &signed.command).unwrap());
    let body = serde_json::to_value(&signed).unwrap();
    assert_eq!(
        device_post(&app, DEVICE_PATH, body.clone()).await.status(),
        StatusCode::OK
    );
    assert_eq!(
        device_post(&app, DEVICE_PATH, body).await.status(),
        StatusCode::UNAUTHORIZED
    );
    #[cfg(feature = "test-fixtures")]
    {
        let user = store
            .authenticate(cookie.split_once('=').unwrap().1.to_string())
            .await
            .unwrap();
        let request = store
            .broker_fixture_probe(
                &user,
                d.clone(),
                grant.binding.clone(),
                grant.probe_digest(),
            )
            .await
            .unwrap();
        let challenge = store.broker_auth_challenge(d.id).await.unwrap();
        let command = Command::Commit {
            invocation: request.invocation.clone(),
        };
        let signed = SignedCommand {
            challenge: challenge.nonce,
            signature: key
                .sign(&authenticated_bytes(&challenge, "POST", DEVICE_PATH, &command).unwrap()),
            command,
        };
        let response = device_post(&app, DEVICE_PATH, serde_json::to_value(signed).unwrap()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let Reply::Commitment { commitment } =
            serde_json::from_value::<Reply>(value(response).await).unwrap()
        else {
            panic!("missing commitment")
        };
        assert!(commitment.request == request);
    }
    let _ = store;
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/v1/credential-broker/{}/revoke", d.id),
            Some(&cookie),
            json!({})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
}
