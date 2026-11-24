use super::*;
use crate::personal_browser::tasks::tests::{Member, member};

// Requests and projections are built from their JSON contract, so these scenarios exercise the
// same snake_case shapes the HTTP layer exchanges.
fn req(id: Uuid, message: &str, items: Value) -> ConversationWorkRequest {
    serde_json::from_value(
        json!({"request_id": id.to_string(), "message": message, "items": items}),
    )
    .unwrap()
}
fn item(title: &str, scope: &str, query: &str) -> Value {
    json!({"title": title, "scope": scope, "query": query})
}
fn act(request: &str, task: &str, revision: i64, body: Value) -> ConversationWorkAction {
    let mut value = json!({"request_id": request, "task_id": task, "revision": revision});
    for (key, field) in body.as_object().unwrap() {
        value[key] = field.clone();
    }
    serde_json::from_value(value).unwrap()
}
fn plain<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap()
}
fn states(group: &Value) -> Vec<String> {
    group["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["state"].as_str().unwrap().to_owned())
        .collect()
}
fn text(value: &Value) -> &str {
    value.as_str().unwrap()
}
macro_rules! remember {
    ($store:expr, $user:expr, $subject:expr, $text:expr) => {
        $store
            .create_memory(
                $user,
                Uuid::new_v4().to_string(),
                moosuri_core::MemoryDraft {
                    subject: $subject.into(),
                    text: $text.into(),
                    speaker: None,
                    evidence: vec![],
                    valid_from: None,
                    valid_until: None,
                },
            )
            .await
            .unwrap()
    };
}
async fn make(m: &Member, id: Uuid, items: Value) -> Value {
    plain(
        &m.store
            .create_conversation_work(
                m.scope.owner,
                m.scope.user.clone(),
                1,
                m.scope.thread.clone(),
                req(id, "여러 조사를 해 줘", items),
            )
            .await
            .unwrap(),
    )
}
async fn control(m: &Member, action: ConversationWorkAction) -> Result<Value> {
    m.store
        .control_conversation_work(
            m.scope.owner,
            m.scope.user.clone(),
            1,
            m.scope.thread.clone(),
            action,
        )
        .await
        .map(|g| plain(&g))
}
async fn page(m: &Member) -> Value {
    plain(
        &m.store
            .conversation_work_page(
                m.scope.owner,
                m.scope.user.clone(),
                m.scope.thread.clone(),
                None,
            )
            .await
            .unwrap(),
    )
}
async fn tick(m: &Member) -> usize {
    m.store
        .tick_member_workflows(m.scope.owner, 1)
        .await
        .unwrap()
}
async fn count(store: &Store, sql: &'static str) -> i64 {
    store
        .call(move |db| Ok(db.conn.query_row(sql, [], |r| r.get(0))?))
        .await
        .unwrap()
}
fn conflict<T>(result: Result<T>) -> bool {
    matches!(result, Err(StoreError::Conflict))
}

#[tokio::test]
async fn create_is_atomic_replays_stable_ids_and_rejects_a_different_payload() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let id = Uuid::new_v4();
    let items = json!([
        item("기억 조사", "memory", "수첩"),
        item("대화 조사", "conversation", "일정"),
        item("질문 필요", "memory", "")
    ]);
    let (a, b) = tokio::join!(
        m.store.create_conversation_work(
            m.scope.owner,
            m.scope.user.clone(),
            1,
            m.scope.thread.clone(),
            req(id, "여러 조사를 해 줘", items.clone())
        ),
        m.store.create_conversation_work(
            m.scope.owner,
            m.scope.user.clone(),
            1,
            m.scope.thread.clone(),
            req(id, "여러 조사를 해 줘", items.clone())
        ),
    );
    let (a, b) = (plain(&a.unwrap()), plain(&b.unwrap()));
    assert_eq!(a, b);
    assert_eq!(a["id"], id.to_string());
    assert_eq!(states(&a), ["queued", "queued", "needs_input"]);
    let titles: Vec<_> = a["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| text(&t["title"]))
        .collect();
    assert_eq!(titles, ["기억 조사", "대화 조사", "질문 필요"]);
    assert!(a["tasks"][0]["question_id"].is_null() && a["tasks"][2]["question_id"].is_string());
    assert!(
        a["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["revision"] == 1 && t["runs"].as_array().unwrap().is_empty())
    );
    // Submitting never searches, and exactly one ordinary receipt turn exists.
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        0
    );
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_conversation_groups").await,
        1
    );
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_conversation_tasks").await,
        3
    );
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM chat_runs WHERE state='completed' AND json_extract(model,'$.provider_id')='member-conversation-work'").await,
        1
    );
    let turn = text(&a["turn_id"]).to_owned();
    let (request, user_text): (String, String) = m
        .store
        .call(move |db| {
            Ok(db.conn.query_row(
                "SELECT request_id,user_text FROM chat_runs WHERE id=?1",
                [turn],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(
        (request.as_str(), user_text.as_str()),
        (id.to_string().as_str(), "여러 조사를 해 줘")
    );
    // The receipt is not a text preview, so it can never be found as a source.
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM personal_text_previews").await,
        0
    );
    let again = plain(
        &m.store
            .create_conversation_work(
                m.scope.owner,
                m.scope.user.clone(),
                1,
                m.scope.thread.clone(),
                req(id, "여러 조사를 해 줘", items.clone()),
            )
            .await
            .unwrap(),
    );
    assert_eq!(again, a);
    for (message, changed) in [
        ("다른 메시지", items.clone()),
        (
            "여러 조사를 해 줘",
            json!([
                item("기억 조사", "memory", "바뀜"),
                item("대화 조사", "conversation", "일정"),
                item("질문 필요", "memory", "")
            ]),
        ),
        (
            "여러 조사를 해 줘",
            json!([item("기억 조사", "memory", "수첩")]),
        ),
    ] {
        let result = m
            .store
            .create_conversation_work(
                m.scope.owner,
                m.scope.user.clone(),
                1,
                m.scope.thread.clone(),
                req(id, message, changed),
            )
            .await;
        assert!(conflict(result));
    }
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_conversation_groups").await,
        1
    );
    for bad in [
        json!([]),
        json!(
            (0..7)
                .map(|i| item(&format!("조사 {i}"), "memory", "수첩"))
                .collect::<Vec<_>>()
        ),
        json!([item("긴 검색어", "memory", &"가".repeat(129))]),
        json!([item("", "memory", "수첩")]),
    ] {
        let result = m
            .store
            .create_conversation_work(
                m.scope.owner,
                m.scope.user.clone(),
                1,
                m.scope.thread.clone(),
                req(Uuid::new_v4(), "검증", bad),
            )
            .await;
        assert!(matches!(result, Err(StoreError::InvalidInput)));
    }
    let elsewhere = m
        .store
        .create_conversation_work(
            m.scope.owner,
            m.scope.user.clone(),
            1,
            Uuid::new_v4().to_string(),
            req(
                Uuid::new_v4(),
                "없는 대화",
                json!([item("조사", "memory", "수첩")]),
            ),
        )
        .await;
    assert!(matches!(elsewhere, Err(StoreError::NotFound)));
    // The same request id in another thread is not a replay.
    let other = m
        .store
        .create_thread(&m.scope.user, "다른 대화".into())
        .await
        .unwrap()
        .id;
    let moved = m
        .store
        .create_conversation_work(
            m.scope.owner,
            m.scope.user.clone(),
            1,
            other,
            req(id, "여러 조사를 해 줘", items),
        )
        .await;
    assert!(conflict(moved));
}

#[tokio::test]
async fn queued_work_survives_restart_and_concurrent_ticks_read_each_child_once() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    remember!(m.store, &m.scope.user, "수첩 색상", "합성 파란 표지");
    let id = Uuid::new_v4();
    make(
        &m,
        id,
        json!([
            item("첫째", "memory", "수첩"),
            item("둘째", "memory", "표지")
        ]),
    )
    .await;
    let Member { root, store, scope } = m;
    drop(store);
    let store = scope
        .owner
        .open_verified(&root.path().canonicalize().unwrap())
        .await
        .unwrap();
    let m = Member { root, store, scope };
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        0
    );
    let (one, two) = tokio::join!(
        m.store.tick_member_workflows(m.scope.owner, 1),
        m.store.tick_member_workflows(m.scope.owner, 1)
    );
    assert_eq!(one.unwrap() + two.unwrap(), 2);
    assert_eq!(tick(&m).await, 0);
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        2
    );
    let listed = page(&m).await;
    let group = &listed["groups"][0];
    assert_eq!(group["id"], id.to_string());
    assert_eq!(states(group), ["completed", "completed"]);
    for task in group["tasks"].as_array().unwrap() {
        assert_eq!(task["revision"], 2);
        assert_eq!(task["runs"].as_array().unwrap().len(), 1);
        assert_eq!(task["run_id"], task["runs"][0]);
        assert_eq!(task["source_count"], 1);
        assert!(text(&task["summary"]).contains("합성 파란 표지"));
        let events = task["events"].as_array().unwrap();
        assert_eq!(
            events.iter().map(|e| text(&e["state"])).collect::<Vec<_>>(),
            ["queued", "completed"]
        );
        assert!(events[0]["at"].as_i64() <= events[1]["at"].as_i64());
    }
    // Each child retains one stable ledger entry after either timer completes it.
    let ledger = m
        .store
        .personal_task_page(m.scope.owner, m.scope.user.clone(), None)
        .await
        .unwrap();
    assert_eq!(
        ledger
            .items
            .iter()
            .filter(|t| t.kind == "conversation_work" && t.state == "completed")
            .count(),
        2
    );
}

