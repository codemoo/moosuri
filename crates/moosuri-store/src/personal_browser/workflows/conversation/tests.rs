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
