//! Preference helpers against a real member volume (SQLite store, real authentication).
//! No broker is involved: the typed decision is exactly what the interpreter would pass in.
use super::*;
use moosuri_core::{EvidenceRef, IngestBatch, KnowledgeKind, KnowledgeQuery, SourceInput};
use std::{fs, os::unix::fs::PermissionsExt};

const EPOCH: i64 = 3;

struct Member {
    store: Store,
    owner: VolumeOwner,
    token: String,
}
impl Member {
    async fn new(root: &std::path::Path, email: &str) -> Self {
        fs::create_dir(root).unwrap();
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = VolumeOwner {
            account: uuid::Uuid::new_v4(),
            tenant: uuid::Uuid::new_v4(),
            volume: uuid::Uuid::new_v4(),
            generation: 1,
        };
        owner
            .initialize(
                root,
                "Synthetic member".into(),
                email.into(),
                "Synthetic-preference-42".into(),
            )
            .await
            .unwrap();
        let store = owner.open_verified(root).await.unwrap();
        let initial = store
            .login(email.into(), "Synthetic-preference-42".into())
            .await
            .unwrap();
        let login = store
            .change_password(&initial.user, "Changed-preference-member-42".into())
            .await
            .unwrap();
        Self {
            store,
            owner,
            token: login.token,
        }
    }
    /// Re-authenticated on every use, as the interpreter does after any model call.
    async fn user(&self) -> UserContext {
        self.store.authenticate(self.token.clone()).await.unwrap()
    }
    async fn seed(&self, subject: &str, text: &str) -> MemoryDocument {
        self.store
            .create_memory(
                &self.user().await,
                uuid::Uuid::new_v4().to_string(),
                MemoryDraft {
                    text: text.into(),
                    subject: subject.into(),
                    speaker: None,
                    valid_from: None,
                    valid_until: None,
                    evidence: vec![],
                },
            )
            .await
            .unwrap()
    }
    async fn plan(&self, text: &str, decision: &Decision) -> Plan {
        resolve(
            &self.store,
            self.owner,
            &self.user().await,
            EPOCH,
            &uuid::Uuid::new_v4().to_string(),
            text,
            decision,
        )
        .await
        .unwrap()
    }
    async fn all(&self) -> Vec<MemoryDocument> {
        live_memories(&self.store, &self.user().await)
            .await
            .unwrap()
    }
}
fn say(operation: Operation, subject: Option<&str>, value: Option<&str>) -> Decision {
    Decision {
        subject: subject.map(Into::into),
        value: value.map(Into::into),
        ..Decision::new(operation)
    }
}
async fn run(member: &Member, plan: &Plan) -> Outcome {
    apply(
        &member.store,
        member.owner,
        &member.user().await,
        EPOCH,
        plan,
    )
    .await
    .unwrap()
}
async fn show(member: &Member, outcome: &Outcome) -> View {
    project(&member.store, member.owner, &member.user().await, outcome)
        .await
        .unwrap()
}
/// The journal stores plans as JSON; a resumed plan must behave the same.
fn journaled(plan: &Plan) -> Plan {
    serde_json::from_str(&serde_json::to_string(plan).unwrap()).unwrap()
}

#[tokio::test]
async fn a_stated_preference_uses_the_members_complete_words_and_normal_memory_controls() {
    let temp = tempfile::tempdir().unwrap();
    let a = Member::new(
        &temp.path().canonicalize().unwrap().join("a"),
        "a@example.test",
    )
    .await;
    let mut decision = say(Operation::Create, Some("기차"), Some("invented value"));
    decision.lasting = Lasting::Stated;
    let statement = "기차는 통로 쪽이 편해";
    let plan = a.plan(statement, &decision).await;
    let first = run(&a, &plan).await;
    assert_eq!(run(&a, &journaled(&plan)).await, first);
    let memories = a.all().await;
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].draft.text, statement);
    for text in [
        "이번엔 기차는 창가 쪽이 편해",
        "기차는 통로가 편해?",
        "친구는 기차 통로가 편해",
        "기차는 ‘통로가 편해’라고 했어",
        "만약 기차는 통로가 편해",
        "기차는 통로가 편해. 저장하지 마",
    ] {
        assert!(
            matches!(a.plan(text, &decision).await, Plan::NoChange(_)),
            "{text}"
        );
    }
    assert!(stated_preference(
        "저는 보통 회의는 오전을 선호합니다.",
        "회의"
    ));
    assert!(stated_preference(
        "저는 이제 기차는 창가 쪽이 편해요",
        "기차"
    ));
    assert!(stated_preference(
        "난 커피는 따뜻하게 마시는 게 좋아",
        "커피"
    ));
    for (words, topic) in [
        ("커피는 동생에게 좋아", "커피"),
        ("커피는 어떤 걸 고르는 게 좋아", "커피"),
        ("회의는 늦으면 먼저 시작하는 게 좋아", "회의"),
        ("오늘만 커피는 차갑게 마시는 게 좋아", "커피"),
        ("친구는 커피는 차갑게 마시는 게 좋아", "커피"),
        ("난 커피는 차갑게 마시는 게 좋아?", "커피"),
        ("엄마는 통로석이 편해", "엄마"),
        ("남편은 창가 쪽이 편해", "남편"),
        ("아이는 매운 음식을 좋아해", "아이"),
        ("요즘은 기차 통로 쪽이 편해", "요즘"),
        ("기차는 내일만 통로가 편해", "기차"),
    ] {
        assert!(!stated_preference(words, topic), "{words}");
    }
    decision.operation = Some(Operation::Forget);
    assert!(matches!(
        a.plan(statement, &decision).await,
        Plan::NoChange(_)
    ));
    assert_eq!(a.all().await.len(), 1);
}

