//! Scoped automatic recall of saved, hand-managed personal memories for the
//! text-only preview. Candidates are advisory: admission re-reads every pinned
//! memory in its own writer transaction, so a stale caller list is never trusted.
//! No memory text is logged or placed in an error.
use super::*;
use crate::knowledge::{normalize as squash, tenant_key};
use moosuri_core::{KnowledgeKind, KnowledgeQuery, MemoryDocument, MemoryDraft, MemoryStatus};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const MAX_MEMORIES: usize = 3;
const MAX_TERMS: usize = 3;
const MAX_SUBJECT_BYTES: usize = 200;
const MAX_TEXT_BYTES: usize = 1024;
pub(super) const PER_TERM_HITS: usize = 10;

/// A memory identity pinned to the exact revision the classifier-side caller saw.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallMemory {
    pub id: String,
    pub revision: i64,
}

/// Plaintext that may reach the broker. Deliberately has no Debug implementation.
pub(super) struct Recalled {
    pub id: String,
    pub revision: i64,
    pub subject: String,
    pub text: String,
}

fn clean_terms(terms: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for phrase in terms.iter().take(64) {
        if phrase.chars().count() > 64 || phrase.chars().any(char::is_control) {
            continue;
        }
        for noun in phrase.split_whitespace() {
            let term = noun.to_lowercase();
            if !(2..=32).contains(&term.chars().count())
                || matches!(
                    term.as_str(),
                    "취향" | "선호" | "선호도" | "추천" | "기억" | "preference" | "preferences"
                )
            {
                continue;
            }
            if seen.insert(term.clone()) {
                out.push(term);
                if out.len() == MAX_TERMS {
                    return out;
                }
            }
        }
    }
    out
}

fn personal_assertion(doc: &MemoryDocument) -> bool {
    let draft = &doc.draft;
    doc.status == MemoryStatus::Active
        && doc.extraction_origin.is_none()
        && draft.evidence.is_empty()
        && draft
            .speaker
            .as_deref()
            .is_none_or(|s| matches!(s.trim(), "" | "본인" | "self"))
        && !draft.subject.trim().is_empty()
        && draft.subject.len() <= MAX_SUBJECT_BYTES
        && !draft.text.trim().is_empty()
        && draft.text.len() <= MAX_TEXT_BYTES
}

