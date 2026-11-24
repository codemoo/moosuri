//! Long-term member preferences from ordinary Korean chat.
//!
//! This is a *helper*, not an endpoint, registry or model call. The work interpreter (which
//! already journals a request) supplies one typed [`Decision`] proposed by the administrator-
//! pinned broker. Nothing in the decision is trusted:
//!
//! 1. [`resolve`] re-checks the member's own words (quoted, negated, reported, hypothetical and
//!    single-trip text never persists), requires every literal to come from the member's text,
//!    maps the target to exactly one live memory by exact name (never a guess) and returns a
//!    serializable [`Plan`] pinned to owner/tenant/volume/generation, session epoch, object and
//!    revision. Nothing is written.
//! 2. [`apply`] re-checks the pins against a freshly authenticated member and calls the regular
//!    `create_memory` / `correct_memory` / `forget_memory` domain methods with a request UUID
//!    derived from the authenticated interpreter request, so a retry is the store's own
//!    idempotent receipt, never a second write.
//! 3. [`project`] turns the stored [`Outcome`] (ids, revisions, operation only) into a live view,
//!    so a replay shows the *current* text after a correction and nothing after a forget.
//!
//! No schema, mail, calendar, OAuth scope or external effect is involved.
use moosuri_core::{MemoryDocument, MemoryDraft};
use moosuri_store::{Store, StoreError, UserContext, personal_volume::VolumeOwner};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MAX_SUBJECT: usize = 80;
const MAX_VALUE: usize = 500;
const MAX_PAGES: usize = 210; // 4096 memories / 20 per page, plus slack
const MAX_CANDIDATES: usize = 5;

// ------------------------------------------------------------------ typed decision

use moosuri_core::{
    MemberMemoryCandidate as Candidate, MemberMemoryOperation as Operation,
    MemberMemoryView as View,
};
/// Whether the member meant this to last. A single-trip exception never becomes a memory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Lasting {
    #[default]
    Persistent,
    OneTime,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MemoryRef {
    pub id: String,
    pub revision: i64,
}
/// Closed shape for the broker's proposal. `memory` can only be set by trusted server code
/// (for example from an earlier receipt), never by model output.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Decision {
    pub operation: Option<Operation>,
    #[serde(default)]
    pub lasting: Lasting,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(skip)]
    pub memory: Option<MemoryRef>,
}
#[cfg(test)]
impl Decision {
    pub(crate) fn new(operation: Operation) -> Self {
        Self {
            operation: Some(operation),
            ..Self::default()
        }
    }
    pub(crate) fn with_memory(mut self, memory: MemoryRef) -> Self {
        self.memory = Some(memory);
        self
    }
}

// ------------------------------------------------------------------ plan and outcome

/// Why nothing was changed. Carries ids only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum NoChange {
    OneTime,
    Negated,
    NotARequest,
    NeedsLiteral,
    NoMatch,
    Stale,
    Ambiguous { candidates: Vec<MemoryRef> },
    Exists { memory: MemoryRef },
    Unchanged { memory: MemoryRef },
}
impl NoChange {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::OneTime => "one_time",
            Self::Negated => "negated",
            Self::NotARequest => "not_a_request",
            Self::NeedsLiteral => "needs_literal",
            Self::NoMatch => "no_match",
            Self::Stale => "stale",
            Self::Ambiguous { .. } => "ambiguous",
            Self::Exists { .. } => "exists",
            Self::Unchanged { .. } => "unchanged",
        }
    }
}
/// The authority a plan was made under; apply refuses any other member, volume or session.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Pin {
    account: uuid::Uuid,
    tenant: uuid::Uuid,
    volume: uuid::Uuid,
    generation: u64,
    epoch: i64,
    request: uuid::Uuid,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Fields {
    subject: String,
    text: String,
    speaker: Option<String>,
    valid_from: Option<i64>,
    valid_until: Option<i64>,
}
impl Fields {
    fn draft(&self) -> MemoryDraft {
        MemoryDraft {
            text: self.text.clone(),
            subject: self.subject.clone(),
            speaker: self.speaker.clone(),
            valid_from: self.valid_from,
            valid_until: self.valid_until,
            // The member's own chat words are the assertion; nothing is source-dependent.
            evidence: vec![],
        }
    }
}
/// Serializable, pinned decision. Stored in the interpreter journal before anything is written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Plan {
    NoChange(NoChange),
    Create {
        pin: Pin,
        fields: Fields,
    },
    Edit {
        pin: Pin,
        memory: MemoryRef,
        fields: Fields,
    },
    Forget {
        pin: Pin,
        memory: MemoryRef,
    },
}
/// What the journal keeps after applying: ids, revisions and the operation, never text.
/// For a forget, `memory.revision` is the tombstone revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum Outcome {
    NoChange(NoChange),
    Applied {
        operation: Operation,
        memory: MemoryRef,
    },
    /// The pinned revision moved or the object is gone; nothing was changed.
    Stale,
}

