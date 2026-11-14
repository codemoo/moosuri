# Remember, correct and forget in conversation

With an exact tool-verified shared or personal ChatGPT model selected, ask to remember one concrete fact, correct one
existing memory, or forget one memory. A clear request is applied without an extra
approval dialog. Examples:

- `나는 홍차를 좋아해. 이 사실을 기억해 줘.`
- `"나는 홍차를 좋아해"라는 기억을 "나는 녹차를 좋아해"로 수정해 줘.`
- `"나는 녹차를 좋아해"라는 기억을 잊어 줘.`

A saved or corrected memory appears in `/app/memory`. The confirmation links to its
actual revision; a new conversation can retrieve it. Forget removes the memory and
its correction history. It does not delete separate source documents or the user's
original chat messages. Derived answers and their references are revoked, including
answers in other conversations. Unsent composer drafts remain intact.

## Intent and target

A lightweight Korean/English cue selects a separate planning call. That call receives
only the current authenticated user message and fixed instructions: no retrieved
sources, earlier chat or previous model continuation. It must return a single completed,
closed function decision. Requested facts and targets are literal current-message
quotes; the model cannot provide authority, tenant or memory IDs. Planning text is
hidden, but its observed usage and dispatch count remain in the run ledger.

The planner distinguishes a command from ordinary conversation, recall, quotation,
negation and capability questions. No-command starts fresh normal chat. These are
model instructions and bounded validation, not proof that the model understands every
sentence. The41-case fixture remains a public development corpus, not a blind holdout.
Its original `not_run` metadata does not contain results; actual account-backed
label and end-to-end evaluations are recorded separately in
[the live evaluation record](reviews/2026-11-13-live-chatgpt-quality.md).
Protocol/parser tests alone are not an intent-accuracy benchmark. Cue routing is
limited to supported Korean/English wording and messages up to8192 UTF-8 bytes.

Correction and forgetting require one unique current match within the account.
The planner preserves a literal target phrase, including meaningful category,
person, negation and scope wording. A unique substring match against the fact or
subject keeps the single-planning-call path. Multiple literal matches require
clarification; a model cannot override that ambiguity.

If no literal match exists, the Store seals the accepted current-message intent
and a complete bounded snapshot before a separate isolated target-selection call.
That call sees the current request and data-only current candidate text, subject,
speaker and validity fields. It returns only a listed numeric index, missing or
ambiguous. It cannot change the operation, replacement fact, tenant or target ID.
Saved instructions and role claims are untrusted candidate data. No shortest-token
or synonym reduction is used. This supports broader wording such as “홍차 취향”
without requiring that phrase to appear in “나는 홍차를 좋아해”.

Enumeration is bounded to64 effective-version rows and32KiB of candidate data;
expired rows count toward the scan bound. Incomplete coverage asks for a narrower
request instead of selecting from a partial list. Any knowledge-generation or full
candidate-snapshot change before selection or commit prevents mutation. A still
eligible command returns clarification; an already revoked or purged run retains
its existing failure/invalidation behavior. This includes other candidate changes and time-only activation or
expiry. Missing/ambiguous selections never mutate a memory. Model selection remains
semantic judgment: scripted checks verify the protocol and storage boundaries,
not arbitrary natural-language accuracy or prompt-injection resistance. The earlier
live7/10 score is preserved; this path has not yet been live-model evaluated.

The effective temporal revision determines the match, while the latest head revision
provides mutation CAS. Both are pinned; changing either, or newly ambiguous matching,
requires clarification with zero mutation. A bounded search that cannot establish a
unique target also asks for a narrower request. The initial command path supports one
undated assertion at a time. Effective-time changes use the existing manual memory
editor. References such as “remember that” without the literal fact need clarification.

## Commit, retry and recovery

A command has one stable slot per run, independent of provider call IDs or retry
attempts. Current session/provider/run authority and target revision are checked
inside the Store actor. An accepted intent resumes before model client construction
or unrelated classification, without another intent-classification call. Explicit
retry clears provider checkpoints but preserves the sealed local intent. See the
[execution-order repair](reviews/2026-11-14-sealed-memory-retry.md). Pending
candidate selection may dispatch again after an explicit retry under the existing
run-wide budget, using the same sealed intent and candidate namespace; an accepted
selection resumes its command slot without another model call. Purging a private
candidate plan permanently fences reclassification in that run. Every candidate's
effective memory and evidence-source revisions are registered as knowledge
dependencies. Forgetting a candidate or withdrawing its evidence purges the private
plan and checkpoints immediately, including candidates the selector would not
choose. Resume revalidates the complete snapshot before returning candidate data;
dispatch admission repeats that check in the model-step transaction. A stale plan
cannot be retransmitted by retry. An already admitted in-flight provider request
retains the existing cancellation/revocation boundary.
A retry after any prior knowledge-tool execution skips memory planning; retrieved
content cannot become authority for a new mutation in that run.

Mutation, content-free receipt and fixed truthful confirmation are committed together.
The runtime discards old assistant output, obsolete events, dependencies and private
continuations. A saved/corrected memory is the new confirmation's only knowledge
reference. There is no model-generated claim of success after the mutation.

Forget first appends and fsyncs its opaque intent in the independent journal.
Preparation before that append is not a completed deletion and never executes
automatically on startup. After adoption, startup replay completes the same deletion,
even if the session or the memory's validity expires during downtime. Its resolution
metadata survives thread deletion, but recovery never recreates a deleted conversation.
A post-append storage failure prevents serving the inconsistent installation.

The existing6 model/12 local tool/120-second limits apply across retries, including
planning. No model/network wait holds the shared HTTP/SSE delivery gate. Cancellation
before commit prevents a mutation; later cancellation cannot undo a committed receipt.
Repeated receipt reads do not repeat the mutation.

This feature is explicit memory management. Automatic extraction, suppression of
re-extraction after forgetting, semantic retrieval and general external actions remain
separate work. Assistant memories are not the user-key encrypted vault.

See [private knowledge](knowledge.md), [knowledge agent](knowledge-agent.md),
[current status](status.md) and [acceptance evidence](reviews/2026-11-12-conversational-memory.md).

Personal ChatGPT uses the same typed intent, target-snapshot, withdrawal and receipt paths. Each initial classifier or candidate-selector request resolves current personal access before durable step admission and uses the plan namespace/dialect. An accepted prepared local command still resumes before model client construction; no fresh provider call is needed just to commit its real receipt. Personal tools proof is protocol evidence, not an accuracy score for intent or semantic target selection. Calendar changes, autonomous missions and media remain separate capabilities.
