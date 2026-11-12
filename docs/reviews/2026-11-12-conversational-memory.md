# Conversational memory acceptance — 2026-11-12

Scope: migration6, isolated current-user planning, stable memory-command slots,
terminal truthful confirmations and forget journal replay. Automated acceptance uses
synthetic accounts and file SQLite with scripted loopback providers. The owner has
separately authorized live ChatGPT quality evaluation; those results are not inferred
from these tests. No production service, collector, cron, data or vault was changed.

## Independent review and fixes

Non-authors reviewed the frozen planner, Store and runtime/HTTP changes. Findings
were corrected and the modified boundaries reviewed again:

1. The resolver examined only the newest management head. With a future correction,
   it could miss a currently effective match and delete another ambiguous match.
   It now uses the same effective-time selection as search, pins head/effective
   revisions separately and rechecks uniqueness. Tests cover two current matches,
   a unique future-head target, time advancing and an invalidated future head.
2. Retry after an executed read could invoke the planner again, consuming budget
   and risking a changed decision. Store now exposes eligibility derived from its
   retained run-wide tool counter. A failed recall/retry test asserts one classifier
   across five model calls and a successful grounded answer.
3. Adopted journal replay originally rechecked current memory validity. Expiry during
   downtime could block completion of an already committed deletion. Replay now uses
   the sealed structural revision/status without current-time or session checks.
   Preparation before adoption still never autoexecutes on restart.

The exact quote check proves current-message origin, not semantic authorization.
Current Korean/English cues and temporal phrasing are bounded heuristics. No live
semantic score is claimed by the fixture corpus's `not_run` metadata.

## Automated evidence

- Final integrated Rust191 passing cases: Store74, agent37, connector59, HTTP20 and
  CLI installation1. The separate child-process writer-lock invocation is part of
  Store74, not an extra. The live classifier is ignored by the ordinary test suite.
- macOS passed190 cases before the last cursor-error repair, then the affected19 Store
  agent-execution cases passed including its new regression. Linux passed the full191
  cases in offline Docker with Rust1.89. The subsequent prompt-only change passed all7
  parser/protocol cases on both platforms. Fmt and all-target Clippy passed; generated
  DTO agreement, strict OpenSpec and planning-link verification are also required.
- Store covers literal quotes, tenant/auth/provider fences, stable identity/reopen,
  correction CAS, ambiguity, cancelled/expired work, pre/post-journal interruption,
  deleted conversation recovery, terminal output purge and retained accounting.
- The actual HTTP test drives remember/correct/forget through Router+runtime+file DB,
  verifies cross-thread answer/reference purge and old-cursor SSE reset, and logs out
  during held planning. An open SSE closes unauthorized with no late mutation or
  hidden-planner preview; a fresh login sees no new memory and retains usage.
- Real runtime tests cover a new-thread recall, self-invalidating correction, no-command
  isolation, malformed quote rejection, cancellation, ambiguous forget and sealed-plan
  retry without another model call. Seven parser/protocol cases cover four dialects,
  duplicate/unknown fields, byte bounds and literal Unicode.
- React70 cases, typecheck and production build pass. This unit only updates knowledge
  help copy; existing reset, draft and exact-reference tests remain the UI coverage.
- Bounded memory browsing preserves effective-time/account isolation, real pagination
  and dependency purge. A5+1-page runtime test recalls and then revokes the answer after
  forgetting. A mismatched cursor is persisted as `invalid_input`; a model can repair it
  without resetting budgets or bypassing outer authority/generation fences.
- Three offline evaluator tests check captured-call adaptation, literal-origin rejection
  and model/transport/apparatus failure classification. Actual account-backed results,
  including7/10 end-to-end passes and unresolved target phrasing, are in the separate
  [live evaluation record](2026-11-13-live-chatgpt-quality.md).

## Browser observations

Chromium used the real embedded Rust server and a synthetic compatible provider.
Saving a tea preference returned a real memory reference. A separate chat searched
and recalled it; correction removed the old derived answer and linked revision2.
Opening that reference showed the new text in the memory editor. Forget removed the
memory/history and the correction's reference; the independent fixed deletion receipt
remained. At390×844, document width was390 and the editable unsent draft survived.
Planner preview text never appeared. The screenshot was inspected. This is a viewport
check, not a real-device or accessibility certification.

## Boundaries

No automatic extraction, date inference, semantic merging, arbitrary code, external
work or user-key vault is implemented by this unit. Source documents and original user
messages have their own deletion scope. Already delivered data cannot be recalled.
The41 public semantic cases are a development corpus, not a blind holdout. Live-model
quality, latency and authorization errors must be reported separately, including
failed cases and the actual transport/model/effort. CI remains disabled.