// ------------------------------------------------------------------ text gates

// All marker lists are written without spaces: they are matched against folded text.
const ONE_TIME: &[&str] = &[
    "이번만",
    "이번에만",
    "이번한번",
    "이번엔",
    "이번에는",
    "이번여행",
    "이번출장",
    "이번주만",
    "오늘만",
    "오늘은",
    "잠깐만",
    "한번만",
    "일회성",
    "임시로",
    "당분간",
];
const NEGATION: &[&str] = &[
    "지마",
    "지말",
    "지않",
    "말아",
    "마세요",
    "마라",
    "않아도",
    "안해도",
    "필요없",
    "금지",
    "안돼",
    "하면안",
    "안할",
    "못하게",
];
/// "잊지 말고 기억해" asks to remember; these are not negations of a save.
const AFFIRMED: &[&str] = &["잊지마", "잊지말", "잊지않"];
const REPORTED: &[&str] = &["라고말", "라고했", "라는말"];
const INFORMATIONAL: &[&str] = &[
    "어떻게",
    "방법",
    "왜",
    "뭐야",
    "무엇",
    "설명",
    "의미",
    "만약",
    "가정",
    "예를들어",
];
const REQUEST_FORM: &[&str] = &["줘", "주세요", "줄래", "줄수", "해라", "세요"];
const CREATE_WORDS: &[&str] = &[
    "기억해",
    "기억하세요",
    "기억하도록",
    "저장해",
    "메모해",
    "적어둬",
    "적어놔",
    "적어줘",
    "알아둬",
    "알아두세요",
];
const EDIT_WORDS: &[&str] = &[
    "바꿔",
    "바꾸세요",
    "고쳐",
    "수정",
    "변경",
    "정정",
    "업데이트",
    "교체",
];
const FORGET_WORDS: &[&str] = &[
    "잊어줘",
    "잊어주",
    "잊어버려",
    "잊어라",
    "잊어요",
    "잊으세요",
    "지워",
    "지우세요",
    "삭제",
    "없애",
];
const FORGET_BARE: &[&str] = &["잊어"];