/// Re-read one pinned memory. Returns `None` unless it is still the revision the
/// ordinary search would select now and is a plain first-person saved memory.
fn current(
    conn: &Connection,
    tenant: &str,
    id: &str,
    revision: i64,
    now: i64,
) -> Result<Option<MemoryDocument>> {
    if revision <= 0 || Uuid::parse_str(id).map_or(true, |u| u.to_string() != id) {
        return Ok(None);
    }
    let selected: Option<i64> = conn.query_row(
        "SELECT max(t.revision) FROM knowledge_memory_timeline t JOIN knowledge_memories m ON m.tenant_id=t.tenant_id AND m.id=t.memory_id WHERE t.tenant_id=?1 AND t.memory_id=?2 AND m.status='active' AND (t.valid_from IS NULL OR t.valid_from<=?3)",
        params![tenant, id, now],
        |r| r.get(0),
    )?;
    if selected != Some(revision) {
        return Ok(None);
    }
    let document = match crate::knowledge::memories::document(conn, tenant, id, Some(revision)) {
        Ok(document) => document,
        Err(StoreError::NotFound | StoreError::InvalidInput) => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let draft = &document.draft;
    let usable = personal_assertion(&document)
        && draft.valid_from.is_none_or(|v| v <= now)
        && draft.valid_until.is_none_or(|v| v > now);
    Ok(usable.then_some(document))
}

/// A generic answer or an already-made choice for this occasion doesn't need a standing preference.
pub fn allows_preference_recall(text: &str) -> bool {
    let words = squash(text);
    let contains = |phrases: &[&str]| phrases.iter().any(|phrase| words.contains(phrase));
    let generic = contains(&["개인화없이", "일반적으로만", "일반적인답"]);
    let subjects = ["기억", "취향", "선호", "지난대화"];
    let clauses = || {
        text.split_inclusive([',', '，', '!', '！', '?', '？', '。', '\n'])
            .flat_map(|line| line.split(". "))
            .map(squash)
    };
    let omits_subject = clauses().any(|part| {
        subjects.iter().any(|&subject| {
            part.match_indices(subject).any(|(index, _)| {
                let after = part[index + subject.len()..]
                    .trim_start_matches(['은', '는', '을', '를', '도', '만']);
                if [
                    "말고",
                    "없이",
                    "빼고",
                    "빼줘",
                    "빼주",
                    "제외",
                    "무시해",
                    "무시하고",
                ]
                .iter()
                .any(|phrase| after.starts_with(phrase))
                {
                    return true;
                }
                // "취향을 따르지 마" declines the preference. "취향을 놓치지 마" asks to use it.
                ["지마", "지말"].iter().any(|ending| {
                    after.match_indices(ending).any(|(i, _)| {
                        !["잊", "놓치", "무시하", "빼먹", "까먹", "빠뜨리"]
                            .iter()
                            .any(|verb| after[..i].ends_with(verb))
                    })
                })
            })
        })
    });
    // Explicit use/omission can also be stated across a clause boundary.
    let refuses_use = contains(&subjects)
        && contains(&[
            "반영하지",
            "참고하지",
            "사용하지",
            "쓰지",
            "적용하지",
            "제외해",
        ]);
    if generic || omits_subject || refuses_use {
        return false;
    }
    let asks_usual = clauses().any(|part| {
        ["평소", "취향", "선호"]
            .iter()
            .any(|word| part.contains(word))
            && ["알려", "추천", "어떤", "무슨", "?", "？"]
                .iter()
                .any(|word| part.contains(word))
    });
    let already_chosen = clauses().any(|part| {
        let ending = part
            .trim_end_matches([',', '，', '.', '。', '!', '！', '~', '…', 'ㅋ', 'ㅎ'])
            .trim_end_matches('요');
        ["오늘", "이번", "한번만", "한잔만"]
            .iter()
            .any(|word| part.contains(word))
            && [
                "로할래",
                "로할게",
                "로하자",
                "로마실래",
                "로마실게",
                "로주문할게",
                "로정했어",
            ]
            .iter()
            .any(|choice| ending.ends_with(choice))
    });
    asks_usual || !already_chosen
}

/// Expiry and a scheduled change of preference can occur without a write.
/// A remembered answer may supply history or settle only while its sources apply.
pub(super) fn can_use_history(
    conn: &Connection,
    tenant: &str,
    run: &str,
    allow_memory: bool,
) -> Result<bool> {
    let mut query = conn.prepare(
        "SELECT object_id,revision FROM agent_dependencies
        WHERE tenant_id=?1 AND run_id=?2 AND kind='memory'",
    )?;
    let rows = query.query_map(params![tenant, run], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let at = crate::now()?;
    for row in rows {
        let (id, revision) = row?;
        if !allow_memory || current(conn, tenant, &id, revision, at)?.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Admission-time validation. Invalid, stale or duplicate entries are dropped
/// quietly; ordinary chat must not fail because optional recall went stale.
pub(super) fn validated(
    conn: &Connection,
    tenant: &str,
    requested: &[RecallMemory],
) -> Result<Vec<Recalled>> {
    let now = crate::now()?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for item in requested.iter().take(16) {
        if out.len() == MAX_MEMORIES {
            break;
        }
        if !seen.insert(item.id.clone()) {
            continue;
        }
        if let Some(document) = current(conn, tenant, &item.id, item.revision, now)? {
            out.push(Recalled {
                id: document.id,
                revision: document.revision,
                subject: document.draft.subject,
                text: document.draft.text,
            });
        }
    }
    Ok(out)
}

/// The final user message: untrusted data carrying the current question.
pub(super) fn wrap(request: &str, memories: &[Recalled]) -> Result<String> {
    serde_json::to_string(&serde_json::json!({
        "current_user_request": request,
        "member_memories": memories.iter().map(|m| serde_json::json!({"subject": m.subject, "text": m.text})).collect::<Vec<_>>(),
    }))
    .map_err(|_| StoreError::Corrupt)
}

struct Candidate {
    id: String,
    revision: i64,
    score: u32,
    subject_hits: u32,
    updated: i64,
}
impl Candidate {
    fn key(&self) -> (u32, u32, i64) {
        (self.score, self.subject_hits, self.updated)
    }
}

fn recall(conn: &Connection, tenant: &str, terms: &[String]) -> Result<Vec<RecallMemory>> {
    let now = crate::now()?;
    let mut matches = BTreeMap::new();
    let mut candidates = Vec::new();
    let key = tenant_key(tenant);
    let mut topic_lookup = conn.prepare(
        "SELECT m.id,m.revision FROM knowledge_fts i
         JOIN knowledge_memories m ON m.tenant_id=?3 AND m.id=i.object_id AND m.revision=CAST(i.revision AS INTEGER)
         JOIN knowledge_memory_versions v ON v.tenant_id=m.tenant_id AND v.memory_id=m.id AND v.revision=m.revision
         WHERE i.tenant_key=?1 AND i.kind='preference-topic' AND i.normalized=?2 AND m.status='active' AND v.status='active'
           AND COALESCE(json_extract(v.draft,'$.valid_from'),?4)<=?4
           AND (json_extract(v.draft,'$.valid_until') IS NULL OR json_extract(v.draft,'$.valid_until')>?4)
         ORDER BY m.updated_at DESC,i.rowid DESC LIMIT ?5",
    )?;
    for term in terms {
        let topics = topic_lookup.query_map(
            params![key, term, tenant, now, PER_TERM_HITS as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        for topic in topics {
            *matches.entry(topic?).or_insert(0u32) += 1;
        }
        let result = crate::knowledge::search::search_owner(
            conn,
            tenant,
            KnowledgeQuery {
                kind: KnowledgeKind::Memory,
                text: term.clone(),
                match_mode: None,
                collection_id: None,
                subject: None,
                speaker: None,
                valid_at: Some(now),
                observed_from: None,
                observed_until: None,
                cursor: None,
            },
            PER_TERM_HITS,
        )?;
        for hit in result.hits {
            matches.entry((hit.id, hit.revision)).or_default();
        }
    }
    for ((id, revision), mut score) in matches {
        let Some(document) = current(conn, tenant, &id, revision, now)? else {
            continue;
        };
        let subject = squash(&document.draft.subject);
        let body = squash(&document.draft.text);
        let mut subject_hits = 0;
        for term in terms {
            if subject.contains(term) {
                score += 2;
                subject_hits += 1;
            } else if body.contains(term) {
                score += 1;
            }
        }
        if score > 0 {
            candidates.push(Candidate {
                id,
                revision,
                score,
                subject_hits,
                updated: document.updated_at,
            });
        }
    }
    // Strength first; recency only breaks ties. Never order by id.
    candidates.sort_by(|a, b| b.key().cmp(&a.key()));
    let mut chosen = candidates.len().min(MAX_MEMORIES);
    if candidates.len() > MAX_MEMORIES {
        // An undecidable tie at the cutoff is ambiguity: leave the whole tied group out.
        let edge = candidates[MAX_MEMORIES].key();
        if candidates[MAX_MEMORIES - 1].key() == edge {
            chosen = candidates[..MAX_MEMORIES]
                .iter()
                .take_while(|c| c.key() != edge)
                .count();
        }
    }
    candidates.truncate(chosen);
    Ok(candidates
        .into_iter()
        .map(|c| RecallMemory {
            id: c.id,
            revision: c.revision,
        })
        .collect())
}

impl Store {
    /// Commit the member's literal assertion and its optional search hint together.
    /// The hint is private to automatic recall. Manual reads and searches use the original draft.
    pub async fn write_member_preference(
        &self,
        user: &UserContext,
        owner: personal_volume::VolumeOwner,
        previous: Option<(String, i64)>,
        request: String,
        draft: MemoryDraft,
        topic: Option<String>,
    ) -> Result<MemoryDocument> {
        let request = Uuid::parse_str(&request)
            .map_err(|_| StoreError::InvalidInput)?
            .to_string();
        let words = squash(&format!("{} {}", draft.subject, draft.text));
        let hint = clean_terms(topic.into_iter().collect())
            .into_iter()
            .find(|term| !words.contains(term));
        let user = user.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let user = authority(&tx, &user, owner)?;
            let tenant = &user.tenant.0;
            let replay = hint.is_some() && tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM knowledge_writes WHERE tenant_id=?1 AND request_id=?2)",
                params![tenant, request], |r| r.get::<_, bool>(0),
            )?;
            let doc = crate::knowledge::memories::write_in_transaction(&tx, tenant, previous, request, draft)?;
            if let Some(term) = hint && !replay && personal_assertion(&doc) {
                tx.execute("INSERT INTO knowledge_fts(tenant_key,kind,object_id,revision,normalized)
                    VALUES(?1,'preference-topic',?2,?3,?4)", params![tenant_key(tenant), doc.id, doc.revision, term])?;
            }
            tx.commit()?;
            Ok(doc)
        }).await
    }

    /// Up to three current, relevant, first-person saved memories for the topic
    /// nouns. No terms means no recall; this never browses the memory list.
    pub async fn member_text_recall(
        &self,
        user: &UserContext,
        owner: personal_volume::VolumeOwner,
        terms: Vec<String>,
    ) -> Result<Vec<RecallMemory>> {
        let terms = clean_terms(terms);
        let user = user.clone();
        self.call(move |db| {
            let user = authority(&db.conn, &user, owner)?;
            if terms.is_empty() {
                return Ok(vec![]);
            }
            recall(&db.conn, &user.tenant.0, &terms)
        })
        .await
    }
}
