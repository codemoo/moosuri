//! The scripted model uses only current user literals and the actual sealed target/read.
use super::*;
const CREATE: &str = "'여행 취소 계산' 문서로 '예상 환불액 95000원'을 저장해 줘.";
const RENAME: &str = "방금 만든 문서 제목을 '가을 여행 환불 계산'으로 바꿔 줘.";
const APPEND: &str = "그 문서에 '실제 예약 취소는 아직 하지 않았음'이라는 문장도 추가해 줘.";
pub(super) fn model_calls(input: &Value, prior: &[Value]) -> Vec<Value> {
    let current = input["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .unwrap()["content"]
        .as_str()
        .unwrap();
    let quoted = current.split('\'').collect::<Vec<_>>();
    let system = input["messages"][0]["content"].as_str().unwrap();
    let target = system.split("Current request target: id=").nth(1).map(|p| {
        let mut pieces = p.split_whitespace();
        let id = pieces.next().unwrap();
        let revision = pieces
            .next()
            .unwrap()
            .strip_prefix("revision=")
            .unwrap()
            .trim_end_matches('.')
            .parse::<i64>()
            .unwrap();
        (id, revision)
    });
    if let Some((id, revision)) = target {
        if let Some(read) = prior.iter().find(|p| p["value"]["body"].is_string()) {
            let body = read["value"]["body"].as_str().unwrap();
            let (title, body_new) = if current.contains("제목을") {
                (quoted[1], body.to_owned())
            } else {
                (
                    read["value"]["artifact"]["title"].as_str().unwrap(),
                    format!("{body}\n{}", quoted[quoted.len() - 2]),
                )
            };
            return vec![call(
                0,
                "artifact_write",
                json!({"title":title,"body":body_new,"payload":{"kind":"document"},"anchor":body}),
            )];
        }
        return vec![call(
            0,
            "artifact_read",
            json!({"id":id,"revision":revision,"start_byte":0,"max_bytes":8192}),
        )];
    }
    vec![call(
        0,
        "artifact_write",
        json!({"title":quoted[1],"body":quoted[3],"payload":{"kind":"document"},"anchor":null}),
    )]
}
async fn say(store: &Store, u: &UserContext, t: &str, text: &str, runtime: &Runtime) -> ChatTurn {
    store
        .submit_turn(u, t.into(), uuid::Uuid::new_v4().to_string(), text.into())
        .await
        .unwrap();
    runtime.wake();
    wait(store, u, t).await
}
#[tokio::test]
async fn quoted_title_create_then_recent_receipt_edits_reopen_and_replay_one_document() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let s = server().await;
    s.state.mode.store(9, Ordering::SeqCst);
    let (dir, store, u, t) = setup(&s.base).await;
    let runtime = Runtime::start(store.clone());
    let created = say(&store, &u, &t, CREATE, &runtime).await;
    assert_eq!(
        created.run.state,
        RunState::Completed,
        "{:?}",
        created.run.error
    );
    let receipt = created.run.artifact_receipts.unwrap().remove(0);
    assert_eq!(s.state.calls.load(Ordering::SeqCst), 2);
    runtime.shutdown().await;
    drop(runtime);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let runtime = Runtime::start(store.clone());
    let renamed = say(&store, &u, &t, RENAME, &runtime).await;
    assert_eq!(
        renamed.run.state,
        RunState::Completed,
        "{:?}",
        renamed.run.error
    );
    assert_eq!(
        renamed.run.artifact_receipts.as_ref().unwrap()[0].id,
        receipt.id
    );
    let append_id = uuid::Uuid::new_v4().to_string();
    store
        .submit_turn(&u, t.clone(), append_id.clone(), APPEND.into())
        .await
        .unwrap();
    runtime.wake();
    let appended = wait(&store, &u, &t).await;
    assert_eq!(
        appended.run.state,
        RunState::Completed,
        "{:?}",
        appended.run.error
    );
    let before = s.state.calls.load(Ordering::SeqCst);
    let replay = store
        .submit_turn(&u, t.clone(), append_id, APPEND.into())
        .await
        .unwrap();
    assert_eq!(replay.run.id, appended.run.id);
    assert_eq!(s.state.calls.load(Ordering::SeqCst), before);
    let version = store.artifact(&u, receipt.id.clone(), None).await.unwrap();
    assert_eq!(version.artifact.revision, 3);
    assert_eq!(version.artifact.title, "가을 여행 환불 계산");
    assert_eq!(
        version.draft.unwrap().body,
        "예상 환불액 95000원\n실제 예약 취소는 아직 하지 않았음"
    );
    assert_eq!(
        store.artifacts(&u, None, None).await.unwrap().items.len(),
        1
    );
    assert_eq!(appended.run.artifact_receipts.unwrap()[0].id, receipt.id);
    runtime.shutdown().await;
}
#[tokio::test]
async fn previous_document_identity_does_not_cross_threads_stale_edits_or_named_ambiguity() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    for mode in [
        "other_thread",
        "editor_changed",
        "named_ambiguity",
        "other_thread_named",
    ] {
        let s = server().await;
        s.state.mode.store(9, Ordering::SeqCst);
        let (_dir, store, u, t) = setup(&s.base).await;
        let runtime = Runtime::start(store.clone());
        let created = say(&store, &u, &t, CREATE, &runtime).await;
        let id = created.run.artifact_receipts.unwrap()[0].id.clone();
        let mut thread = t;
        if mode == "other_thread" || mode == "other_thread_named" {
            thread = store
                .create_thread(&u, "new conversation".into())
                .await
                .unwrap()
                .id;
        }
        if mode == "editor_changed" || mode == "named_ambiguity" || mode == "other_thread_named" {
            store
                .write_artifact(
                    &u,
                    ArtifactWrite {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        id: (mode == "editor_changed").then(|| id.clone()),
                        expected_revision: (mode == "editor_changed").then_some(1),
                        draft: ArtifactDraft {
                            title: if mode == "editor_changed" {
                                "사용자 편집본".into()
                            } else {
                                "그 문서".into()
                            },
                            body: "Owner editor text must survive".into(),
                            payload: ArtifactPayload::Document,
                            references: vec![],
                        },
                    },
                )
                .await
                .unwrap();
        }
        let before = serde_json::to_value(store.artifacts(&u, None, None).await.unwrap()).unwrap();
        let done = say(&store, &u, &thread, APPEND, &runtime).await;
        assert!(done.run.artifact_receipts.is_none(), "{mode}");
        assert_eq!(
            serde_json::to_value(store.artifacts(&u, None, None).await.unwrap()).unwrap(),
            before,
            "{mode}"
        );
        let saved = store.artifact(&u, id, None).await.unwrap();
        assert_eq!(
            saved.draft.unwrap().body,
            if mode == "editor_changed" {
                "Owner editor text must survive"
            } else {
                "예상 환불액 95000원"
            },
            "{mode}"
        );
        if mode == "other_thread_named" {
            let named = store
                .artifacts(&u, None, None)
                .await
                .unwrap()
                .items
                .into_iter()
                .find(|a| a.title == "그 문서")
                .unwrap();
            let done = say(
                &store,
                &u,
                &thread,
                "'그 문서'라는 문서에 '명시한 제목의 문서만 수정'이라는 문장도 추가해 줘.",
                &runtime,
            )
            .await;
            assert_eq!(done.run.artifact_receipts.unwrap()[0].id, named.id);
            let saved = store.artifact(&u, named.id, None).await.unwrap();
            assert_eq!(saved.artifact.revision, 2);
            assert_eq!(
                saved.draft.unwrap().body,
                "Owner editor text must survive\n명시한 제목의 문서만 수정"
            );
        }
        runtime.shutdown().await;
    }
}
