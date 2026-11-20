use super::*;
use moosuri_store::{
    personal_browser::{Admission, Output, Scope},
    personal_volume::VolumeOwner,
};

#[tokio::test]
async fn member_task_receipt_round_trips_through_its_own_gateway_and_original_chat() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let options = options(&root);
    let (a, store, service_a) = real_member(&options, "task-a@example.test").await;
    let (b, _, service_b) = real_member(&options, "task-b@example.test").await;
    let gateway = Gateway::new(&options).unwrap();
    let mut auth = Vec::new();
    for (allocation, email) in [(&a, "task-a@example.test"), (&b, "task-b@example.test")] {
        let login = request(
            &gateway,
            "/api/v1/auth/login",
            Method::POST,
            headers(),
            &serde_json::json!({"email":email,"password":"Synthetic-password-42"}).to_string(),
        )
        .await;
        auth.push(authenticated(&cookies(&login), allocation));
    }
    let base = "/api/v1/personal/tasks";
    let user = store
        .authenticate(cookie(&auth[0], SESSION, 2048).unwrap())
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Schedule".into())
        .await
        .unwrap()
        .id;
    let owner = VolumeOwner {
        tenant: a.tenant_id.parse().unwrap(),
        account: a.account_id.parse().unwrap(),
        volume: a.volume_id.parse().unwrap(),
        generation: a.generation as u64,
    };
    let scope = Scope {
        owner,
        user,
        thread: thread.clone(),
    };
    let request_id = Uuid::new_v4();
    let Admission::Fresh(mut job) = store
        .personal_browser_open(
            scope.clone(),
            request_id,
            "https://example.org/".into(),
            "Check the fixture schedule".into(),
            false,
        )
        .await
        .unwrap()
    else {
        panic!("new job")
    };
    let run_id = job.view.run_id.clone();
    job.view.state = "completed".into();
    store
        .personal_browser_finish(
            scope,
            job.view,
            request_id,
            Some(Output::Observation("The fixture starts at 09:30.".into())),
        )
        .await
        .unwrap();
    let page = json(request(&gateway, base, Method::GET, auth[0].clone(), "").await).await;
    assert_eq!(page["items"][0]["id"], run_id);
    let path = format!("{base}/{run_id}");
    let detail = json(request(&gateway, &path, Method::GET, auth[0].clone(), "").await).await;
    assert_eq!(detail["task"]["thread_id"], thread);
    assert_eq!(detail["task"]["next_actor"], "none");
    assert_eq!(detail["result"], "The fixture starts at 09:30.");
    assert_eq!(detail["history"][1]["state"], "completed");
    let conversation = json(
        request(
            &gateway,
            &format!("/api/v1/threads/{thread}/messages"),
            Method::GET,
            auth[0].clone(),
            "",
        )
        .await,
    )
    .await;
    assert!(conversation.to_string().contains(&run_id));
    assert_eq!(
        json(request(&gateway, base, Method::GET, auth[1].clone(), "").await).await["items"],
        serde_json::json!([])
    );
    assert_eq!(
        request(&gateway, &path, Method::GET, auth[1].clone(), "")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&gateway, base, Method::GET, headers(), "")
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &gateway,
            &format!("{base}?before=0"),
            Method::GET,
            auth[0].clone(),
            ""
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &gateway,
            &format!("/api/v1/threads/{thread}"),
            Method::DELETE,
            auth[0].clone(),
            ""
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&gateway, &path, Method::GET, auth[0].clone(), "")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    service_a.abort();
    service_b.abort();
}
