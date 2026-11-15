use super::*;

#[tokio::test]
async fn thread_creation_request_key_reconciles_and_never_resurrects_deleted_thread() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let request_id = "04731108-a885-4ea6-a550-10ed88239066";
    let body = json!({"title":"  Synthetic conversation  ","request_id":request_id});
    let (a, b) = tokio::join!(
        send(&app, "POST", "/api/v1/threads", Some(&cookie), body.clone()),
        send(&app, "POST", "/api/v1/threads", Some(&cookie), body.clone())
    );
    assert_eq!(a.status(), StatusCode::CREATED);
    assert_eq!(b.status(), StatusCode::CREATED);
    let a = value(a).await;
    let b = value(b).await;
    assert_eq!(a, b);
    assert_eq!(a["id"], request_id);
    assert_eq!(a["title"], "Synthetic conversation");
    let user = store
        .authenticate(cookie.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    assert_eq!(store.list_threads(&user).await.unwrap().len(), 1);
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/threads",
            Some(&cookie),
            json!({"title":"Different","request_id":request_id})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/threads",
            Some(&cookie),
            json!({"title":"Valid","request_id":"not-a-uuid"})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &app,
            "DELETE",
            &format!("/api/v1/threads/{request_id}"),
            Some(&cookie),
            json!({})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(&app, "POST", "/api/v1/threads", Some(&cookie), body)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let legacy = send(
        &app,
        "POST",
        "/api/v1/threads",
        Some(&cookie),
        json!({"title":"Legacy"}),
    )
    .await;
    assert_eq!(legacy.status(), StatusCode::CREATED);
    let first = value(legacy).await;
    let second = value(
        send(
            &app,
            "POST",
            "/api/v1/threads",
            Some(&cookie),
            json!({"title":"Legacy"}),
        )
        .await,
    )
    .await;
    assert_ne!(first["id"], second["id"]);
    store.logout(&user).await.unwrap();
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/threads",
            Some(&cookie),
            json!({"title":"Legacy","request_id":first["id"]})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}
