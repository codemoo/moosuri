# Durable text chat verification — 2026-11-12

This unit connects the published model/account foundation to a real plain-text
conversation path. All acceptance inputs, accounts and model responses are synthetic.
No production model credential, collector, cron, service or database was used.

## Evidence

- Rust1.89 workspace:93 tests pass on macOS and Linux x86_64, including Store22,
  runtime12, connector45, HTTP13 and CLI installation1. The child-process writer
  test is counted once. React48 tests, typecheck and production build pass. Final assets were rebuilt
  into the Linux CLI and its process installation test was repeated successfully.
- Formatting and workspace/all-target Clippy with warnings denied pass.
- The final generated TypeScript contract matches Rust. OpenSpec strict validation
  passes11 items; plan metadata retains28 requirements,69 features and67 planned
  evaluation groups. This is not a product evaluation score.
- The232 registry dependencies are unchanged from the provider unit's OSV query;
  no new third-party package was added. CI remains disabled.
- Actual Chromium against the Rust server and a dedicated local streaming fixture:
  first-send thread creation, partial reply, failure→retry preserving an unsent draft,
  cancellation, reload while streaming, saved output without duplicate questions,
  server restart with session intact and automatic first-prompt titles.
- A390×844 viewport has no horizontal/document overflow, with conversation scrolling
  inside the app. Screenshots were inspected. Real iOS/Android devices are untested.

## Failure boundaries

New file-backed Store tests cover request UUID/digest idempotency, tenant separation,
retry/attempt fences, deletion and receipt settlement, restart classification without
replay, phase metadata, event retention/reset, history pagination, queue fairness and
bounds, provider/session revocation, migration3 and successful whole-turn context.

Actual loopback model tests exercise partial output, browser subscriber disconnect,
protocol failure, truncation, refusal, token limit, different model identity, cancellation,
deletion, logout, runtime drop, shutdown/reopen, large Unicode appends and missing final
checkpoint settlement. Timeout uses Tokio's advanced clock with a held real HTTP
response; it does not wait180 seconds or call a paid provider. Connection failure must
release the account's execution slot rather than strand a running turn.

HTTP acceptance covers duplicate POST acknowledgement, private fetch SSE, disconnect
independence, cancel/delete versus late results, observed usage after deletion and the
wire-size difference between escaped JSON and decoded text. CLI acceptance holds an
incomplete raw TCP request after HTTP100 Continue, sends SIGTERM and checks bounded
shutdown, then restarts with the existing session intact.

## Independent review and corrections

Separate reviewers inspected Store, runtime, HTTP/SSE and process boundaries. Corrected
issues were re-reviewed:

1. Runtime transport failures produced safe unknown outcomes that Store originally
   rejected, leaving runs active. Allowed terminal combinations now agree, and real
   timeout/truncated/failed-connection scenarios verify capacity recovery.
2. Oversized context originally skipped a recent turn and restored older history.
   Selection now stops at the boundary, retaining a contiguous suffix.
3. Transient writer pressure was classified as revoked authority. Bounded retries now
   distinguish infrastructure errors; a supervisor also settles exited workers whose
   terminal checkpoint failed, without replaying their provider call.
4. HTTP graceful shutdown could drain indefinitely. The CLI now bounds HTTP drain
   independently of runtime cancellation; the stalled-body regression is explicit.
5. A16 KiB JSON cap rejected valid escaped8 KiB prompts. Only the chat envelope permits
   up to64 KiB while the decoded Store limit remains8,192 bytes.

6. A cursor-less duplicate POST snapshot could double-append delayed SSE text. POST
   acknowledgement now refreshes a same-snapshot cursor before continuing the stream.
7. Explicit phase clearing was ignored. `Phase(null)` now clears metadata while a
   delta's unspecified phase continues to inherit the current value.
8. A quiet stream displayed connecting until the next message. Authenticated response
   headers now signal connection readiness even when no new content arrives.

Reviewers inspected source and regression cases; the main ran integrated acceptance.
This is not a formal security audit or a live-model performance/quality benchmark.

## Limits

This is a text-chat slice, not completed P0/P1/P2 or the full assistant. It has no tools,
source retrieval, personal memory, media, scheduling, subscriptions or user-key vault.
Reported usage is not a price or hard spending budget. Remote model acceptance cannot
be undone by cancelling locally. Already-sent network bytes cannot be recalled after
delete, and old filesystem snapshots/WAL are outside logical deletion guarantees.

Auth-journal recovery, supported backup/restore, deployment packaging, live-provider
capability/latency tests and real mobile-device acceptance remain separate work.