#[tokio::test]
async fn create_retry_and_correction_use_one_memory_and_show_only_current_text() {
    let temp = tempfile::tempdir().unwrap();
    let a = Member::new(
        &temp.path().canonicalize().unwrap().join("a"),
        "a@example.test",
    )
    .await;

    let plan = a
        .plan(
            "식사 취향: 해산물 알레르기가 있어요. 기억해 줘",
            &say(
                Operation::Create,
                Some("식사 취향"),
                Some("해산물 알레르기가 있어요"),
            ),
        )
        .await;
    assert!(matches!(plan, Plan::Create { .. }), "{plan:?}");
    let plan = journaled(&plan);
    let created = run(&a, &plan).await;
    // A crash-and-retry applies the same plan again: same receipt, still one memory.
    assert_eq!(run(&a, &plan).await, created);
    let all = a.all().await;
    assert_eq!(all.len(), 1);
    assert_eq!(
        (all[0].revision, all[0].draft.text.as_str()),
        (1, "해산물 알레르기가 있어요")
    );

    // A duplicate name is never a second memory; the member must ask for a correction.
    let again = a
        .plan(
            "식사 취향: 견과류 알레르기. 기억해 줘",
            &say(
                Operation::Create,
                Some("식사 취향"),
                Some("견과류 알레르기"),
            ),
        )
        .await;
    assert!(
        matches!(again, Plan::NoChange(NoChange::Exists { .. })),
        "{again:?}"
    );

    let correction = a
        .plan(
            "식사 취향을 '갑각류만 안 된다'로 바꿔 줘",
            &say(Operation::Edit, Some("식사 취향"), Some("갑각류만 안 된다")),
        )
        .await;
    let Plan::Edit { memory, .. } = &correction else {
        panic!("{correction:?}")
    };
    assert_eq!(memory.revision, 1);
    let edited = run(&a, &journaled(&correction)).await;
    assert_eq!(run(&a, &correction).await, edited);
    assert_eq!(
        edited,
        Outcome::Applied {
            operation: Operation::Edit,
            memory: MemoryRef {
                id: memory.id.clone(),
                revision: 2
            }
        }
    );

    // Replaying the *creation* now shows the live text; the stored outcomes carry no text.
    let View::Remembered {
        text,
        superseded,
        current_revision,
        ..
    } = show(&a, &created).await
    else {
        panic!("expected a live memory")
    };
    assert_eq!(
        (text.as_str(), superseded, current_revision),
        ("갑각류만 안 된다", true, 2)
    );
    let stored = serde_json::to_string(&(&created, &edited)).unwrap();
    assert!(
        !stored.contains("해산물") && !stored.contains("갑각류"),
        "{stored}"
    );
    let shown = serde_json::to_string(&show(&a, &created).await).unwrap();
    assert!(!shown.contains("해산물"), "{shown}");
    assert_eq!(a.all().await.len(), 1);
}

