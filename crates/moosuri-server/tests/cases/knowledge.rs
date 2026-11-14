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

#[tokio::test]
async fn source_observation_range_http_separates_validity_binds_pages_and_keeps_owner_withdrawal() {
    use uuid::Uuid;
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let collection = create_collection(&app, &cookie).await;
    let path = format!(
        "/api/v1/knowledge/collections/{}/batches",
        collection["id"].as_str().unwrap()
    );
    let entries: Vec<Value> = [199, 200, 201, 202, 203, 204, 205, 206, 207, 208, 209, 210, 211, 212, 213, 214, 215, 216, 217, 218, 219, 299, 300].into_iter().map(|at| json!({"external_id":format!("observed-{at}"),"version":"1","expected_revision":null,"title":format!("Observed {at}"),"text":"source observation fixture 자료","speaker":null,"observed_at":at,"valid_from":1000,"valid_until":2000})).collect();
    let payload = json!({"request_id":Uuid::new_v4().to_string(),"expected_cursor":0,"next_cursor":1,"complete_snapshot":false,"entries":entries});
    assert_eq!(
        send(&app, "POST", &path, Some(&cookie), payload)
            .await
            .status(),
        StatusCode::CREATED
    );
    let q = json!({"kind":"source","text":"observation","collection_id":null,"subject":null,"speaker":null,"valid_at":1500,"observed_from":200,"observed_until":300,"cursor":null});
    let first = send(
        &app,
        "POST",
        "/api/v1/knowledge/search",
        Some(&cookie),
        q.clone(),
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);
    let first = value(first).await;
    assert_eq!(first["hits"].as_array().unwrap().len(), 20);
    let cursor = first["next_cursor"].as_str().unwrap();
    let mut next = q.clone();
    next["cursor"] = json!(cursor);
    let last = value(
        send(
            &app,
            "POST",
            "/api/v1/knowledge/search",
            Some(&cookie),
            next.clone(),
        )
        .await,
    )
    .await;
    assert_eq!(last["hits"].as_array().unwrap().len(), 1);
    let titles: Vec<&str> = first["hits"]
        .as_array()
        .unwrap()
        .iter()
        .chain(last["hits"].as_array().unwrap())
        .map(|h| h["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains(&"Observed 200") && titles.contains(&"Observed 299"));
    assert!(!titles.contains(&"Observed 199") && !titles.contains(&"Observed 300"));
    for (from, until, title) in [
        (None, Some(200), "Observed 199"),
        (Some(300), None, "Observed 300"),
    ] {
        let mut one_sided = q.clone();
        one_sided["observed_from"] = json!(from);
        one_sided["observed_until"] = json!(until);
        let page = value(
            send(
                &app,
                "POST",
                "/api/v1/knowledge/search",
                Some(&cookie),
                one_sided,
            )
            .await,
        )
        .await;
        assert_eq!(page["hits"].as_array().unwrap().len(), 1);
        assert_eq!(page["hits"][0]["title"], title);
    }
    // A changed range cannot reuse the old authenticated cursor.
    let mut changed = next.clone();
    changed["observed_from"] = json!(201);
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/knowledge/search",
            Some(&cookie),
            changed
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    for (from, until) in [
        (300_i64, 300_i64),
        (301, 300),
        (-62135596801, 300),
        (200, 253402300800),
    ] {
        let mut invalid = q.clone();
        invalid["observed_from"] = json!(from);
        invalid["observed_until"] = json!(until);
        assert_eq!(
            send(
                &app,
                "POST",
                "/api/v1/knowledge/search",
                Some(&cookie),
                invalid
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let mut memory = q.clone();
    memory["kind"] = json!("memory");
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/knowledge/search",
            Some(&cookie),
            memory
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let mut mixed = q.clone();
    mixed["kind"] = json!("all");
    assert!(
        !send(
            &app,
            "POST",
            "/api/v1/knowledge/search",
            Some(&cookie),
            mixed
        )
        .await
        .status()
        .is_success()
    );
    let mut not_valid = q.clone();
    not_valid["valid_at"] = json!(999);
    assert!(
        value(
            send(
                &app,
                "POST",
                "/api/v1/knowledge/search",
                Some(&cookie),
                not_valid
            )
            .await
        )
        .await["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut short = q.clone();
    short["text"] = json!("资料"); // no match, short substring SQL path
    assert!(
        value(
            send(
                &app,
                "POST",
                "/api/v1/knowledge/search",
                Some(&cookie),
                short.clone()
            )
            .await
        )
        .await["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    short["text"] = json!("자료");
    assert_eq!(
        value(
            send(
                &app,
                "POST",
                "/api/v1/knowledge/search",
                Some(&cookie),
                short
            )
            .await
        )
        .await["hits"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
    // Ingestion has no missing-timestamp fallback to now or source validity.
    let mut missing = batch(&Uuid::new_v4().to_string(), 1, "missing observation");
    missing["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("observed_at");
    assert!(
        !send(&app, "POST", &path, Some(&cookie), missing)
            .await
            .status()
            .is_success()
    );
    let context = store
        .authenticate(cookie.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    let invite = store
        .create_user(
            &context,
            "Other".into(),
            "observed-other@example.test".into(),
        )
        .await
        .unwrap();
    let other = store
        .login(
            "observed-other@example.test".into(),
            invite.temporary_password,
        )
        .await
        .unwrap();
    let other = store
        .change_password(&other.user, PASSWORD.into())
        .await
        .unwrap();
    let other_cookie = format!("moosuri_session={}", other.token);
    assert!(
        value(
            send(
                &app,
                "POST",
                "/api/v1/knowledge/search",
                Some(&other_cookie),
                q.clone()
            )
            .await
        )
        .await["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !send(
            &app,
            "POST",
            "/api/v1/knowledge/search",
            Some(&other_cookie),
            next.clone()
        )
        .await
        .status()
        .is_success()
    );
    let id = first["hits"][0]["id"].as_str().unwrap();
    assert_eq!(
        send(
            &app,
            "DELETE",
            &format!("/api/v1/knowledge/sources/{id}"),
            Some(&cookie),
            json!({"expected_revision":1})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/knowledge/search",
            Some(&cookie),
            next
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let after = value(send(&app, "POST", "/api/v1/knowledge/search", Some(&cookie), q).await).await;
    assert!(
        after["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["id"] != id)
    );
}

async fn alias_page(app: &Router, cookie: &str, query: Value) -> Value {
    let response = send(app, "POST", "/api/v1/knowledge/search", Some(cookie), query).await;
    assert_eq!(response.status(), StatusCode::OK);
    value(response).await
}
#[tokio::test]
async fn korean_aliases_http_unions_before_paging_and_fences_mode_scope_time_owner_and_mutation() {
    use uuid::Uuid;
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let collection = create_collection(&app, &cookie).await;
    let path = format!(
        "/api/v1/knowledge/collections/{}/batches",
        collection["id"].as_str().unwrap()
    );
    let words = ["휴대전화", "휴대폰", "핸드폰"];
    let entries:Vec<Value>=(0..23).map(|i|json!({"external_id":format!("alias-{i}"),"version":"1","expected_revision":null,"title":format!("Alias row {i}"),"text":if i==1{"휴대전화 휴대폰 핸드폰"}else{words[i%3]},"speaker":"이메일","observed_at":199+i,"valid_from":1000,"valid_until":2000})).collect();
    let payload = json!({"request_id":Uuid::new_v4().to_string(),"expected_cursor":0,"next_cursor":1,"complete_snapshot":false,"entries":entries});
    assert_eq!(
        send(&app, "POST", &path, Some(&cookie), payload)
            .await
            .status(),
        StatusCode::CREATED
    );
    let q = json!({"kind":"source","text":"휴대폰","match_mode":"korean_aliases_v1","collection_id":null,"subject":null,"speaker":null,"valid_at":1500,"observed_from":200,"observed_until":221,"cursor":null});
    let first = alias_page(&app, &cookie, q.clone()).await;
    assert_eq!(first["hits"].as_array().unwrap().len(), 20);
    let mut next = q.clone();
    next["cursor"] = first["next_cursor"].clone();
    assert!(next["cursor"].is_string());
    let last = alias_page(&app, &cookie, next.clone()).await;
    assert_eq!(last["hits"].as_array().unwrap().len(), 1);
    let ids: std::collections::BTreeSet<_> = first["hits"]
        .as_array()
        .unwrap()
        .iter()
        .chain(last["hits"].as_array().unwrap())
        .map(|h| h["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids.len(),
        21,
        "overlapping aliases must not duplicate a source or skip a page"
    );
    let titles: std::collections::BTreeSet<_> = first["hits"]
        .as_array()
        .unwrap()
        .iter()
        .chain(last["hits"].as_array().unwrap())
        .map(|h| h["title"].as_str().unwrap())
        .collect();
    assert!(titles.contains("Alias row 1") && titles.contains("Alias row 21"));
    assert!(!titles.contains("Alias row 0") && !titles.contains("Alias row 22"));
    for (field, new) in [
        ("match_mode", json!("literal")),
        ("text", json!("핸드폰")),
        ("observed_from", json!(201)),
    ] {
        let mut changed = next.clone();
        changed[field] = new;
        assert_eq!(
            send(
                &app,
                "POST",
                "/api/v1/knowledge/search",
                Some(&cookie),
                changed
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
    }
    let mut invalid = q.clone();
    invalid["match_mode"] = json!("korean_aliases_v2");
    assert!(
        !send(
            &app,
            "POST",
            "/api/v1/knowledge/search",
            Some(&cookie),
            invalid
        )
        .await
        .status()
        .is_success()
    );
    for (field, new) in [("speaker", json!("전자우편")), ("valid_at", json!(999))] {
        let mut excluded = q.clone();
        excluded[field] = new;
        assert!(
            alias_page(&app, &cookie, excluded).await["hits"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    let context = store
        .authenticate(cookie.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    let empty = store
        .create_collection(&context, "Empty scope".into(), Uuid::new_v4().to_string())
        .await
        .unwrap();
    let mut scoped = q.clone();
    scoped["collection_id"] = json!(empty.id);
    assert!(
        alias_page(&app, &cookie, scoped).await["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let first = alias_page(&app, &cookie, q.clone()).await;
    next["cursor"] = first["next_cursor"].clone();
    let invitation = store
        .create_user(
            &context,
            "Alias other".into(),
            "alias-other@example.test".into(),
        )
        .await
        .unwrap();
    let other = store
        .login(
            "alias-other@example.test".into(),
            invitation.temporary_password,
        )
        .await
        .unwrap();
    let other = store
        .change_password(&other.user, PASSWORD.into())
        .await
        .unwrap();
    let other_cookie = format!("moosuri_session={}", other.token);
    assert!(
        alias_page(&app, &other_cookie, q.clone()).await["hits"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !send(
            &app,
            "POST",
            "/api/v1/knowledge/search",
            Some(&other_cookie),
            next.clone()
        )
        .await
        .status()
        .is_success()
    );
    let id = first["hits"][0]["id"].as_str().unwrap();
    assert_eq!(
        send(
            &app,
            "DELETE",
            &format!("/api/v1/knowledge/sources/{id}"),
            Some(&cookie),
            json!({"expected_revision":1})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/knowledge/search",
            Some(&cookie),
            next
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let after = alias_page(&app, &cookie, q).await;
    assert_eq!(after["hits"].as_array().unwrap().len(), 20);
    assert!(after["next_cursor"].is_null());
    assert!(
        after["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["id"] != id)
    );
}