fn folded(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}
fn has(text: &str, keys: &[&str]) -> bool {
    keys.iter().any(|key| text.contains(key))
}
/// Text outside any quotation pair, so a quoted command is not a command.
fn outside_quotes(text: &str) -> String {
    let pairs = [
        ('"', '"'),
        ('\'', '\''),
        ('“', '”'),
        ('‘', '’'),
        ('「', '」'),
        ('『', '』'),
        ('《', '》'),
        ('`', '`'),
    ];
    let mut out = String::new();
    let mut closing: Option<char> = None;
    for c in text.chars() {
        match closing {
            Some(end) if c == end => closing = None,
            Some(_) => {}
            None => match pairs.iter().find(|(open, _)| *open == c) {
                Some((_, end)) => closing = Some(*end),
                None => out.push(c),
            },
        }
    }
    out
}
/// A literal must be the member's own words, bounded and free of control characters.
fn literal(value: &Option<String>, text: &str, max: usize) -> Option<String> {
    let value = value.as_deref()?.trim();
    let valid = !value.is_empty()
        && value.chars().count() <= max
        && !value.chars().any(char::is_control)
        && folded(text).contains(&folded(value));
    valid.then(|| value.to_owned())
}
/// Deterministic checks that run after the model's proposal and before any lookup or write.
fn gate(
    text: &str,
    decision: &Decision,
    operation: Operation,
    literals: &[&str],
) -> Option<NoChange> {
    if decision.lasting == Lasting::OneTime {
        return Some(NoChange::OneTime);
    }
    let outside = folded(&outside_quotes(text));
    if has(&outside, ONE_TIME) {
        return Some(NoChange::OneTime);
    }
    // Literals (a value such as "전화하지 마") are data, not the framing of the request.
    let mut framing = outside;
    for l in literals {
        let l = folded(l);
        if !l.is_empty() {
            framing = framing.replace(&l, "");
        }
    }
    if operation != Operation::Forget {
        for k in AFFIRMED {
            framing = framing.replace(k, "");
        }
    }
    if has(&framing, NEGATION) {
        return Some(NoChange::Negated);
    }
    if has(&framing, REPORTED) || has(&framing, INFORMATIONAL) {
        return Some(NoChange::NotARequest);
    }
    let core = framing.trim_end_matches(['.', '!', '~', '…']);
    let bare = core.trim_end_matches('?');
    let imperative = match operation {
        Operation::Create => has(core, CREATE_WORDS),
        Operation::Edit => has(core, EDIT_WORDS) || has(core, CREATE_WORDS),
        Operation::Forget => {
            has(core, FORGET_WORDS) || FORGET_BARE.iter().any(|w| bare.ends_with(w))
        }
    };
    if !imperative || (core.ends_with('?') && !has(core, REQUEST_FORM)) {
        return Some(NoChange::NotARequest);
    }
    None
}

// ------------------------------------------------------------------ resolve

fn check_member(owner: VolumeOwner, user: &UserContext) -> Result<(), StoreError> {
    if user.account().id != owner.account.to_string() {
        return Err(StoreError::Forbidden);
    }
    Ok(())
}
fn request_uuid(owner: &VolumeOwner, request: &str, operation: Operation) -> uuid::Uuid {
    // Same recipe as the interpreter's other stable ids: label, account, tenant, request.
    let hash: [u8; 32] = Sha256::digest(
        serde_json::to_vec(&(
            "member-preference-v1",
            owner.account,
            owner.tenant,
            request,
            operation,
        ))
        .unwrap_or_default(),
    )
    .into();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}
async fn live_memories(
    store: &Store,
    user: &UserContext,
) -> Result<Vec<MemoryDocument>, StoreError> {
    let (mut all, mut after) = (Vec::new(), None);
    for _ in 0..MAX_PAGES {
        let page = store.memories(user, after).await?;
        all.extend(page.items);
        match page.next_after {
            Some(next) => after = Some(next),
            None => return Ok(all),
        }
    }
    Err(StoreError::Corrupt)
}
fn reference(doc: &MemoryDocument) -> MemoryRef {
    MemoryRef {
        id: doc.id.clone(),
        revision: doc.revision,
    }
}
enum Found {
    One(MemoryDocument),
    Not(NoChange),
}
/// Exact id+revision, or exact (folded) subject. A partial name is never guessed.
async fn locate(
    store: &Store,
    user: &UserContext,
    decision: &Decision,
    subject: Option<&str>,
) -> Result<Found, StoreError> {
    if let Some(pinned) = &decision.memory {
        return match store.memory(user, pinned.id.clone(), None).await {
            Ok(doc) if doc.revision == pinned.revision => Ok(Found::One(doc)),
            Ok(_) => Ok(Found::Not(NoChange::Stale)),
            Err(StoreError::NotFound | StoreError::InvalidInput) => {
                Ok(Found::Not(NoChange::NoMatch))
            }
            Err(error) => Err(error),
        };
    }
    let Some(subject) = subject else {
        return Ok(Found::Not(NoChange::NeedsLiteral));
    };
    let wanted = folded(subject);
    let all = live_memories(store, user).await?;
    let exact: Vec<&MemoryDocument> = all
        .iter()
        .filter(|d| folded(&d.draft.subject) == wanted)
        .collect();
    if let [one] = exact.as_slice() {
        return Ok(Found::One((*one).clone()));
    }
    let near: Vec<&MemoryDocument> = if exact.is_empty() {
        all.iter()
            .filter(|d| {
                let name = folded(&d.draft.subject);
                name.contains(&wanted) || wanted.contains(&name)
            })
            .collect()
    } else {
        exact
    };
    if near.is_empty() {
        return Ok(Found::Not(NoChange::NoMatch));
    }
    Ok(Found::Not(NoChange::Ambiguous {
        candidates: near
            .into_iter()
            .take(MAX_CANDIDATES)
            .map(reference)
            .collect(),
    }))
}

