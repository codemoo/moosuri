# Memory and current-message context — 2026-11-16

Source review confirmed three practical main-flow problems. The semantic memory
selector counted expired versions against its64-row coverage budget before expiry
filtering;65 expired assertions blocked correction of one current fact. Ordinary
chat lacked a trusted date/time reference despite storing the original timestamp
and timezone. Two literal read-only Korean memory requests also entered the
mutation classifier before ordinary memory read tools.

The selector now filters the newest begun version's expiry before the bounded
coverage probe. It never revives a superseded older version. Current snapshot,
generation, owner, selected-index, target revision and mutation checks remain.
The64-current-candidate and32KiB bounds remain unchanged.

Chat context now carries a trusted original-message UTC timestamp, local RFC3339
timestamp, timezone and date. Missing timezone is explicitly UTC fallback. Retry
preserves the original anchor. Runner projection merges trusted system context
into the leading system instruction, preserving it through tool/media instruction
replacement and history trimming. This also avoids Anthropic late-system errors.
The isolated memory classifier still sees only the current user message; time
metadata grants no action or mutation authority.

The routing shortcut recognizes only complete bounded Korean lookup forms with
an owned target and no other memory/mutation cue. Quoted, mixed and unknown
requests keep existing classification. It creates no write authority and does
not broaden normal tool permissions. No model/effort or schema changes occur.

## Evidence

All fixtures use synthetic accounts, messages, dates and private temporary DBs.
There were no live provider/model calls, production data reads, deployment or push.

- RED: `expired_population_does_not_block_current_semantic_correction_or_revive_old_versions`
  failed on frozen old selector source because preparation returned clarification
  instead of candidate selection. GREEN: the real Store flow selects its one
  current candidate and commits only that correction despite65 expired latest
  versions with earlier undated history.
- `cargo test -p moosuri-store memory_commands:: --offline`: **39 passed**,
  including existing65-active-row overflow, byte limits, expired/future temporal
  snapshots, stale target/authority, retry and journal recovery. This ran after
  the selector change, before the independent time-anchor change.
- `cargo test -p moosuri-store chat::tests:: --offline`: **53 passed, 2 failed**.
  All direct chat tests, including real admission/claim/retry reference checks,
  passed. That substring filter also selected mission/schedule chat tests. The
  failures are old synthetic downgrade fixtures in mission code outside this
  worker's ownership (listed below), not new time-reference assertions.
- `original_message_reference_anchors_local_midnight_dst_fallback_and_retry`
  checks Seoul crossing UTC midnight, both New York DST-fold offsets, explicit
  UTC fallback and identical stored reference after a real failed-run retry.
- `cargo test -p moosuri-agent runner::tests:: --offline`: **4 passed**. Actual
  encoded requests preserve the trusted reference under all four dialects;
  projection retains final/commentary behavior. Tool/media profile replacement
  uses the tested trusted-context helper.
- `cargo test -p moosuri-agent memory_intent::tests:: --offline`: **9 passed**,
  including both fresh evaluation request forms, quoted/mixed mutation precedence,
  strict literal decision shape, temporal and isolated-request contracts.
- `cargo clippy -p moosuri-store -p moosuri-agent --all-targets --no-deps --offline -- -D warnings`
  and workspace formatting: passed. Strict OpenSpec validation is recorded with
  the frozen handoff. Integrated Linux, live latency and independent review are
  main-owned; classifier-round elimination is established by routing, not a
  production latency measurement.

The broad chat filter failures were:

- `missions::chat::tests::completed_untyped_history_is_not_backfilled_as_fresh_mission_authority`:
  synthetic old-schema reopen returns Unavailable at missions/chat/tests.rs:1959.
- `missions::chat::tests::mission_chat_schema29_rolls_back_late_failure_without_recreating_prior_authority`:
  downgrade fixture fails before Store reopen with SQLite `no such table:
  main.credential_broker_jobs` at missions/chat/tests.rs:492.

The parent owns resolution of those encountered migration fixtures and final
integrated evidence. They were not weakened or modified in this worker.

## Independent routing review follow-up

Independent review found the first read-only gate still accepted mixed Korean
clauses with a verb outside its mutation cue vocabulary, for example
“내 주소를 서울로 변경하고 기억에서 찾아 알려줘”. A follow-up requires a bounded
noun-target case or “얼마나 되는지” question form, rejects nested case targets and
conservatively rejects clause/predicate markers before accepting the shortcut.
Unknown forms, including ordinary nouns that happen to contain a rejected marker,
keep the classifier. This deliberately trades shortcut coverage for preserving
mutation/clarification behavior. The expanded intent fixtures cover 변경하고,
변경 후, 등록한 다음, 바꿔서, 정정해 and a trailing mutation as well as the original
positive retrieval forms. Final focused verification is main-owned after review.

Parent integration, final independent review and actual browser/live evidence are
recorded in the [combined acceptance](2026-11-16-assistant-main.md).
