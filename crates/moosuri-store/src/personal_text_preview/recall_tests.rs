use super::tests::setup;
use super::*;
use crate::tests::TEST_LOCK;
use moosuri_core::{MemoryDocument, MemoryDraft, RunInstructionRequest};

type Fixture = (
    tempfile::TempDir,
    Store,
    UserContext,
    personal_volume::VolumeOwner,
    String,
    ModelPin,
);

fn draft(subject: &str, text: &str) -> MemoryDraft {
    MemoryDraft {
        text: text.into(),
        subject: subject.into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    }
}
async fn save(store: &Store, user: &UserContext, subject: &str, text: &str) -> MemoryDocument {
    store
        .create_memory(user, Uuid::new_v4().to_string(), draft(subject, text))
        .await
        .unwrap()
}
fn pin_of(m: &MemoryDocument) -> RecallMemory {
    RecallMemory {
        id: m.id.clone(),
        revision: m.revision,
    }
}
fn unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
fn terms(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}
fn fresh(admission: Admission) -> Work {
    match admission {
        Admission::Fresh(work) => work,
        _ => panic!("expected a fresh admission"),
    }
}
fn messages(work: &Work) -> Vec<String> {
    let frame: serde_json::Value = serde_json::from_slice(work.request()).unwrap();
    frame["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["text"].as_str().unwrap().to_owned())
        .collect()
}
fn last(work: &Work) -> String {
    messages(work).pop().unwrap()
}
fn settled(work: &Work) -> Vec<u8> {
    let request: serde_json::Value = serde_json::from_slice(work.request()).unwrap();
    serde_json::to_vec(&serde_json::json!({"v":1,"state":"settled","call_id":request["call_id"],"connection_revision":request["connection_revision"],"model":request["model"],"output":"SYNTHETIC_PREVIEW_ANSWER","outcome":"completed","receipt":{"state":"settled","input_tokens":7,"output_tokens":3,"charged_tokens":10}})).unwrap()
}
async fn count(store: &Store, sql: &'static str, run: &str) -> i64 {
    let run = run.to_owned();
    store
        .call(move |db| Ok(db.conn.query_row(sql, [&run], |r| r.get::<_, i64>(0))?))
        .await
        .unwrap()
}
async fn deps(store: &Store, run: &str) -> i64 {
    count(
        store,
        "SELECT count(*) FROM agent_dependencies WHERE run_id=?1",
        run,
    )
    .await
}
async fn begin(
    f: &Fixture,
    request: &str,
    text: &str,
    recall: Vec<RecallMemory>,
) -> Result<Admission> {
    f.1.begin_text_preview_with_recall(
        &f.2,
        f.3,
        f.4.clone(),
        request.into(),
        text.into(),
        f.5.clone(),
        recall,
    )
    .await
}
fn id() -> String {
    Uuid::new_v4().to_string()
}

#[tokio::test]
async fn recall_is_scoped_cleaned_and_never_a_browse() {
    let _g = TEST_LOCK.lock().await;
    let f = setup().await;
    let diet = save(&f.1, &f.2, "Diet", "I am vegetarian and avoid shellfish").await;
    save(&f.1, &f.2, "Garage", "Door code is private").await;
    let got =
        f.1.member_text_recall(&f.2, f.3, terms(&["vegetarian", "a", &"x".repeat(33), " "]))
            .await
            .unwrap();
    assert_eq!(got, vec![pin_of(&diet)]);
    let train = save(&f.1, &f.2, "기차", "기차는 통로 쪽이 편해").await;
    save(&f.1, &f.2, "식사", "매운 음식을 선호해").await;
    for hints in [
        terms(&["좌석", "기차 좌석", "선호"]),
        terms(&["좌석", "창가", "통로"]),
    ] {
        assert_eq!(
            f.1.member_text_recall(&f.2, f.3, hints).await.unwrap(),
            vec![pin_of(&train)]
        );
    }
    for empty in [
        terms(&[]),
        terms(&["취향", "선호도", "기억", "추천"]),
        terms(&["a", "  ", &"y".repeat(40)]),
        terms(&["astronomy"]),
    ] {
        assert!(
            f.1.member_text_recall(&f.2, f.3, empty)
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn neutral_topics_are_search_index_only_and_follow_the_memory_revision() {
    let _guard = TEST_LOCK.lock().await;
    let f = setup().await;
    let request = id();
    let original = draft("기차", "기차는 통로 쪽이 편해");
    let lookup = || {
        f.1.member_text_recall(&f.2, f.3, terms(&["좌석", "창가", "복도"]))
    };
    let store = |req: String, value: MemoryDraft, previous, topic: &str| {
        f.1.write_member_preference(&f.2, f.3, previous, req, value, Some(topic.to_owned()))
    };
    let first = store(request.clone(), original.clone(), None, "좌석 위치")
        .await
        .unwrap();
    store(request.clone(), original.clone(), None, "좌석")
        .await
        .unwrap();
    assert_eq!(lookup().await.unwrap(), vec![pin_of(&first)]);
    assert!(
        f.1.member_text_recall(&f.2, f.3, terms(&["위치"]))
            .await
            .unwrap()
            .is_empty()
    );
    let physical =
        "SELECT count(*) FROM knowledge_fts WHERE kind='preference-topic' AND object_id=?1";
    assert_eq!(count(&f.1, physical, &first.id).await, 1);
    assert!(!first.draft.text.contains("좌석"));
    let literal =
        f.1.search_knowledge(
            &f.2,
            moosuri_core::KnowledgeQuery {
                kind: moosuri_core::KnowledgeKind::Memory,
                text: "좌석".into(),
                match_mode: None,
                collection_id: None,
                subject: None,
                speaker: None,
                valid_at: None,
                observed_from: None,
                observed_until: None,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert!(literal.hits.is_empty());
    f.1.correct_memory(
        &f.2,
        first.id.clone(),
        1,
        id(),
        draft("기차", "기차는 조용한 칸이 편해"),
    )
    .await
    .unwrap();
    store(request, original, None, "좌석").await.unwrap();
    assert!(lookup().await.unwrap().is_empty());
    assert_eq!(count(&f.1, physical, &first.id).await, 0);
    let changed = store(
        id(),
        draft("기차", "기차는 창문 근처가 편해"),
        Some((first.id.clone(), 2)),
        "좌석",
    )
    .await
    .unwrap();
    assert_eq!(lookup().await.unwrap(), vec![pin_of(&changed)]);
    f.1.forget_memory(&f.2, first.id.clone(), changed.revision)
        .await
        .unwrap();
    assert!(lookup().await.unwrap().is_empty());
    assert_eq!(count(&f.1, physical, &first.id).await, 0);
}

#[tokio::test]
async fn expired_topic_matches_cannot_hide_an_eligible_preference() {
    let _guard = TEST_LOCK.lock().await;
    let f = setup().await;
    let write = |value| {
        f.1.write_member_preference(&f.2, f.3, None, id(), value, Some("시간대".into()))
    };
    let valid = write(draft("운동", "아침이 편해")).await.unwrap();
    for i in 0..=recall::PER_TERM_HITS {
        let mut old = draft(&format!("일정 {i}"), "저녁이 편해");
        old.valid_until = Some(unix() - 1);
        write(old).await.unwrap();
    }
    assert_eq!(
        f.1.member_text_recall(&f.2, f.3, terms(&["시간대"]))
            .await
            .unwrap(),
        vec![pin_of(&valid)]
    );
}

#[tokio::test]
async fn recall_filters_expired_forgotten_stale_and_third_party_memories() {
    let _g = TEST_LOCK.lock().await;
    let f = setup().await;
    let now = unix();
    let mut old = draft("Coffee", "Prefers oolong over coffee");
    old.valid_from = Some(now - 200);
    old.valid_until = Some(now - 100);
    f.1.create_memory(&f.2, id(), old).await.unwrap();
    let mut other = draft("Coffee", "Brother likes oolong");
    other.speaker = Some("Brother".into());
    f.1.create_memory(&f.2, id(), other).await.unwrap();
    let gone = save(&f.1, &f.2, "Oolong", "oolong forgotten fact").await;
    f.1.forget_memory(&f.2, gone.id.clone(), gone.revision)
        .await
        .unwrap();
    let mut own = draft("Oolong", "oolong own fact");
    own.speaker = Some("본인".into());
    let own = f.1.create_memory(&f.2, id(), own).await.unwrap();
    let got =
        f.1.member_text_recall(&f.2, f.3, terms(&["oolong"]))
            .await
            .unwrap();
    assert_eq!(got, vec![pin_of(&own)]);
}

#[tokio::test]
async fn recall_returns_at_most_three_strongest_first() {
    let _g = TEST_LOCK.lock().await;
    let f = setup().await;
    for n in 0..5 {
        save(&f.1, &f.2, &format!("Note {n}"), "mentions tea only").await;
    }
    let strong = save(&f.1, &f.2, "Tea", "green tea in the morning").await;
    let got =
        f.1.member_text_recall(&f.2, f.3, terms(&["tea", "green"]))
            .await
            .unwrap();
    assert!(got.len() <= 3);
    assert_eq!(got.first(), Some(&pin_of(&strong)));
}

#[tokio::test]
async fn new_thread_wraps_current_request_and_pins_only_the_sent_memory() {
    let _g = TEST_LOCK.lock().await;
    let f = setup().await;
    let diet = save(&f.1, &f.2, "Diet", "I am vegetarian").await;
    let request = id();
    let work = fresh(
        begin(
            &f,
            &request,
            "Recommend dinner",
            vec![pin_of(&diet), pin_of(&diet)],
        )
        .await
        .unwrap(),
    );
    let all = messages(&work);
    assert_eq!(all.len(), 2);
    let wire: serde_json::Value = serde_json::from_str(&all[1]).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"current_user_request":"Recommend dinner","member_memories":[{"subject":"Diet","text":"I am vegetarian"}]})
    );
    assert!(!all[0].contains("vegetarian"));
    assert_eq!(deps(&f.1, &work.run).await, 1);
    assert_eq!(
        count(
            &f.1,
            "SELECT count(*) FROM chat_runs WHERE id=?1 AND user_text='Recommend dinner'",
            &work.run
        )
        .await,
        1
    );
    let turn = f.1.settle_text_preview(work, None).await;
    assert!(turn.is_ok());
    // Retry is idempotent even if the caller's recall differs.
    assert!(matches!(
        begin(&f, &request, "Recommend dinner", vec![])
            .await
            .unwrap(),
        Admission::Existing(_)
    ));
}

#[tokio::test]
async fn existing_thread_keeps_literal_history_and_wraps_only_the_final_message() {
    let _g = TEST_LOCK.lock().await;
    let f = setup().await;
    let work = fresh(begin(&f, &id(), "Plain question", vec![]).await.unwrap());
    assert_eq!(last(&work), "Plain question");
    assert_eq!(deps(&f.1, &work.run).await, 0);
    let reply = settled(&work);
    f.1.settle_text_preview(work, Some(reply)).await.unwrap();
    // The legacy entry point stays recall-free.
    let legacy = fresh(
        f.1.begin_text_preview(&f.2, f.3, f.4.clone(), id(), "Second".into(), f.5.clone())
            .await
            .unwrap(),
    );
    assert_eq!(last(&legacy), "Second");
    let reply = settled(&legacy);
    f.1.settle_text_preview(legacy, Some(reply)).await.unwrap();
    // A plain memory saved before this feature needs no extra fields.
    let pets = save(&f.1, &f.2, "Pets", "Has a cat named Nabi").await;
    let work = fresh(
        begin(&f, &id(), "Name a toy", vec![pin_of(&pets)])
            .await
            .unwrap(),
    );
    let all = messages(&work);
    assert_eq!(all.len(), 6);
    assert_eq!(all[1], "Plain question");
    assert_eq!(all[3], "Second");
    let wire: serde_json::Value = serde_json::from_str(&all[5]).unwrap();
    assert_eq!(wire["current_user_request"], "Name a toy");
    assert_eq!(wire["member_memories"][0]["subject"], "Pets");
    assert_eq!(deps(&f.1, &work.run).await, 1);
}

#[tokio::test]
async fn stale_corrected_forgotten_or_foreign_pins_are_dropped_without_failing_chat() {
    let _g = TEST_LOCK.lock().await;
    let f = setup().await;
    let a = save(&f.1, &f.2, "Diet", "I am vegetarian").await;
    let stale = pin_of(&a);
    let b =
        f.1.correct_memory(
            &f.2,
            a.id.clone(),
            a.revision,
            id(),
            draft("Diet", "I eat everything"),
        )
        .await
        .unwrap();
    let work = fresh(
        begin(&f, &id(), "Dinner?", vec![stale.clone()])
            .await
            .unwrap(),
    );
    assert_eq!(last(&work), "Dinner?");
    assert_eq!(deps(&f.1, &work.run).await, 0);
    f.1.settle_text_preview(work, None).await.unwrap();
    let fresh_pin = pin_of(&b);
    f.1.forget_memory(&f.2, b.id.clone(), b.revision)
        .await
        .unwrap();
    let work = fresh(
        begin(
            &f,
            &id(),
            "Dinner again?",
            vec![
                fresh_pin,
                RecallMemory {
                    id: "not-an-id".into(),
                    revision: 1,
                },
                RecallMemory {
                    id: id(),
                    revision: 1,
                },
            ],
        )
        .await
        .unwrap(),
    );
    assert_eq!(last(&work), "Dinner again?");
    assert_eq!(deps(&f.1, &work.run).await, 0);
}

#[tokio::test]
async fn member_isolation_rejects_foreign_actor_and_foreign_ids() {
    let _g = TEST_LOCK.lock().await;
    let f = setup().await;
    let g = setup().await;
    let mine = save(&f.1, &f.2, "Diet", "I am vegetarian").await;
    assert!(
        g.1.member_text_recall(&f.2, g.3, terms(&["vegetarian"]))
            .await
            .is_err()
    );
    assert!(
        g.1.member_text_recall(&g.2, g.3, terms(&["vegetarian"]))
            .await
            .unwrap()
            .is_empty()
    );
    let work = fresh(
        begin(&g, &id(), "Dinner?", vec![pin_of(&mine)])
            .await
            .unwrap(),
    );
    assert_eq!(last(&work), "Dinner?");
}

#[tokio::test]
async fn relevant_memory_fits_by_trimming_older_history_and_expiry_blocks_late_output() {
    let _guard = TEST_LOCK.lock().await;
    let mut fixture = setup().await;
    let stored = save(&fixture.1, &fixture.2, "Meeting", "Prefer mornings").await;
    let old = fresh(
        begin(&fixture, &id(), &"long earlier text ".repeat(180), vec![])
            .await
            .unwrap(),
    );
    let reply = settled(&old);
    fixture
        .1
        .settle_text_preview(old, Some(reply))
        .await
        .unwrap();
    let question = "Choose a meeting time";
    fixture.5.reservation_tokens = Some(crate::runtime_model_ledger::Budget::text_reservation(
        POLICY.len() + RECALL_POLICY.len() + question.len() + 320,
        2,
        fixture.5.max_output_tokens,
    ));
    let current = fresh(
        begin(&fixture, &id(), question, vec![pin_of(&stored)])
            .await
            .unwrap(),
    );
    assert_eq!(messages(&current).len(), 2);
    assert!(last(&current).contains("Prefer mornings"));
    // Advance the synthetic source's validity boundary while this reply is in flight.
    fixture.1.call(move |db| {
        db.conn.execute("UPDATE knowledge_memory_versions SET draft=json_set(draft,'$.valid_until',unixepoch()-1)
            WHERE memory_id=?1 AND revision=?2", params![stored.id, stored.revision])?;
        Ok(())
    }).await.unwrap();
    let reply = settled(&current);
    assert!(matches!(
        fixture.1.settle_text_preview(current, Some(reply)).await,
        Err(StoreError::Conflict)
    ));
}

#[tokio::test]
async fn too_small_reservation_drops_recall_but_keeps_ordinary_chat() {
    let _g = TEST_LOCK.lock().await;
    let mut f = setup().await;
    let diet = save(&f.1, &f.2, "Diet", &"vegetarian ".repeat(80)).await;
    let question = "Dinner?";
    f.5.reservation_tokens = Some(
        crate::runtime_model_ledger::Budget::text_reservation(
            POLICY.len() + question.len(),
            2,
            f.5.max_output_tokens,
        ) + 4,
    );
    let work = fresh(
        begin(&f, &id(), question, vec![pin_of(&diet)])
            .await
            .unwrap(),
    );
    assert_eq!(last(&work), question);
    assert_eq!(deps(&f.1, &work.run).await, 0);
    assert!(work.request().len() < 64 * 1024);
}

#[tokio::test]
async fn same_thread_history_inherits_dependencies_and_forgetting_blocks_publication() {
    let _g = TEST_LOCK.lock().await;
    let f = setup().await;
    let diet = save(&f.1, &f.2, "Diet", "I am vegetarian").await;
    let first = fresh(
        begin(&f, &id(), "Recommend dinner", vec![pin_of(&diet)])
            .await
            .unwrap(),
    );
    let reply = settled(&first);
    f.1.settle_text_preview(first, Some(reply)).await.unwrap();
    // The next ordinary turn carries the earlier recall-derived answer.
    let second = fresh(begin(&f, &id(), "And dessert?", vec![]).await.unwrap());
    let all = messages(&second);
    assert_eq!(all[1], "Recommend dinner");
    assert_eq!(all[2], "SYNTHETIC_PREVIEW_ANSWER");
    assert_eq!(last(&second), "And dessert?");
    assert_eq!(deps(&f.1, &second.run).await, 1);
    let late = settled(&second);
    f.1.forget_memory(&f.2, diet.id.clone(), diet.revision)
        .await
        .unwrap();
    assert!(matches!(
        f.1.settle_text_preview(second, Some(late)).await,
        Err(StoreError::Conflict)
    ));
}

#[tokio::test]
async fn saved_follow_up_inherits_recall_and_correction_purges_the_inflight_reply() {
    let _g = TEST_LOCK.lock().await;
    let f = setup().await;
    let diet = save(&f.1, &f.2, "Diet", "I am vegetarian").await;
    let first = fresh(
        begin(&f, &id(), "Recommend dinner", vec![pin_of(&diet)])
            .await
            .unwrap(),
    );
    let follow = RunInstructionRequest {
        request_id: id(),
        expected_attempt: 1,
        text: "Make it spicy".into(),
    };
    f.1.save_text_follow_up(&f.2, f.3, f.4.clone(), first.run.clone(), follow.clone())
        .await
        .unwrap();
    let reply = settled(&first);
    f.1.settle_text_preview(first, Some(reply)).await.unwrap();
    let second = fresh(
        begin(&f, &follow.request_id, &follow.text, vec![])
            .await
            .unwrap(),
    );
    let all = messages(&second);
    assert_eq!(all[1], "Recommend dinner");
    assert_eq!(last(&second), "Make it spicy");
    assert_eq!(deps(&f.1, &second.run).await, 1);
    let late = settled(&second);
    f.1.correct_memory(
        &f.2,
        diet.id.clone(),
        diet.revision,
        id(),
        draft("Diet", "Now vegan"),
    )
    .await
    .unwrap();
    assert!(matches!(
        f.1.settle_text_preview(second, Some(late)).await,
        Err(StoreError::Conflict)
    ));
}