/// Decision + the member's real words + live memories -> exactly one pinned plan.
/// `request_id` is the authenticated interpreter request UUID; `epoch` the gateway session epoch.
/// `user` must be the freshly authenticated member. Reads only; nothing is written.
pub(crate) async fn resolve(
    store: &Store,
    owner: VolumeOwner,
    user: &UserContext,
    epoch: i64,
    request_id: &str,
    text: &str,
    decision: &Decision,
) -> Result<Plan, StoreError> {
    check_member(owner, user)?;
    uuid::Uuid::parse_str(request_id).map_err(|_| StoreError::InvalidInput)?;
    let Some(operation) = decision.operation else {
        return Ok(Plan::NoChange(NoChange::NotARequest));
    };
    let subject = literal(&decision.subject, text, MAX_SUBJECT);
    let value = literal(&decision.value, text, MAX_VALUE);
    let used: Vec<&str> = [subject.as_deref(), value.as_deref()]
        .into_iter()
        .flatten()
        .collect();
    if let Some(reason) = gate(text, decision, operation, &used) {
        return Ok(Plan::NoChange(reason));
    }
    let pin = Pin {
        account: owner.account,
        tenant: owner.tenant,
        volume: owner.volume,
        generation: owner.generation,
        epoch,
        request: request_uuid(&owner, request_id, operation),
    };
    let none = |reason| Ok(Plan::NoChange(reason));
    match operation {
        Operation::Create => {
            let (Some(subject), Some(value)) = (subject, value) else {
                return none(NoChange::NeedsLiteral);
            };
            let wanted = folded(&subject);
            let existing = live_memories(store, user)
                .await?
                .into_iter()
                .find(|d| folded(&d.draft.subject) == wanted);
            if let Some(doc) = existing {
                return none(NoChange::Exists {
                    memory: reference(&doc),
                });
            }
            Ok(Plan::Create {
                pin,
                fields: Fields {
                    subject,
                    text: value,
                    speaker: None,
                    valid_from: None,
                    valid_until: None,
                },
            })
        }
        Operation::Edit => {
            let Some(value) = value else {
                return none(NoChange::NeedsLiteral);
            };
            let doc = match locate(store, user, decision, subject.as_deref()).await? {
                Found::One(doc) => doc,
                Found::Not(reason) => return none(reason),
            };
            if folded(&doc.draft.text) == folded(&value) {
                return none(NoChange::Unchanged {
                    memory: reference(&doc),
                });
            }
            Ok(Plan::Edit {
                pin,
                memory: reference(&doc),
                fields: Fields {
                    subject: doc.draft.subject.clone(),
                    text: value,
                    speaker: doc.draft.speaker.clone(),
                    valid_from: doc.draft.valid_from,
                    valid_until: doc.draft.valid_until,
                },
            })
        }
        Operation::Forget => match locate(store, user, decision, subject.as_deref()).await? {
            Found::One(doc) => Ok(Plan::Forget {
                pin,
                memory: reference(&doc),
            }),
            Found::Not(reason) => none(reason),
        },
    }
}

// ------------------------------------------------------------------ apply

