# Acknowledged mission recovery before remote readiness

Base `6892673`; source fix `5cb5685`. Synthetic SQLite, account, signed OAuth and
loopback personal model streams only. No production data/services, live provider,
external send, Push, deployment or CI was used.

## Consequential defect and result

A restarted mission with an acknowledged completed final proposal entered personal
model-client/token readiness before consuming the durable proposal. With an
expired access token and temporary discovery failure, the real Runtime marked the
mission `failed`, discarding local completion even though its earlier exact
calculation receipt and validated final model stream were already stored.

The driver now consumes its guarded verified capsule/local receipts first. Only
unfinished work that still needs inference constructs the model client and enters
remote token readiness. The per-round readiness and guarded model-dispatch path
remain. Current Store lease, owner/session, binding, source, deadline and external
effect guards still control every recovered operation. No result is re-created,
budget reset, provider fallback or automatic unknown-effect replay is introduced.

## RED/GREEN evidence

- New actual Runtime regression failed before the change: the temporary-discovery
  case returned `failed` instead of `achieved` (0.74s test runtime).
- After the ordering change, the regression passed all three synthetic boundaries
  (2.44s): temporary discovery outage after token expiry completes the original
  mission; disconnection and revocation of the original current-session login
  still block success. The final proposal came from the actual personal protocol
  parser and was committed before reopening the Store. No new inference or token
  refresh occurs and both original dispatch receipts remain.
- At frozen `5cb5685`, `cargo test --locked --offline -p moosuri-agent
  subscriptions::tests::missions_tests:: -- --nocapture` passed **4/4** (8.28s):
  this recovery test, actual direct/recurring personal execution, delegated
  connected reads/withdrawal and cancellation/restart/current-session fences.
- `cargo test --locked --offline -p moosuri-agent tests::missions:: --
  --nocapture` passed **10/10** (24.74s), covering stable tool/final recovery,
  report persistence, recurring execution, revised criteria, incomplete/refused
  terminals and unverified legacy capsule refusal.
- Formatting of both affected Rust files and `git diff --check` passed. Existing
  ts-rs `deny_unknown_fields` parsing warnings appeared during compilation; no new
  compiler failure remained. Parent integration owns lint and OpenSpec checks.

## Scope and limits

The bounded inspection also read ordinary retry/checkpoint ordering and original
missing-input continuation paths. This one reproduced defect was repaired; the
review does not claim all assistant defects are known. Loopback evidence does not
measure provider acceptance, semantic model quality or production latency. Parent
integration owns independent review, pinned OpenSpec validation and combined
checks/publication.

## Explicit controls of the preceding fixed reminder

Source fix `93e36c7` follows a separate real-model observation supplied by the
parent: creating a fixed reminder at15:00 succeeded locally, but `방금 그 알림을
내일 오후 4시로 옮겨줘.` entered mission interpretation and the following cancel
could not identify it. Korean move imperatives were missing and the deictic noun
phrase was being treated as a title.

The parser now composes the existing action/time grammar with a distinct previous
receipt target for `그 알림` / `방금 그 알림`. Without explicit selection, only the
immediately preceding completed actual reminder receipt in the same
owner/thread/login/authentication epoch within thirty minutes can supply an ID.
The current reminder must still be owned, unrevoked and at the exact receipt
revision. A separate selected domain blocks implicit reminder resolution. Missing
or stale evidence asks for exact name/selection; it never falls back to a global
latest or sole reminder. Existing named/generic controls and atomic write,
confirmation and UUID replay behavior remain.

- Actual baseline Runtime RED: with only production parser/target code restored
  to the previous HEAD, the new test returned `Failed` / `provider_unavailable`
  rather than `Completed` (0.65s). The loopback503 endpoint caught unwanted model
  interpretation of the move request.
- Fixed `cargo test --locked --offline -p moosuri-agent tests::mission_chat:: --
  --nocapture`: **7/7**, 7.19s. For both references, the real Runtime creates the
  original15:00 reminder, edits that same ID to16:00, replays the exact UUID without
  another revision and revokes that same ID. An unrelated existing reminder
  remains; no duplicate, mission or model call occurs. Existing natural mission,
  calendar, missing-clock, restart and ordinary-chat routing tests also pass.
- `cargo test --locked --offline -p moosuri-store schedules::chat:: --
  --nocapture`: **21/21**, 18.08s. The added negative test covers another thread,
  a new same-owner login, password epoch change, out-of-band revision, an
  intervening completed ordinary turn and an expired receipt. Every case retains
  a sole owned reminder unchanged and has no claimable model run. Existing
  attachment, exact-title, owner/revision, rollback, draft/restart and scheduled
  mission boundaries pass.
- Rust formatting and `git diff --check` pass. Only existing ts-rs attribute
  warnings appeared. Synthetic accounts, local SQLite and loopback only; parent
  owns independent review, fresh real-model recheck, combined lint/OpenSpec
  validation and publication. No live provider, external send or Push was used
  by this worker.