#[tokio::test]
async fn single_trip_negated_quoted_reported_and_invented_requests_change_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let a = Member::new(
        &temp.path().canonicalize().unwrap().join("a"),
        "a@example.test",
    )
    .await;
    let seeded = a.seed("식사 취향", "해산물 알레르기가 있어요").await;
    let edit = |value: &str| say(Operation::Edit, Some("식사 취향"), Some(value));

    let cases: Vec<(&str, Decision, NoChange)> = vec![
        // Single-trip exception: not a persistent edit.
        (
            "이번 여행만 식사 취향을 '해산물 빼기'로 바꿔 줘",
            edit("해산물 빼기"),
            NoChange::OneTime,
        ),
        (
            "식사 취향을 '갑각류만 안 된다'로 바꿔 줘",
            Decision {
                lasting: Lasting::OneTime,
                ..edit("갑각류만 안 된다")
            },
            NoChange::OneTime,
        ),
        (
            "식사 취향을 '갑각류만 안 된다'로 바꾸지 마",
            edit("갑각류만 안 된다"),
            NoChange::Negated,
        ),
        (
            "\"식사 취향을 갑각류만 안 된다로 바꿔 줘\"",
            edit("갑각류만 안 된다"),
            NoChange::NotARequest,
        ),
        (
            "친구가 식사 취향을 갑각류만 안 된다로 바꿔 달라고 말했어",
            edit("갑각류만 안 된다"),
            NoChange::NotARequest,
        ),
        (
            "식사 취향을 갑각류만 안 된다로 바꾸면 어떻게 돼?",
            edit("갑각류만 안 된다"),
            NoChange::NotARequest,
        ),
        // The model may not invent the value.
        (
            "식사 취향을 바꿔 줘",
            edit("땅콩 알레르기"),
            NoChange::NeedsLiteral,
        ),
        (
            "식사 취향 기억을 지우지 마",
            say(Operation::Forget, Some("식사 취향"), None),
            NoChange::Negated,
        ),
    ];
    for (text, decision, expected) in cases {
        assert_eq!(
            a.plan(text, &decision).await,
            Plan::NoChange(expected),
            "{text}"
        );
    }
    let all = a.all().await;
    assert_eq!(all.len(), 1);
    assert_eq!(
        (
            all[0].id.as_str(),
            all[0].revision,
            all[0].draft.text.as_str()
        ),
        (seeded.id.as_str(), 1, "해산물 알레르기가 있어요")
    );

    // A stored preference may itself contain a negation; it is data, not the request's framing.
    let plan = a
        .plan(
            "연락 방식: 전화하지 마. 기억해 줘",
            &say(Operation::Create, Some("연락 방식"), Some("전화하지 마")),
        )
        .await;
    assert!(matches!(plan, Plan::Create { .. }), "{plan:?}");
    // ...and "잊지 말고 기억해" is a save, not a negated one.
    let plan = a
        .plan(
            "연락 방식: 문자 선호, 잊지 말고 기억해 줘",
            &say(Operation::Create, Some("연락 방식"), Some("문자 선호")),
        )
        .await;
    assert!(matches!(plan, Plan::Create { .. }), "{plan:?}");
}

