use super::*;
use serde::Deserialize;
use serde_json::json;
use std::{collections::BTreeMap, time::Instant};

#[derive(Deserialize)]
struct World {
    threads: [String; 2],
    turns: usize,
    padding_user: String,
    assistant: String,
    messages: BTreeMap<usize, String>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    question: String,
    query: String,
    turn: usize,
}

#[tokio::test]
#[ignore = "Run python3 audit/memory-reader/run.py for the matched live-reader evaluation"]
async fn export_namesake_reader_inputs() {
    let _lock = TEST_LOCK.lock().await;
    let world: World = serde_json::from_str(include_str!(
        "../../../../../audit/memory-reader/world.json"
    ))
    .unwrap();
    let selected: Vec<usize> =
        serde_json::from_str(&std::env::var("MOOSURI_SYNTHETIC_TURN_NUMBERS").unwrap()).unwrap();
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let a = store
        .create_thread(&user, world.threads[0].clone())
        .await
        .unwrap();
    let b = store
        .create_thread(&user, world.threads[1].clone())
        .await
        .unwrap();
    let mut turn_numbers = BTreeMap::new();
    let mut memory_write_us = 0;
    for n in 1..=world.turns {
        let thread = if n % 2 == 0 { &a } else { &b };
        let message = world.messages.get(&n).unwrap_or(&world.padding_user);
        let turn = completed(&store, &user, &thread.id, message, &world.assistant).await;
        turn_numbers.insert(turn.run.id, n);
        if selected.contains(&n) {
            let start = Instant::now();
            store
                .create_memory(
                    &user,
                    Uuid::new_v4().to_string(),
                    MemoryDraft {
                        subject: thread.title.clone(),
                        text: message.clone(),
                        speaker: None,
                        valid_from: None,
                        valid_until: None,
                        evidence: vec![],
                    },
                )
                .await
                .unwrap();
            memory_write_us += start.elapsed().as_micros();
        }
    }
    let mut rows = Vec::new();
    for case in world.cases {
        let ask = store.create_thread(&user, "새 대화".into()).await.unwrap();
        let question = queued(&store, &user, &ask.id, &case.question).await;
        let tenant = user.tenant.0.clone();
        let needle = case.query.clone();
        let start = Instant::now();
        let hits = store
            .call(move |db| {
                let r = chat::load_run(&db.conn, &tenant, &question.run.id)?;
                let results = search(&db.conn, &r, &needle, ConversationScope::OwnedConversations)?;
                results
                    .hits
                    .iter()
                    .map(|h| read(&db.conn, &r, &h.turn_id, h.revision))
                    .collect::<Result<Vec<_>>>()
            })
            .await
            .unwrap();
        let search_us = start.elapsed().as_micros();
        let rank = hits
            .iter()
            .position(|t| turn_numbers[&t.turn_id] == case.turn)
            .map(|i| i + 1);
        let raw: Vec<_> = hits
            .into_iter()
            .map(|t| {
                t.messages
                    .into_iter()
                    .map(|m| m.text)
                    .collect::<Vec<_>>()
                    .join(" / ")
            })
            .collect();
        let start = Instant::now();
        let result = store
            .search_knowledge(
                &user,
                KnowledgeQuery {
                    kind: KnowledgeKind::Memory,
                    text: case.query,
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
        let structured: Vec<_> = result.hits.into_iter().take(5).map(|h| h.snippet).collect();
        rows.push(json!({"id":case.id, "raw":raw, "rank":rank, "structured":structured, "raw_us":search_us, "structured_us":start.elapsed().as_micros()}));
    }
    println!(
        "SYNTHETIC_READER={}",
        json!({"rows":rows,"write_us":memory_write_us})
    );
}
