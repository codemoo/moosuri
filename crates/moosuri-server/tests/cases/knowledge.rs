use super::*;

fn batch(request: &str, cursor: i64, text: &str) -> Value {
    json!({"request_id":request,"expected_cursor":cursor,"next_cursor":cursor+1,"complete_snapshot":false,
        "entries":[{"external_id":"synthetic-note","version":"1","expected_revision":null,
        "title":"합성 회의 메모","text":text,"speaker":"Synthetic owner","observed_at":1700000000,
        "valid_from":null,"valid_until":null}]})
}
fn draft(text: &str, evidence: Value) -> Value {
    json!({"text":text,"subject":"합성 일정","speaker":"Synthetic owner","valid_from":null,"valid_until":null,"evidence":evidence})
}
async fn create_collection(app: &Router, cookie: &str) -> Value {
    let response = send(
        app,
        "POST",
        "/api/v1/knowledge/collections",
        Some(cookie),
        json!({"name":"개인 메모","scope":"직접 입력"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    value(response).await
}
async fn search(app: &Router, cookie: &str, kind: &str, text: &str) -> Value {
    let response=send(app,"POST","/api/v1/knowledge/search",Some(cookie),json!({"kind":kind,"text":text,"collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null})).await;
    assert_eq!(response.status(), StatusCode::OK);
    value(response).await
}

#[tokio::test]
async fn source_evidence_correction_history_and_forget_have_distinct_scopes() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, _store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let collection = create_collection(&app, &cookie).await;
    let replay_collection = create_collection(&app, &cookie).await;
    assert_eq!(collection["id"], replay_collection["id"]);
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/knowledge/collections",
            Some(&cookie),
            json!({"name":"Changed creation payload","scope":"직접 입력"})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let path = format!(
        "/api/v1/knowledge/collections/{}/batches",
        collection["id"].as_str().unwrap()
    );
    let text = "합성 회의는 금요일입니다. 🌱";
    let payload = batch("a1111111-1111-4111-8111-111111111111", 0, text);
    let response = send(&app, "POST", &path, Some(&cookie), payload.clone()).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let receipt = value(response).await;
    let replay = value(send(&app, "POST", &path, Some(&cookie), payload).await).await;
    assert_eq!(receipt, replay);
    let source = receipt["source_ids"][0].as_str().unwrap();
    assert_eq!(
        search(&app, &cookie, "source", "회의").await["hits"][0]["id"],
        source
    );
    let evidence = json!([{"source_id":source,"source_revision":1,"start_byte":0,"end_byte":text.len(),"quote":text}]);
    let new = json!({"request_id":"a2222222-2222-4222-8222-222222222222","draft":draft("합성 회의는 금요일",evidence)});
    let response = send(
        &app,
        "POST",
        "/api/v1/knowledge/memories",
        Some(&cookie),
        new.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let memory = value(response).await;
    let id = memory["id"].as_str().unwrap();
    let memory_path = format!("/api/v1/knowledge/memories/{id}");
    // A subsequent explicit independent correction survives source deletion;
    // the old source-dependent revision must not survive in history.
    let correction = json!({"request_id":"a3333333-3333-4333-8333-333333333333","expected_revision":1,"draft":draft("합성 회의는 월요일",json!([]))});
    let response = send(&app, "PUT", &memory_path, Some(&cookie), correction).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(value(response).await["revision"], 2);
    let stale=send(&app,"PUT",&memory_path,Some(&cookie),json!({"request_id":"a4444444-4444-4444-8444-444444444444","expected_revision":1,"draft":draft("과거 초안",json!([]))})).await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert_eq!(
        send(
            &app,
            "DELETE",
            &format!("/api/v1/knowledge/sources/{source}"),
            Some(&cookie),
            json!({"expected_revision":1})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        search(&app, &cookie, "memory", "월요일").await["hits"][0]["id"],
        id
    );
    let history = value(
        send(
            &app,
            "GET",
            &format!("{memory_path}/history"),
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(history["items"].as_array().unwrap().len(), 1);
    assert_eq!(history["items"][0]["revision"], 2);
    assert_eq!(
        send(
            &app,
            "GET",
            &format!("{memory_path}?revision=1"),
            Some(&cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            "DELETE",
            &memory_path,
            Some(&cookie),
            json!({"expected_revision":2})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert!(
        search(&app, &cookie, "memory", "월요일").await["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !send(
            &app,
            "POST",
            "/api/v1/knowledge/memories",
            Some(&cookie),
            new
        )
        .await
        .status()
        .is_success()
    );
    assert_eq!(
        send(&app, "GET", &memory_path, Some(&cookie), Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn member_knowledge_is_private_and_disconnect_keep_then_delete_is_explicit() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let admin = login(&app, "admin@example.test", PASSWORD).await;
    let context = store
        .authenticate(admin.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    let invite = store
        .create_user(&context, "Member".into(), "knowledge@example.test".into())
        .await
        .unwrap();
    let invited = store
        .login("knowledge@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let member = store
        .change_password(&invited.user, PASSWORD.into())
        .await
        .unwrap();
    let member = format!("moosuri_session={}", member.token);
    let collection = create_collection(&app, &member).await;
    let id = collection["id"].as_str().unwrap();
    let collection_path = format!("/api/v1/knowledge/collections/{id}");
    let response = send(
        &app,
        "POST",
        &format!("{collection_path}/batches"),
        Some(&member),
        batch(
            "b1111111-1111-4111-8111-111111111111",
            0,
            "Private 합성 문서",
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let receipt = value(response).await;
    let source = receipt["source_ids"][0].as_str().unwrap();
    for path in [
        format!("/api/v1/knowledge/sources/{source}"),
        format!("{collection_path}/sources"),
    ] {
        assert_eq!(
            send(&app, "GET", &path, Some(&admin), Value::Null)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert!(
        search(&app, &admin, "source", "합성").await["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let collections = value(
        send(
            &app,
            "GET",
            "/api/v1/knowledge/collections",
            Some(&member),
            Value::Null,
        )
        .await,
    )
    .await;
    let revision = collections["collections"][0]["revision"].clone();
    let kept = send(
        &app,
        "POST",
        &format!("{collection_path}/disconnect"),
        Some(&member),
        json!({"expected_revision":revision,"mode":"keep"}),
    )
    .await;
    assert_eq!(kept.status(), StatusCode::OK);
    let kept = value(kept).await;
    assert_eq!(kept["connected"], false);
    assert_eq!(
        search(&app, &member, "source", "합성").await["hits"][0]["id"],
        source
    );
    assert!(
        !send(
            &app,
            "POST",
            &format!("{collection_path}/batches"),
            Some(&member),
            batch(
                "b2222222-2222-4222-8222-222222222222",
                1,
                "After disconnect"
            )
        )
        .await
        .status()
        .is_success()
    );
    let deleted = send(
        &app,
        "POST",
        &format!("{collection_path}/disconnect"),
        Some(&member),
        json!({"expected_revision":kept["revision"],"mode":"delete"}),
    )
    .await;
    assert_eq!(deleted.status(), StatusCode::OK);
    assert_eq!(
        send(
            &app,
            "GET",
            &format!("/api/v1/knowledge/sources/{source}"),
            Some(&member),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert!(
        search(&app, &member, "source", "합성").await["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