#[tokio::test]
async fn one_child_waits_for_an_answer_while_its_sibling_completes_and_replies_are_exact() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    remember!(m.store, &m.scope.user, "수첩 색상", "합성 파란 표지");
    let id = Uuid::new_v4();
    let created = make(
        &m,
        id,
        json!([item("바로", "memory", "수첩"), item("질문", "memory", "")]),
    )
    .await;
    let (ready, asking) = (
        text(&created["tasks"][0]["id"]).to_owned(),
        text(&created["tasks"][1]["id"]).to_owned(),
    );
    let question = text(&created["tasks"][1]["question_id"]).to_owned();
    assert_eq!(tick(&m).await, 1);
    assert_eq!(tick(&m).await, 0);
    let group = page(&m).await["groups"][0].clone();
    assert_eq!(states(&group), ["completed", "needs_input"]);
    assert!(group["tasks"][1]["run_id"].is_null());
    let id = id.to_string();
    let reply = |revision, question: &str, term: &str| {
        act(
            &id,
            &asking,
            revision,
            json!({"action": "answer", "question_id": question, "text": term}),
        )
    };
    assert!(conflict(
        control(&m, reply(1, &Uuid::new_v4().to_string(), "수첩")).await
    ));
    assert!(conflict(control(&m, reply(7, &question, "수첩")).await));
    assert!(matches!(
        control(&m, reply(1, &question, "  ")).await,
        Err(StoreError::InvalidInput)
    ));
    let queued = control(&m, reply(1, &question, "수첩")).await.unwrap();
    assert_eq!(queued["tasks"][1]["state"], "queued");
    assert_eq!(queued["tasks"][1]["revision"], 2);
    assert_eq!(queued["tasks"][1]["query"], "수첩");
    assert!(queued["tasks"][1]["question_id"].is_null());
    // An exact retry is idempotent; a different answer to the same question is stale.
    assert_eq!(
        control(&m, reply(1, &question, "수첩")).await.unwrap(),
        queued
    );
    assert!(conflict(control(&m, reply(1, &question, "다른말")).await));
    assert_eq!(tick(&m).await, 1);
    let done = page(&m).await["groups"][0].clone();
    assert_eq!(states(&done), ["completed", "completed"]);
    assert_eq!(done["tasks"][1]["revision"], 3);
    assert_eq!(done["tasks"][1]["source_count"], 1);
    assert!(conflict(control(&m, reply(3, &question, "수첩")).await));
    assert_eq!(
        states(&control(&m, reply(1, &question, "수첩")).await.unwrap()),
        ["completed", "completed"]
    );
    // A child cannot be driven through another group's id.
    let foreign = act(
        &Uuid::new_v4().to_string(),
        &ready,
        2,
        json!({"action": "cancel"}),
    );
    assert!(matches!(
        control(&m, foreign).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        2
    );
}