#[tokio::test]
async fn forget_withdraws_the_memory_its_text_and_local_evidence_and_retries_cleanly() {
    let temp = tempfile::tempdir().unwrap();
    let a = Member::new(
        &temp.path().canonicalize().unwrap().join("a"),
        "a@example.test",
    )
    .await;
    let user = a.user().await;
    let group = a
        .store
        .create_collection(&user, "Synthetic".into(), uuid::Uuid::new_v4().to_string())
        .await
        .unwrap();
    let ingested = a
        .store
        .ingest_text(
            &user,
            group.id.clone(),
            IngestBatch {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: "one".into(),
                    version: "v1".into(),
                    expected_revision: None,
                    title: "Synthetic source".into(),
                    text: "원문 해산물 알레르기".into(),
                    speaker: None,
                    observed_at: 100,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap();
    let source = ingested.source_ids[0].clone();
    // A source-derived memory: its evidence is local and must go with the memory.
    let memory = a
        .store
        .create_memory(
            &user,
            uuid::Uuid::new_v4().to_string(),
            MemoryDraft {
                text: "해산물 알레르기".into(),
                subject: "식사 취향".into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![EvidenceRef {
                    source_id: source.clone(),
                    source_revision: 1,
                    start_byte: 0,
                    end_byte: 6,
                    quote: "원문".into(),
                }],
            },
        )
        .await
        .unwrap();
    let search = || KnowledgeQuery {
        kind: KnowledgeKind::Memory,
        text: "해산물".into(),
        match_mode: None,
        collection_id: None,
        subject: None,
        speaker: None,
        valid_at: None,
        observed_from: None,
        observed_until: None,
        cursor: None,
    };
    assert_eq!(
        a.store
            .search_knowledge(&user, search())
            .await
            .unwrap()
            .hits
            .len(),
        1
    );

    let plan = a
        .plan(
            "식사 취향 기억 지워 줘",
            &say(Operation::Forget, Some("식사 취향"), None),
        )
        .await;
    assert!(
        matches!(&plan, Plan::Forget { memory: m, .. } if m.id == memory.id && m.revision == 1),
        "{plan:?}"
    );
    let plan = journaled(&plan);
    let done = run(&a, &plan).await;
    assert_eq!(
        done,
        Outcome::Applied {
            operation: Operation::Forget,
            memory: MemoryRef {
                id: memory.id.clone(),
                revision: 2
            }
        }
    );
    assert_eq!(run(&a, &plan).await, done, "a retry is idempotent");

    let user = a.user().await;
    assert!(matches!(
        a.store.memory(&user, memory.id.clone(), None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        a.store.memory(&user, memory.id.clone(), Some(1)).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        a.store.memory_history(&user, memory.id.clone(), None).await,
        Err(StoreError::NotFound)
    ));
    assert!(a.all().await.is_empty());
    assert!(
        a.store
            .search_knowledge(&user, search())
            .await
            .unwrap()
            .hits
            .is_empty()
    );
    // Only the memory's own derived evidence was withdrawn; the member's source remains.
    assert!(a.store.source(&user, source, None).await.is_ok());

    // Every replay surface hides the old text, including an earlier creation receipt.
    let earlier = Outcome::Applied {
        operation: Operation::Create,
        memory: MemoryRef {
            id: memory.id.clone(),
            revision: 1,
        },
    };
    for outcome in [&done, &earlier] {
        let view = show(&a, outcome).await;
        assert_eq!(
            view,
            View::Withdrawn {
                memory_id: memory.id.clone()
            }
        );
        assert!(!serde_json::to_string(&view).unwrap().contains("해산물"));
    }
    // The name now matches nothing, so repeating the sentence mutates nothing.
    let repeat = a
        .plan(
            "식사 취향 기억 지워 줘",
            &say(Operation::Forget, Some("식사 취향"), None),
        )
        .await;
    assert_eq!(repeat, Plan::NoChange(NoChange::NoMatch));
}

#[tokio::test]
async fn ambiguous_or_partial_names_never_pick_a_memory() {
    let temp = tempfile::tempdir().unwrap();
    let a = Member::new(
        &temp.path().canonicalize().unwrap().join("a"),
        "a@example.test",
    )
    .await;
    let stay = a.seed("여행 숙소", "호텔보다 한옥").await;
    a.seed("여행 교통", "기차 선호").await;
    let forget = |name: &str| say(Operation::Forget, Some(name), None);

    // Partial name: two candidates, nothing chosen.
    let partial = a.plan("여행 기억 지워 줘", &forget("여행")).await;
    let Plan::NoChange(NoChange::Ambiguous { candidates }) = &partial else {
        panic!("{partial:?}")
    };
    assert_eq!(candidates.len(), 2);
    let View::NoChange { reason, candidates } = show(
        &a,
        &Outcome::NoChange(NoChange::Ambiguous {
            candidates: candidates.clone(),
        }),
    )
    .await
    else {
        panic!("expected a no-change view")
    };
    assert_eq!(reason, "ambiguous");
    assert_eq!(candidates.len(), 2);
    assert!(candidates.iter().any(|c| c.subject == "여행 숙소"));
    // A unique exact name is fine, but a duplicate exact name is ambiguous again.
    assert!(matches!(
        a.plan("여행 숙소 기억 지워 줘", &forget("여행 숙소")).await,
        Plan::Forget { .. }
    ));
    a.seed("여행 숙소", "다른 항목").await;
    assert!(matches!(
        a.plan("여행 숙소 기억 지워 줘", &forget("여행 숙소")).await,
        Plan::NoChange(NoChange::Ambiguous { candidates }) if candidates.len() == 2
    ));
    assert_eq!(
        a.plan("취미 기억 지워 줘", &forget("취미")).await,
        Plan::NoChange(NoChange::NoMatch)
    );
    // An exact pin to an outdated revision is stale, not retargeted.
    let outdated = Decision::new(Operation::Forget).with_memory(MemoryRef {
        id: stay.id.clone(),
        revision: 9,
    });
    assert_eq!(
        a.plan("그 기억 지워 줘", &outdated).await,
        Plan::NoChange(NoChange::Stale)
    );
    let all = a.all().await;
    assert_eq!(all.len(), 3);
    assert!(all.iter().all(|d| d.revision == 1));
}

#[tokio::test]
async fn foreign_members_stale_pins_and_other_sessions_cannot_apply() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let a = Member::new(&root.join("a"), "a@example.test").await;
    let b = Member::new(&root.join("b"), "b@example.test").await;
    let mine = a.seed("식사 취향", "해산물 알레르기가 있어요").await;
    let by_id = Decision::new(Operation::Forget).with_memory(MemoryRef {
        id: mine.id.clone(),
        revision: 1,
    });

    // Another member's volume has no such object, and A's authority rejects B's session.
    assert_eq!(
        b.plan("그 기억 지워 줘", &by_id).await,
        Plan::NoChange(NoChange::NoMatch)
    );
    let request = uuid::Uuid::new_v4().to_string();
    let foreign = resolve(
        &a.store,
        a.owner,
        &b.user().await,
        EPOCH,
        &request,
        "그 기억 지워 줘",
        &by_id,
    )
    .await;
    assert!(matches!(foreign, Err(StoreError::Forbidden)));

    let plan = a.plan("그 기억 지워 줘", &by_id).await;
    assert!(matches!(plan, Plan::Forget { .. }));
    let other_user = b.user().await;
    assert!(matches!(
        apply(&a.store, a.owner, &other_user, EPOCH, &plan).await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        apply(&b.store, b.owner, &other_user, EPOCH, &plan).await,
        Err(StoreError::Forbidden)
    ));
    // A plan resumed under another session epoch or generation writes nothing.
    assert!(matches!(
        apply(&a.store, a.owner, &a.user().await, EPOCH + 1, &plan).await,
        Err(StoreError::Unauthorized)
    ));
    let mut next = a.owner;
    next.generation += 1;
    assert!(matches!(
        apply(&a.store, next, &a.user().await, EPOCH, &plan).await,
        Err(StoreError::Forbidden)
    ));
    assert_eq!(a.all().await.len(), 1);

    // The object moves on after planning: the plan is stale and overwrites/forgets nothing.
    a.store
        .correct_memory(
            &a.user().await,
            mine.id.clone(),
            1,
            uuid::Uuid::new_v4().to_string(),
            MemoryDraft {
                text: "갑각류만 안 된다".into(),
                subject: "식사 취향".into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![],
            },
        )
        .await
        .unwrap();
    assert_eq!(run(&a, &plan).await, Outcome::Stale);
    assert_eq!(show(&a, &Outcome::Stale).await, View::Stale);
    let all = a.all().await;
    assert_eq!(
        (all.len(), all[0].revision, all[0].draft.text.as_str()),
        (1, 2, "갑각류만 안 된다")
    );
    // B's own data was never touched.
    assert!(b.all().await.is_empty());
}

#[test]
fn model_output_is_closed_and_cannot_name_a_memory() {
    let ok: Decision = serde_json::from_str(
        r#"{"operation":"edit","lasting":"one_time","subject":"식사","value":"x"}"#,
    )
    .unwrap();
    assert_eq!(
        (ok.operation, ok.lasting, ok.memory),
        (Some(Operation::Edit), Lasting::OneTime, None)
    );
    for bad in [
        r#"{"operation":"forget","memory":{"id":"a","revision":1}}"#,
        r#"{"operation":"forget","memory_id":"a"}"#,
        r#"{"operation":"shell"}"#,
        r#"{"operation":"create","evidence":[]}"#,
    ] {
        assert!(serde_json::from_str::<Decision>(bad).is_err(), "{bad}");
    }
    // The same authenticated request always derives the same write id; operations differ.
    let owner = VolumeOwner {
        account: uuid::Uuid::from_u128(1),
        tenant: uuid::Uuid::from_u128(2),
        volume: uuid::Uuid::from_u128(3),
        generation: 1,
    };
    let r = uuid::Uuid::new_v4().to_string();
    assert_eq!(
        request_uuid(&owner, &r, Operation::Create),
        request_uuid(&owner, &r, Operation::Create)
    );
    assert_ne!(
        request_uuid(&owner, &r, Operation::Create),
        request_uuid(&owner, &r, Operation::Edit)
    );
    let mut other = owner;
    other.tenant = uuid::Uuid::from_u128(9);
    assert_ne!(
        request_uuid(&owner, &r, Operation::Create),
        request_uuid(&other, &r, Operation::Create)
    );
}