/// Re-check authority, then perform exactly the planned domain call. Safe to repeat with the
/// same plan: create/correct return the store's receipt, forget is idempotent at its revision.
/// `user` must be authenticated *after* any model call (a revoked member mutates nothing).
/// Hold `AuthState::delivery.read()` around this call, as the interpreter's `execute` does.
pub(crate) async fn apply(
    store: &Store,
    owner: VolumeOwner,
    user: &UserContext,
    epoch: i64,
    plan: &Plan,
) -> Result<Outcome, StoreError> {
    let pin = match plan {
        Plan::NoChange(reason) => return Ok(Outcome::NoChange(reason.clone())),
        Plan::Create { pin, .. } | Plan::Edit { pin, .. } | Plan::Forget { pin, .. } => pin,
    };
    check_member(owner, user)?;
    if pin.account != owner.account
        || pin.tenant != owner.tenant
        || pin.volume != owner.volume
        || pin.generation != owner.generation
    {
        return Err(StoreError::Forbidden);
    }
    if pin.epoch != epoch {
        return Err(StoreError::Unauthorized);
    }
    let stale = |error: StoreError| match error {
        StoreError::Conflict | StoreError::NotFound => Ok(Outcome::Stale),
        other => Err(other),
    };
    match plan {
        Plan::NoChange(_) => unreachable!("returned above"),
        Plan::Create { pin, fields } => {
            match store
                .create_memory(user, pin.request.to_string(), fields.draft())
                .await
            {
                Ok(doc) => Ok(Outcome::Applied {
                    operation: Operation::Create,
                    memory: reference(&doc),
                }),
                Err(error) => stale(error),
            }
        }
        Plan::Edit {
            pin,
            memory,
            fields,
        } => {
            match store
                .correct_memory(
                    user,
                    memory.id.clone(),
                    memory.revision,
                    pin.request.to_string(),
                    fields.draft(),
                )
                .await
            {
                Ok(doc) => Ok(Outcome::Applied {
                    operation: Operation::Edit,
                    memory: reference(&doc),
                }),
                Err(error) => stale(error),
            }
        }
        Plan::Forget { memory, .. } => {
            match store
                .forget_memory(user, memory.id.clone(), memory.revision)
                .await
            {
                Ok(()) => Ok(Outcome::Applied {
                    operation: Operation::Forget,
                    memory: MemoryRef {
                        id: memory.id.clone(),
                        revision: memory.revision.saturating_add(1),
                    },
                }),
                Err(error) => stale(error),
            }
        }
    }
}

// ------------------------------------------------------------------ live projection

async fn candidate(store: &Store, user: &UserContext, memory: &MemoryRef) -> Option<Candidate> {
    let doc = store.memory(user, memory.id.clone(), None).await.ok()?;
    Some(Candidate {
        memory_id: doc.id,
        revision: doc.revision,
        subject: doc.draft.subject,
    })
}
/// Resolve a stored outcome against the *current* member-scoped memory. Never repeats a write.
pub(crate) async fn project(
    store: &Store,
    owner: VolumeOwner,
    user: &UserContext,
    outcome: &Outcome,
) -> Result<View, StoreError> {
    check_member(owner, user)?;
    Ok(match outcome {
        Outcome::Stale => View::Stale,
        Outcome::Applied {
            operation: Operation::Forget,
            memory,
        } => View::Withdrawn {
            memory_id: memory.id.clone(),
        },
        Outcome::Applied { operation, memory } => {
            match store.memory(user, memory.id.clone(), None).await {
                Ok(doc) => View::Remembered {
                    operation: *operation,
                    memory_id: doc.id,
                    revision: memory.revision,
                    current_revision: doc.revision,
                    subject: doc.draft.subject,
                    text: doc.draft.text,
                    superseded: doc.revision != memory.revision,
                },
                Err(StoreError::NotFound) => View::Withdrawn {
                    memory_id: memory.id.clone(),
                },
                Err(error) => return Err(error),
            }
        }
        Outcome::NoChange(reason) => {
            let refs: Vec<&MemoryRef> = match reason {
                NoChange::Ambiguous { candidates } => candidates.iter().collect(),
                NoChange::Exists { memory } | NoChange::Unchanged { memory } => vec![memory],
                _ => vec![],
            };
            let mut candidates = Vec::new();
            for memory in refs {
                candidates.extend(candidate(store, user, memory).await);
            }
            View::NoChange {
                reason: reason.code().into(),
                candidates,
            }
        }
    })
}

#[cfg(test)]
mod tests;