#[tokio::test]
async fn two_children_refine_independently_with_a_real_and_filter_before_the_hit_limit() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let user = &m.scope.user;
    remember!(m.store, user, "수첩 파란", "파란 표지 수첩");
    remember!(m.store, user, "수첩 초록", "초록 표지 수첩");
    let id = Uuid::new_v4();
    let created = make(
        &m,
        id,
        json!([
            item("파랑 찾기", "memory", "수첩"),
            item("초록 찾기", "memory", "수첩")
        ]),
    )
    .await;
    let (a, b) = (
        text(&created["tasks"][0]["id"]).to_owned(),
        text(&created["tasks"][1]["id"]).to_owned(),
    );
    assert_eq!(tick(&m).await, 2);
    let first = page(&m).await["groups"][0].clone();
    let (first_a, first_b) = (
        text(&first["tasks"][0]["run_id"]).to_owned(),
        text(&first["tasks"][1]["run_id"]).to_owned(),
    );
    assert_eq!(
        (
            first["tasks"][0]["source_count"].clone(),
            first["tasks"][1]["source_count"].clone()
        ),
        (json!(2), json!(2))
    );
    let id = id.to_string();
    let refine = |task: &str, revision, term: &str| {
        act(
            &id,
            task,
            revision,
            json!({"action": "refine", "text": term}),
        )
    };
    let (one, two) = tokio::join!(
        control(&m, refine(&a, 2, "파란")),
        control(&m, refine(&b, 2, "초록"))
    );
    let (one, two) = (one.unwrap(), two.unwrap());
    // Whichever commits first, each reply reflects its own child's new condition.
    assert_eq!(one["tasks"][0]["filter"], "파란");
    assert_eq!(two["tasks"][1]["filter"], "초록");
    assert!(conflict(control(&m, refine(&a, 2, "표지")).await));
    assert_eq!(tick(&m).await, 2);
    let after = page(&m).await["groups"][0].clone();
    for (index, keep, drop) in [(0, "파란", "초록"), (1, "초록", "파란")] {
        let task = &after["tasks"][index];
        assert_eq!(task["state"], "completed");
        assert_eq!(task["runs"].as_array().unwrap().len(), 2);
        assert_eq!(task["source_count"], 1);
        assert!(text(&task["summary"]).contains(keep) && !text(&task["summary"]).contains(drop));
        assert_eq!(task["filter"], keep);
    }
    // The earlier run is neither rewritten nor removed.
    assert_eq!(after["tasks"][0]["runs"][0], first_a.as_str());
    assert_eq!(after["tasks"][1]["runs"][0], first_b.as_str());
    let old = first_a.clone();
    let kept = m
        .store
        .call(move |db| {
            Ok(db.conn.query_row(
                "SELECT json_array_length(sources) FROM member_work_runs WHERE id=?1",
                [old],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(kept, 2);
    // A queued child can be refined again; a repeated condition is rejected.
    let mut revision = after["tasks"][0]["revision"].as_i64().unwrap();
    let requeued = control(&m, refine(&a, revision, "표지")).await.unwrap();
    revision = requeued["tasks"][0]["revision"].as_i64().unwrap();
    assert_eq!(requeued["tasks"][0]["state"], "queued");
    assert!(matches!(
        control(&m, refine(&a, revision, "표지")).await,
        Err(StoreError::InvalidInput)
    ));
    let narrowed = control(&m, refine(&a, revision, "수첩")).await.unwrap();
    assert_eq!(narrowed["tasks"][0]["filter"], "파란 AND 표지 AND 수첩");
    assert_eq!(tick(&m).await, 1);
    assert_eq!(
        page(&m).await["groups"][0]["tasks"][0]["runs"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[tokio::test]
async fn memory_filters_apply_before_the_eight_hit_limit() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let user = &m.scope.user;
    for i in 0..12 {
        remember!(
            m.store,
            user,
            format!("수첩 {i}"),
            format!("수첩 일반 메모 {i}")
        );
    }
    remember!(m.store, user, "수첩 특별", "수첩 희귀 문구");
    let id = Uuid::new_v4();
    let created = make(&m, id, json!([item("수첩 찾기", "memory", "수첩")])).await;
    let task = text(&created["tasks"][0]["id"]).to_owned();
    tick(&m).await;
    let broad = page(&m).await["groups"][0].clone();
    assert_eq!(broad["tasks"][0]["source_count"], 8);
    let narrowed = control(
        &m,
        act(
            &id.to_string(),
            &task,
            2,
            json!({"action": "refine", "text": "희귀"}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(narrowed["tasks"][0]["state"], "queued");
    tick(&m).await;
    let done = page(&m).await["groups"][0].clone();
    assert_eq!(done["tasks"][0]["source_count"], 1);
    assert!(text(&done["tasks"][0]["summary"]).contains("희귀 문구"));
    // The ledger opens the run with the added condition spelled out.
    let run = Uuid::parse_str(text(&done["tasks"][0]["run_id"])).unwrap();
    let detail = m
        .store
        .personal_task_detail(m.scope.owner, user.clone(), run)
        .await
        .unwrap();
    assert!(detail.request.contains("그리고 ‘희귀’"));
}

#[tokio::test]
async fn conversation_scope_accepts_filters_and_status_is_a_pure_read() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let id = Uuid::new_v4();
    let created = make(
        &m,
        id,
        json!([
            item("대화", "conversation", "일정"),
            item("기억", "memory", "수첩")
        ]),
    )
    .await;
    let before = (
        count(&m.store, "SELECT count(*) FROM member_conversation_events").await,
        count(&m.store, "SELECT count(*) FROM chat_runs").await,
    );
    for _ in 0..3 {
        let listed = page(&m).await;
        assert_eq!(listed["groups"][0], created);
        assert_eq!(states(&listed["groups"][0]), ["queued", "queued"]);
    }
    let older = plain(
        &m.store
            .conversation_work_page(
                m.scope.owner,
                m.scope.user.clone(),
                m.scope.thread.clone(),
                Some(id),
            )
            .await
            .unwrap(),
    );
    assert_eq!(older["groups"][0], created);
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        0
    );
    assert_eq!(
        (
            count(&m.store, "SELECT count(*) FROM member_conversation_events").await,
            count(&m.store, "SELECT count(*) FROM chat_runs").await
        ),
        before
    );
    assert_eq!(tick(&m).await, 2);
    let task = text(&created["tasks"][0]["id"]).to_owned();
    let requeued = control(
        &m,
        act(
            &id.to_string(),
            &task,
            2,
            json!({"action": "refine", "text": "회의"}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(requeued["tasks"][0]["filter"], "회의");
    assert_eq!(tick(&m).await, 1);
    let done = page(&m).await["groups"][0].clone();
    assert_eq!(done["tasks"][0]["state"], "completed");
    assert_eq!(done["tasks"][0]["source_count"], 0);
}

#[tokio::test]
async fn pages_show_the_forty_most_recent_groups_oldest_first_and_older_ones_by_id() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let mut ids = Vec::new();
    for _ in 0..42 {
        let id = Uuid::new_v4();
        make(&m, id, json!([item("조사", "memory", "수첩")])).await;
        tick(&m).await;
        ids.push(id.to_string());
    }
    let listed = page(&m).await;
    let seen: Vec<_> = listed["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| text(&g["id"]).to_owned())
        .collect();
    assert_eq!(seen, ids[2..]);
    let oldest = plain(
        &m.store
            .conversation_work_page(
                m.scope.owner,
                m.scope.user.clone(),
                m.scope.thread.clone(),
                Some(ids[0].parse().unwrap()),
            )
            .await
            .unwrap(),
    );
    assert_eq!(oldest["groups"].as_array().unwrap().len(), 1);
    assert_eq!(oldest["groups"][0]["id"], ids[0].as_str());
    let missing = m
        .store
        .conversation_work_page(
            m.scope.owner,
            m.scope.user.clone(),
            m.scope.thread.clone(),
            Some(Uuid::new_v4()),
        )
        .await;
    assert!(matches!(missing, Err(StoreError::NotFound)));
}

#[tokio::test]
async fn pending_work_is_bounded_and_cancel_prevents_a_read() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let six = || {
        json!(
            (0..6)
                .map(|i| item(&format!("조사 {i}"), "memory", "수첩"))
                .collect::<Vec<_>>()
        )
    };
    let mut last = None;
    for _ in 0..4 {
        let id = Uuid::new_v4();
        last = Some((id, make(&m, id, six()).await));
    }
    let over = m
        .store
        .create_conversation_work(
            m.scope.owner,
            m.scope.user.clone(),
            1,
            m.scope.thread.clone(),
            req(
                Uuid::new_v4(),
                "넘침",
                json!([item("하나 더", "memory", "수첩")]),
            ),
        )
        .await;
    assert!(matches!(over, Err(StoreError::BudgetExhausted)));
    let (id, group) = last.unwrap();
    let id = id.to_string();
    let ids: Vec<_> = group["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| text(&t["id"]).to_owned())
        .collect();
    let cancel = |task: &str, revision| act(&id, task, revision, json!({"action": "cancel"}));
    let cancelled = control(&m, cancel(&ids[0], 1)).await.unwrap();
    assert_eq!(cancelled["tasks"][0]["state"], "cancelled");
    assert_eq!(control(&m, cancel(&ids[0], 1)).await.unwrap(), cancelled);
    assert!(conflict(control(&m, cancel(&ids[0], 2)).await));
    let refine = act(&id, &ids[0], 2, json!({"action": "refine", "text": "표지"}));
    assert!(conflict(control(&m, refine).await));
    // Cancelling one child frees a slot, and nothing cancelled is ever read.
    m.store
        .create_conversation_work(
            m.scope.owner,
            m.scope.user.clone(),
            1,
            m.scope.thread.clone(),
            req(
                Uuid::new_v4(),
                "이제 됨",
                json!([item("하나 더", "memory", "수첩")]),
            ),
        )
        .await
        .unwrap();
    for _ in 0..8 {
        tick(&m).await;
    }
    let cancelled_id = ids[0].clone();
    let cancelled_runs = m
        .store
        .call(move |db| {
            Ok(db.conn.query_row(
                "SELECT count(*) FROM member_work_runs WHERE workflow_id=?1",
                [cancelled_id],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(cancelled_runs, 0);
    assert_eq!(
        count(
            &m.store,
            "SELECT count(*) FROM member_conversation_tasks WHERE state='completed'"
        )
        .await,
        24
    );
    let finished = act(&id, &ids[1], 2, json!({"action": "cancel"}));
    assert!(conflict(control(&m, finished).await));
}

#[tokio::test]
async fn the_timer_serves_groups_fairly_without_letting_one_starve_the_rest() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let big = Uuid::new_v4();
    make(
        &m,
        big,
        json!(
            (0..6)
                .map(|i| item(&format!("큰 조사 {i}"), "memory", "수첩"))
                .collect::<Vec<_>>()
        ),
    )
    .await;
    for i in 0..3 {
        make(
            &m,
            Uuid::new_v4(),
            json!([item(&format!("작은 조사 {i}"), "memory", "수첩")]),
        )
        .await;
    }
    let snapshot = || async {
        let listed = page(&m).await;
        let mut all = Vec::new();
        for g in listed["groups"].as_array().unwrap() {
            all.push(states(g));
        }
        all
    };
    assert_eq!(tick(&m).await, 4);
    let first = snapshot().await;
    assert_eq!(
        first[0],
        [
            "completed",
            "completed",
            "queued",
            "queued",
            "queued",
            "queued"
        ]
    );
    assert_eq!(first[1], ["completed"]);
    assert_eq!(first[2], ["completed"]);
    assert_eq!(first[3], ["queued"]);
    assert_eq!(tick(&m).await, 3);
    assert_eq!(tick(&m).await, 2);
    assert_eq!(tick(&m).await, 0);
    assert!(snapshot().await.iter().flatten().all(|s| s == "completed"));
}

#[tokio::test]
async fn stale_host_or_auth_authority_closes_queued_work_instead_of_reading_late() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let id = Uuid::new_v4();
    make(&m, id, json!([item("조사", "memory", "수첩")])).await;
    // An older in-flight tick never reads or revokes newer work.
    assert_eq!(
        m.store
            .tick_member_workflows(m.scope.owner, 0)
            .await
            .unwrap(),
        0
    );
    assert_eq!(states(&page(&m).await["groups"][0]), ["queued"]);
    assert_eq!(
        m.store
            .tick_member_workflows(m.scope.owner, 2)
            .await
            .unwrap(),
        0
    );
    let failed = page(&m).await["groups"][0].clone();
    assert_eq!(states(&failed), ["failed"]);
    assert_eq!(
        failed["tasks"][0]["events"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["state"],
        "failed"
    );
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        0
    );
    // A restore quarantines everything still queued.
    let restored = Uuid::new_v4();
    make(
        &m,
        restored,
        json!([
            item("복원 전", "memory", "수첩"),
            item("이전 질문", "memory", "")
        ]),
    )
    .await;
    m.store
        .call(|db| crate::schedules::quarantine_for_restore(&db.conn))
        .await
        .unwrap();
    assert_eq!(tick(&m).await, 0);
    assert_eq!(count(&m.store, "SELECT count(*) FROM member_conversation_tasks WHERE state='cancelled' AND question_id IS NULL").await, 2);
    // So does an account whose authorization changed after the work was granted.
    make(
        &m,
        Uuid::new_v4(),
        json!([item("권한 변경 전", "memory", "수첩")]),
    )
    .await;
    m.store
        .call(|db| {
            Ok(db
                .conn
                .execute("UPDATE accounts SET auth_epoch=auth_epoch+1", [])?)
        })
        .await
        .unwrap();
    assert_eq!(tick(&m).await, 0);
    assert_eq!(
        count(
            &m.store,
            "SELECT count(*) FROM member_conversation_tasks WHERE state='failed'"
        )
        .await,
        2
    );
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        0
    );
}

#[tokio::test]
async fn withdrawn_sources_are_hidden_and_reuse_needs_a_readable_proven_search() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let user = &m.scope.user;
    let memory = remember!(m.store, user, "수첩 가", "수첩 가 합성 내용");
    let id = Uuid::new_v4();
    let created = make(&m, id, json!([item("수첩", "memory", "수첩")])).await;
    let task = text(&created["tasks"][0]["id"]).to_owned();
    // Nothing has been proven yet, so there is nothing to reuse.
    let fresh = Uuid::new_v4().to_string();
    let early = act(
        &fresh,
        &task,
        1,
        json!({"action": "reuse", "text": "수첩 가"}),
    );
    assert!(conflict(control(&m, early).await));
    assert_eq!(tick(&m).await, 1);
    let reuse = |request: &str, revision, term: &str| {
        act(
            request,
            &task,
            revision,
            json!({"action": "reuse", "text": term}),
        )
    };
    let request = Uuid::new_v4().to_string();
    let reused = control(&m, reuse(&request, 2, "수첩 가")).await.unwrap();
    assert_eq!(reused["id"], request.as_str());
    assert_ne!(reused["turn_id"], created["turn_id"]);
    let child = &reused["tasks"][0];
    assert_ne!(child["id"], created["tasks"][0]["id"]);
    assert_eq!(
        (
            child["state"].clone(),
            child["scope"].clone(),
            child["query"].clone()
        ),
        (json!("queued"), json!("memory"), json!("수첩 가"))
    );
    assert!(child["run_id"].is_null() && child["summary"].is_null() && child["source_count"] == 0);
    assert_eq!(
        control(&m, reuse(&request, 2, "수첩 가")).await.unwrap(),
        reused
    );
    assert!(conflict(control(&m, reuse(&request, 2, "다른 말")).await));
    assert!(conflict(
        control(&m, reuse(&Uuid::new_v4().to_string(), 1, "수첩 가")).await
    ));
    assert_eq!(count(&m.store, "SELECT count(*) FROM chat_runs WHERE json_extract(model,'$.provider_id')='member-conversation-work'").await, 2);
    assert_eq!(tick(&m).await, 1);
    // The source is withdrawn: the historical count stays, its text does not.
    m.store
        .forget_memory(user, memory.id, memory.revision)
        .await
        .unwrap();
    let listed = page(&m).await;
    let original = &listed["groups"][0]["tasks"][0];
    assert_eq!(original["source_count"], 1);
    assert!(!text(&original["summary"]).contains("합성 내용"));
    assert!(text(&original["summary"]).contains("더 이상 볼 수 없습니다"));
    assert!(
        !serde_json::to_string(&listed)
            .unwrap()
            .contains("합성 내용")
    );
    let late = act(
        &Uuid::new_v4().to_string(),
        &task,
        2,
        json!({"action": "reuse", "text": "수첩 가"}),
    );
    assert!(conflict(control(&m, late).await));
}

#[tokio::test]
async fn groups_stay_inside_their_thread_volume_and_generation() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let id = Uuid::new_v4();
    let created = make(&m, id, json!([item("조사", "memory", "수첩")])).await;
    let task = text(&created["tasks"][0]["id"]).to_owned();
    let other = m
        .store
        .create_thread(&m.scope.user, "다른 대화".into())
        .await
        .unwrap()
        .id;
    let seen = plain(
        &m.store
            .conversation_work_page(m.scope.owner, m.scope.user.clone(), other.clone(), None)
            .await
            .unwrap(),
    );
    assert!(seen["groups"].as_array().unwrap().is_empty());
    let by_id = m
        .store
        .conversation_work_page(m.scope.owner, m.scope.user.clone(), other.clone(), Some(id))
        .await;
    assert!(matches!(by_id, Err(StoreError::NotFound)));
    let cancel = act(&id.to_string(), &task, 1, json!({"action": "cancel"}));
    let moved = m
        .store
        .control_conversation_work(m.scope.owner, m.scope.user.clone(), 1, other, cancel)
        .await;
    assert!(matches!(moved, Err(StoreError::NotFound)));
    let mut newer = m.scope.owner;
    newer.generation += 1;
    assert!(
        m.store
            .conversation_work_page(newer, m.scope.user.clone(), m.scope.thread.clone(), None)
            .await
            .is_err()
    );
    assert!(m.store.tick_member_workflows(newer, 1).await.is_err());
    assert_eq!(states(&page(&m).await["groups"][0]), ["queued"]);
}

#[tokio::test]
async fn the_work_ledger_lists_pending_children_and_opens_them() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    remember!(m.store, &m.scope.user, "수첩 색상", "합성 파란 표지");
    let id = Uuid::new_v4();
    let created = make(
        &m,
        id,
        json!([item("바로", "memory", "수첩"), item("질문", "memory", "")]),
    )
    .await;
    let (ready, asking) = (
        text(&created["tasks"][0]["id"]).to_owned(),
        text(&created["tasks"][1]["id"]).to_owned(),
    );
    let listed = m
        .store
        .personal_task_page(m.scope.owner, m.scope.user.clone(), None)
        .await
        .unwrap();
    let pending: Vec<_> = listed
        .items
        .iter()
        .filter(|t| t.kind == "conversation_work")
        .map(|t| (t.id.clone(), t.state.clone(), t.next_actor.clone()))
        .collect();
    assert_eq!(pending.len(), 2);
    assert!(pending.contains(&(ready.clone(), "queued".into(), "assistant".into())));
    assert!(pending.contains(&(asking.clone(), "needs_input".into(), "member".into())));
    assert!(listed.items.iter().all(|t| t.thread_id == m.scope.thread));
    let detail = m
        .store
        .personal_task_detail(m.scope.owner, m.scope.user.clone(), ready.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(detail.request, "여러 조사를 해 줘");
    assert_eq!(detail.history[0].state, "queued");
    assert!(detail.result.is_none());
    tick(&m).await;
    let after = m
        .store
        .personal_task_page(m.scope.owner, m.scope.user.clone(), None)
        .await
        .unwrap();
    let finished = after.items.iter().find(|t| t.id == ready).unwrap();
    assert_eq!(finished.state, "completed");
    assert_eq!(after.items.len(), 2);
    assert!(
        after
            .items
            .iter()
            .any(|t| t.id == asking && t.state == "needs_input")
    );
    let opened = m
        .store
        .personal_task_detail(m.scope.owner, m.scope.user.clone(), ready.parse().unwrap())
        .await
        .unwrap();
    assert_eq!(opened.runs.len(), 1);
    assert_eq!(
        opened.origin_turn_id.as_deref(),
        Some(text(&created["turn_id"]))
    );
    assert!(opened.result.unwrap().contains("1건"));
}

#[tokio::test]
async fn save_report_is_bound_to_the_current_completed_run_and_replays() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    remember!(m.store, &m.scope.user, "수첩 색상", "합성 파란 표지");
    let id = Uuid::new_v4();
    let created = make(&m, id, json!([item("보고서", "memory", "수첩")])).await;
    let task = text(&created["tasks"][0]["id"]).to_owned();
    let save = |revision| {
        act(
            &id.to_string(),
            &task,
            revision,
            json!({"action": "save_report"}),
        )
    };
    assert!(conflict(control(&m, save(1)).await));
    tick(&m).await;
    let saved = control(&m, save(2)).await.unwrap();
    assert_eq!(saved["tasks"][0]["state"], "completed");
    assert_eq!(saved["tasks"][0]["revision"], 3);
    assert!(saved["tasks"][0]["report_id"].is_string());
    assert_eq!(control(&m, save(2)).await.unwrap(), saved);
    assert_eq!(page(&m).await["groups"][0], saved);
}

#[tokio::test]
async fn failed_recurring_search_cannot_rollback_independent_chat_work() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let group = make(
        &m,
        Uuid::new_v4(),
        json!([item("별도 조사", "memory", "수첩")]),
    )
    .await;
    let schedule = Uuid::new_v4();
    let request = moosuri_core::personal_browser::WorkRequest {
        request_id: schedule.to_string(),
        title: "예약 조사".into(),
        scope: WorkScope::Memory,
        query: "수첩".into(),
        period: 1800,
        first_due: None,
        thread_id: None,
        message: None,
    };
    m.store
        .create_member_workflow(m.scope.owner, m.scope.user.clone(), 1, request)
        .await
        .unwrap();
    m.store
        .call(|db| {
            db.conn.execute_batch(
                "UPDATE member_workflows SET next_due=unixepoch()-1 WHERE period>0;
            CREATE TRIGGER synthetic_failed_due BEFORE INSERT ON member_work_runs
            WHEN NEW.due_at IS NOT NULL BEGIN SELECT RAISE(ABORT,'fixture failure'); END;",
            )?;
            Ok(())
        })
        .await
        .unwrap();
    for _ in 0..2 {
        assert!(
            m.store
                .tick_member_workflows(m.scope.owner, 1)
                .await
                .is_err()
        );
    }
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        1
    );
    let current = &page(&m).await["groups"][0];
    assert_eq!(current["id"], group["id"]);
    assert_eq!(states(current), ["completed"]);
}

// ------------------------------------------------------------ combined group reports

async fn make_reported(m: &Member, id: Uuid, items: Value, save: bool) -> Result<Value> {
    m.store
        .create_conversation_work_with_report(
            m.scope.owner,
            m.scope.user.clone(),
            1,
            m.scope.thread.clone(),
            req(id, "조사하고 보고서도 만들어 줘", items),
            save,
        )
        .await
        .map(|g| plain(&g))
}
async fn ask_in(m: &Member, thread: String, group: &str, request: &str) -> Result<Value> {
    m.store
        .request_conversation_group_report(
            m.scope.owner,
            m.scope.user.clone(),
            1,
            thread,
            group.to_owned(),
            request.to_owned(),
        )
        .await
        .map(|g| plain(&g))
}
async fn ask(m: &Member, group: &str, request: Uuid) -> Result<Value> {
    ask_in(m, m.scope.thread.clone(), group, &request.to_string()).await
}
async fn open(
    m: &Member,
    thread: &str,
    id: &str,
) -> Option<moosuri_core::personal_browser::ConversationReport> {
    let (owner, thread, id) = (m.scope.owner, thread.to_owned(), id.to_owned());
    m.store
        .call(move |db| group_report(&db.conn, owner, &thread, &id))
        .await
        .unwrap()
}
async fn in_state(store: &Store, state: &'static str) -> i64 {
    store
        .call(move |db| {
            Ok(db.conn.query_row(
                "SELECT count(*) FROM member_conversation_group_reports WHERE state=?1",
                [state],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .await
        .unwrap()
}
fn reported(group: &Value) -> Option<&str> {
    group.get("group_report_id").and_then(Value::as_str)
}

#[tokio::test]
async fn combined_report_waits_for_every_child_then_opens_once_and_drops_withdrawn_text() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let notebook = remember!(m.store, &m.scope.user, "수첩 색상", "합성 파란 표지");
    remember!(m.store, &m.scope.user, "잉크 색상", "합성 검은 잉크");
    let id = Uuid::new_v4();
    let items = json!([
        item("수첩", "memory", "수첩"),
        item("잉크", "memory", "잉크"),
        item("질문", "memory", "")
    ]);
    let created = make_reported(&m, id, items.clone(), true).await.unwrap();
    assert_eq!(created["report_requested"], true);
    assert!(reported(&created).is_none());
    // Exact replay is the same group; a changed flag or payload under the same id conflicts.
    assert_eq!(
        make_reported(&m, id, items.clone(), true).await.unwrap(),
        created
    );
    assert!(conflict(make_reported(&m, id, items.clone(), false).await));
    assert!(conflict(
        m.store
            .create_conversation_work(
                m.scope.owner,
                m.scope.user.clone(),
                1,
                m.scope.thread.clone(),
                req(id, "조사하고 보고서도 만들어 줘", items.clone())
            )
            .await
    ));
    let rows = "SELECT count(*) FROM member_conversation_group_reports";
    assert_eq!(count(&m.store, rows).await, 1);
    // Two children finish, but one still waits for an answer: no early result.
    assert_eq!(tick(&m).await, 2);
    let waiting = page(&m).await["groups"][0].clone();
    assert_eq!(waiting["report_requested"], true);
    assert!(reported(&waiting).is_none());
    assert_eq!(
        count(
            &m.store,
            "SELECT count(*) FROM member_conversation_group_reports WHERE state='ready'"
        )
        .await,
        0
    );
    assert!(
        open(&m, &m.scope.thread, &Uuid::new_v4().to_string())
            .await
            .is_none()
    );
    let (asking, question) = (
        text(&created["tasks"][2]["id"]).to_owned(),
        text(&created["tasks"][2]["question_id"]).to_owned(),
    );
    control(
        &m,
        act(
            &id.to_string(),
            &asking,
            1,
            json!({"action": "answer", "question_id": question, "text": "수첩"}),
        ),
    )
    .await
    .unwrap();
    assert!(reported(&page(&m).await["groups"][0]).is_none());
    // The timer alone completes the report; nobody has to open anything.
    assert_eq!(tick(&m).await, 1);
    let done = page(&m).await["groups"][0].clone();
    let report = reported(&done).unwrap().to_owned();
    assert_eq!(done["report_requested"], true);
    assert!(
        done["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["report_id"].is_null())
    );
    assert_eq!(tick(&m).await, 0);
    assert_eq!(page(&m).await["groups"][0], done);
    // Reload: the same single object, with every child's evidence.
    let Member { root, store, scope } = m;
    drop(store);
    let store = scope
        .owner
        .open_verified(&root.path().canonicalize().unwrap())
        .await
        .unwrap();
    let m = Member { root, store, scope };
    let thread = m.scope.thread.clone();
    let opened = open(&m, &thread, &report).await.unwrap();
    assert_eq!(opened.id, report);
    assert_eq!(opened.title, "조사 3건 묶음 보고서");
    assert_eq!(opened.run_id, text(&done["tasks"][0]["run_id"]));
    assert!(opened.body.contains("3건 중 3건"));
    for expected in [
        "1. 수첩",
        "2. 잉크",
        "3. 질문",
        "읽은 근거",
        "합성 파란 표지",
        "합성 검은 잉크",
    ] {
        assert!(opened.body.contains(expected), "{expected}");
    }
    assert_eq!(page(&m).await["groups"][0], done);
    // Withdrawing a source removes its text from the combined report but not the other child's.
    m.store
        .forget_memory(&m.scope.user, notebook.id, notebook.revision)
        .await
        .unwrap();
    let after = open(&m, &thread, &report).await.unwrap();
    assert!(!after.body.contains("합성 파란 표지"));
    assert!(after.body.contains("더 이상 볼 수 없습니다"));
    assert!(after.body.contains("합성 검은 잉크"));
    // The report is not copied into plain chat history.
    assert_eq!(
        count(
            &m.store,
            "SELECT count(*) FROM chat_output WHERE text LIKE '%합성 검은 잉크%'"
        )
        .await,
        0
    );
}

#[tokio::test]
async fn a_report_requested_after_completion_replays_binds_to_one_group_and_keeps_history() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    remember!(m.store, &m.scope.user, "수첩 파란", "파란 표지 수첩");
    remember!(m.store, &m.scope.user, "수첩 초록", "초록 표지 수첩");
    let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
    let items = json!([
        item("파랑", "memory", "수첩"),
        item("초록", "memory", "수첩")
    ]);
    let plain_group = make(&m, a, items.clone()).await;
    // Groups made without a report carry neither optional field.
    assert!(plain_group.get("report_requested").is_none() && reported(&plain_group).is_none());
    let old: ConversationWorkGroup = serde_json::from_value(plain_group.clone()).unwrap();
    assert!(old.group_report_id.is_none() && !old.report_requested);
    make(&m, b, items).await;
    // Early request: pending until both children are read.
    let first = Uuid::new_v4();
    let pending = ask(&m, &a.to_string(), first).await.unwrap();
    assert_eq!(pending["report_requested"], true);
    assert!(reported(&pending).is_none());
    assert_eq!(ask(&m, &a.to_string(), first).await.unwrap(), pending);
    assert_eq!(
        count(
            &m.store,
            "SELECT count(*) FROM member_conversation_group_reports"
        )
        .await,
        1
    );
    assert_eq!(tick(&m).await, 4);
    let listed = page(&m).await;
    let ready = listed["groups"][0].clone();
    let report = reported(&ready).unwrap().to_owned();
    assert!(reported(&listed["groups"][1]).is_none());
    // Another request id for a report that exists, and an exact replay, are the same object.
    assert_eq!(
        ask(&m, &a.to_string(), Uuid::new_v4()).await.unwrap(),
        ready
    );
    assert_eq!(ask(&m, &a.to_string(), first).await.unwrap(), ready);
    // One request id never serves two groups, and the id of another group is not a request id.
    assert!(conflict(ask(&m, &b.to_string(), first).await));
    assert!(conflict(ask(&m, &b.to_string(), a).await));
    assert!(matches!(
        ask(&m, &Uuid::new_v4().to_string(), Uuid::new_v4()).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        ask_in(&m, m.scope.thread.clone(), &b.to_string(), "not-a-uuid").await,
        Err(StoreError::InvalidInput)
    ));
    let elsewhere = m
        .store
        .create_thread(&m.scope.user, "다른 대화".into())
        .await
        .unwrap()
        .id;
    assert!(matches!(
        ask_in(&m, elsewhere, &b.to_string(), &Uuid::new_v4().to_string()).await,
        Err(StoreError::NotFound)
    ));
    // Completed work can be reported at once.
    let late = ask(&m, &b.to_string(), Uuid::new_v4()).await.unwrap();
    assert!(reported(&late).is_some() && late["report_requested"] == true);
    assert_eq!(
        count(
            &m.store,
            "SELECT count(*) FROM member_conversation_group_reports WHERE state='ready'"
        )
        .await,
        2
    );
    // Refining a child later leaves the saved report on the run it was built from.
    let thread = m.scope.thread.clone();
    let before = open(&m, &thread, &report).await.unwrap();
    assert!(!before.body.contains("다시 조사"));
    let child = &ready["tasks"][0];
    let first_run = text(&child["run_id"]).to_owned();
    control(
        &m,
        act(
            &a.to_string(),
            text(&child["id"]),
            2,
            json!({"action": "refine", "text": "파란"}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(tick(&m).await, 1);
    let current = page(&m).await["groups"][0].clone();
    assert_eq!(current["tasks"][0]["runs"].as_array().unwrap().len(), 2);
    let revised = reported(&current).unwrap();
    assert_ne!(revised, report);
    assert_eq!(current["previous_reports"][0], report);
    let revised = m
        .store
        .conversation_work_report(
            m.scope.owner,
            m.scope.user.clone(),
            Uuid::parse_str(&thread).unwrap(),
            Uuid::parse_str(revised).unwrap(),
        )
        .await
        .unwrap();
    let (first_work, sibling) = revised.body.split_once("\n\n2. 초록").unwrap();
    assert!(!first_work.contains("초록 표지 수첩"));
    assert!(sibling.contains("초록 표지 수첩"));

    let history = open(&m, &thread, &report).await.unwrap();
    assert_eq!(history.run_id, first_run);
    assert!(
        history
            .body
            .contains("다시 조사해 현재 결과와 다를 수 있습니다")
    );
    assert!(history.body.contains("초록 표지 수첩"));
}

#[tokio::test]
async fn cancelled_or_failed_work_never_becomes_a_successful_report() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    remember!(m.store, &m.scope.user, "수첩 색상", "합성 파란 표지");
    let state = |s: &'static str| in_state(&m.store, s);
    // Partial: one child cancelled, the other completes; the report names what it leaves out.
    let partial = Uuid::new_v4();
    let two = json!([
        item("취소될 조사", "memory", "수첩"),
        item("남는 조사", "memory", "수첩")
    ]);
    let created = make_reported(&m, partial, two.clone(), true).await.unwrap();
    let cancelled = control(
        &m,
        act(
            &partial.to_string(),
            text(&created["tasks"][0]["id"]),
            1,
            json!({"action": "cancel"}),
        ),
    )
    .await
    .unwrap();
    assert!(reported(&cancelled).is_none() && cancelled["report_requested"] == true);
    assert_eq!(tick(&m).await, 1);
    let done = page(&m).await["groups"][0].clone();
    let thread = m.scope.thread.clone();
    let report = open(&m, &thread, reported(&done).unwrap()).await.unwrap();
    assert!(report.body.contains("2건 중 1건"));
    assert!(report.body.contains("취소될 조사: 취소됨"));
    assert!(report.body.contains("합성 파란 표지"));
    assert!(!report.body.contains("1. 취소될 조사"));
    // Everything cancelled: no report, no claim of one, and no later request succeeds.
    let none = Uuid::new_v4();
    let created = make_reported(&m, none, two, true).await.unwrap();
    let mut last = Value::Null;
    for task in created["tasks"].as_array().unwrap() {
        last = control(
            &m,
            act(
                &none.to_string(),
                text(&task["id"]),
                1,
                json!({"action": "cancel"}),
            ),
        )
        .await
        .unwrap();
    }
    assert!(last.get("report_requested").is_none() && reported(&last).is_none());
    assert_eq!(state("unavailable").await, 1);
    assert_eq!(page(&m).await["groups"][1], last);
    assert!(conflict(ask(&m, &none.to_string(), Uuid::new_v4()).await));
    assert_eq!(tick(&m).await, 0);
    // Work closed by changed authority is failed, so its report is unavailable as well.
    let stale = Uuid::new_v4();
    make_reported(&m, stale, json!([item("권한 전", "memory", "수첩")]), true)
        .await
        .unwrap();
    assert_eq!(
        m.store
            .tick_member_workflows(m.scope.owner, 2)
            .await
            .unwrap(),
        0
    );
    let failed = page(&m).await["groups"][2].clone();
    assert_eq!(states(&failed), ["failed"]);
    assert!(failed.get("report_requested").is_none() && reported(&failed).is_none());
    assert_eq!(state("unavailable").await, 2);
    assert_eq!(state("ready").await, 1);
    assert!(conflict(ask(&m, &stale.to_string(), Uuid::new_v4()).await));
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        1
    );
}

#[tokio::test]
async fn combined_reports_stay_inside_their_member_thread_and_authority() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    remember!(m.store, &m.scope.user, "수첩 색상", "합성 파란 표지");
    let id = Uuid::new_v4();
    let items = json!([item("조사", "memory", "수첩")]);
    // Concurrent identical creates are one group and one report request.
    let (one, two) = tokio::join!(
        make_reported(&m, id, items.clone(), true),
        make_reported(&m, id, items.clone(), true)
    );
    assert_eq!(one.unwrap(), two.unwrap());
    assert_eq!(
        count(
            &m.store,
            "SELECT count(*) FROM member_conversation_group_reports"
        )
        .await,
        1
    );
    assert_eq!(tick(&m).await, 1);
    let group = page(&m).await["groups"][0].clone();
    let report = reported(&group).unwrap().to_owned();
    let thread = m.scope.thread.clone();
    assert!(open(&m, &thread, &report).await.is_some());
    // Another thread, another generation and another member see nothing.
    let other_thread = m
        .store
        .create_thread(&m.scope.user, "다른 대화".into())
        .await
        .unwrap()
        .id;
    assert!(open(&m, &other_thread, &report).await.is_none());
    let mut newer = m.scope.owner;
    newer.generation += 1;
    let (t, r) = (thread.clone(), report.clone());
    assert!(
        m.store
            .call(move |db| group_report(&db.conn, newer, &t, &r))
            .await
            .unwrap()
            .is_none()
    );
    let stranger = member().await;
    assert!(open(&stranger, &thread, &report).await.is_none());
    assert!(matches!(
        ask_in(
            &stranger,
            thread.clone(),
            &id.to_string(),
            &Uuid::new_v4().to_string()
        )
        .await,
        Err(StoreError::NotFound)
    ));
    assert!(
        page(&stranger).await["groups"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    // Revoked authority before the timer: the work fails and no report is built from it.
    let revoked = Uuid::new_v4();
    make_reported(
        &m,
        revoked,
        json!([item("권한 변경 전", "memory", "수첩")]),
        true,
    )
    .await
    .unwrap();
    m.store
        .call(|db| {
            Ok(db
                .conn
                .execute("UPDATE accounts SET auth_epoch=auth_epoch+1", [])?)
        })
        .await
        .unwrap();
    assert_eq!(tick(&m).await, 0);
    assert_eq!(
        count(
            &m.store,
            "SELECT count(*) FROM member_conversation_group_reports WHERE state='ready'"
        )
        .await,
        1
    );
    assert_eq!(
        count(
            &m.store,
            "SELECT count(*) FROM member_conversation_group_reports WHERE state='unavailable'"
        )
        .await,
        1
    );
}

// ------------------------------------------------ departure evidence (dated trips)

const ASK: &str = "지난 출장 출발 시각";

async fn ask_departure(m: &Member, save: bool) -> (Uuid, Value) {
    let id = Uuid::new_v4();
    let created = make_reported(m, id, json!([item(ASK, "memory", "출장")]), save)
        .await
        .unwrap();
    assert_eq!(tick(m).await, 1);
    (id, created)
}
async fn summary_of(m: &Member) -> String {
    text(&page(m).await["groups"][0]["tasks"][0]["summary"]).to_owned()
}

#[tokio::test]
async fn two_dated_trips_offer_both_dates_without_picking_then_selection_shows_that_value() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let user = &m.scope.user;
    remember!(
        m.store,
        user,
        "제주 출장",
        "2026-03-04 출장: 출발 08:30, 도착 10:00"
    );
    remember!(
        m.store,
        user,
        "부산 출장",
        "2026-04-10 출장: 출발 오후 2시 15분, 도착 16:40"
    );
    let (id, created) = ask_departure(&m, false).await;
    let summary = summary_of(&m).await;
    assert!(summary.contains("출발 시각 선택 필요"), "{summary}");
    assert!(
        summary.contains("선택 2026-03-04 · 출발 08:30 · 제주 출장"),
        "{summary}"
    );
    assert!(
        summary.contains("선택 2026-04-10 · 출발 14:15 · 부산 출장"),
        "{summary}"
    );
    assert!(
        !summary.contains("출발 시각:") && !summary.contains("충돌"),
        "{summary}"
    );
    // Reading is pure: no new search, run or record change.
    let runs = count(&m.store, "SELECT count(*) FROM member_work_runs").await;
    for _ in 0..2 {
        assert_eq!(summary_of(&m).await, summary);
    }
    assert_eq!(
        count(&m.store, "SELECT count(*) FROM member_work_runs").await,
        runs
    );
    // Choosing a date is the existing refine action; only that trip's value remains.
    let task = text(&created["tasks"][0]["id"]).to_owned();
    control(
        &m,
        act(
            &id.to_string(),
            &task,
            2,
            json!({"action": "refine", "text": "2026-03-04"}),
        ),
    )
    .await
    .unwrap();
    assert_eq!(tick(&m).await, 1);
    let chosen = summary_of(&m).await;
    assert!(chosen.contains("출발 시각: 08:30 (2026-03-04)"), "{chosen}");
    assert!(
        !chosen.contains("14:15") && !chosen.contains("선택 필요"),
        "{chosen}"
    );
}

#[tokio::test]
async fn one_trip_with_incompatible_departures_is_a_conflict_in_the_result_and_the_combined_report()
{
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let user = &m.scope.user;
    remember!(m.store, user, "출장 일정표", "2026-03-04 출장 출발 08:30");
    let late = remember!(m.store, user, "출장 메모", "2026-03-04 출장 출발 09:15");
    let (_, created) = ask_departure(&m, true).await;
    let summary = summary_of(&m).await;
    assert!(summary.contains("출발 시각 충돌 (2026-03-04)"), "{summary}");
    for part in [
        "08:30",
        "09:15",
        "출장 일정표",
        "출장 메모",
        late.id.as_str(),
    ] {
        assert!(summary.contains(part), "{part} missing: {summary}");
    }
    assert!(
        !summary.contains("출발 시각:") && !summary.contains("선택 필요"),
        "{summary}"
    );
    let group = page(&m).await["groups"][0].clone();
    let report = open(&m, &m.scope.thread, reported(&group).unwrap())
        .await
        .unwrap();
    for part in [
        "출발 시각 충돌",
        "08:30",
        "09:15",
        "출장 일정표",
        "출장 메모",
    ] {
        assert!(
            report.body.contains(part),
            "{part} missing: {}",
            report.body
        );
    }
    assert_eq!(created["tasks"][0]["state"], "queued");
}

#[tokio::test]
async fn arrival_times_and_agreeing_departures_are_not_conflicts() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let user = &m.scope.user;
    // Same trip: departures agree in different notation; arrivals differ; one record is arrival-only.
    remember!(
        m.store,
        user,
        "출장 항공",
        "2026-03-04 출장 08:30 출발, 10:00 도착"
    );
    remember!(
        m.store,
        user,
        "출장 메일",
        "2026-03-04 출장 출발 오전 8시 30분 / 도착 11:20"
    );
    remember!(
        m.store,
        user,
        "출장 숙소",
        "2026-03-04 출장 숙소 도착 21:00"
    );
    ask_departure(&m, false).await;
    let summary = summary_of(&m).await;
    assert!(
        summary.contains("출발 시각: 08:30 (2026-03-04)"),
        "{summary}"
    );
    for arrival in ["10:00", "11:20", "21:00"] {
        assert!(
            !summary.contains(&format!("출발 {arrival}"))
                && !summary.contains(&format!("{arrival} /")),
            "{summary}"
        );
    }
    assert!(
        !summary.contains("충돌") && !summary.contains("선택 필요"),
        "{summary}"
    );
    // A question that does not ask for a departure gets no departure analysis at all.
    let plain_id = Uuid::new_v4();
    make(&m, plain_id, json!([item("출장 조사", "memory", "출장")])).await;
    assert_eq!(tick(&m).await, 1);
    let listed = page(&m).await;
    let plain_summary = text(&listed["groups"][1]["tasks"][0]["summary"]);
    assert!(!plain_summary.contains("출발 시각"), "{plain_summary}");
}

#[tokio::test]
async fn editing_or_forgetting_an_original_never_reveals_its_old_departure() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let m = member().await;
    let user = &m.scope.user;
    let first = remember!(m.store, user, "출장 일정표", "2026-03-04 출장 출발 08:30");
    let second = remember!(m.store, user, "출장 메모", "2026-03-04 출장 출발 09:15");
    let (_, _) = ask_departure(&m, true).await;
    assert!(summary_of(&m).await.contains("출발 시각 충돌"));
    // Edit the first original: its old value is gone from the list and from the combined report.
    m.store
        .correct_memory(
            user,
            first.id.clone(),
            first.revision,
            Uuid::new_v4().to_string(),
            moosuri_core::MemoryDraft {
                subject: "출장 일정표".into(),
                text: "2026-03-04 출장 일정 변경됨".into(),
                speaker: None,
                evidence: vec![],
                valid_from: None,
                valid_until: None,
            },
        )
        .await
        .unwrap();
    let edited = summary_of(&m).await;
    assert!(
        !edited.contains("08:30") && !edited.contains("충돌"),
        "{edited}"
    );
    assert!(edited.contains("일부 원본이 변경되었거나"), "{edited}");
    let group = page(&m).await["groups"][0].clone();
    let report = open(&m, &m.scope.thread, reported(&group).unwrap())
        .await
        .unwrap();
    assert!(!report.body.contains("08:30"), "{}", report.body);
    // Forget the other one too: no departure value remains anywhere on reopen.
    m.store
        .forget_memory(user, second.id, second.revision)
        .await
        .unwrap();
    let listed = serde_json::to_string(&page(&m).await).unwrap();
    assert!(
        !listed.contains("08:30") && !listed.contains("09:15"),
        "{listed}"
    );
    let report = open(&m, &m.scope.thread, reported(&group).unwrap())
        .await
        .unwrap();
    assert!(
        !report.body.contains("08:30") && !report.body.contains("09:15"),
        "{}",
        report.body
    );
}
