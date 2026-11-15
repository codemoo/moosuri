# Personal foreground transport and refresh admission — 2026-11-15

The source audit found two concrete defects. Personal answer/tool continuations
and isolated planners rebuilt `ModelClient` after every durable readiness check,
discarding its validated HTTP pool. OAuth discovery failure after refresh
acquisition also left a 45-second admission behind; the next readiness attempt
stopped after 35 seconds even though no rotating token had been sent.

The change retains one explicit run-local transport lineage and replaces only
its bearer/capabilities after the same durable readiness checks. Every request
still validates fresh DNS, origin and addresses. Each request sets its own
sensitive credential header; no default credential header, owner-global pool or
service token cache is introduced. Independent clients and changed limits retain
separate caches. Model, effort, durable step admission, tool/write approvals and
inference retry behavior are unchanged.

Discovery failure/drop, including a lost actor acquisition acknowledgement,
releases only the matching acquired revision, generation and nonce. Cleanup is
carried by the actor result before delivery. Full-mailbox waiting keeps only a
weak sender across suspension so final Store shutdown can close and join. A dispatched exchange cannot be reset by cleanup. Runtime/process loss
continues to use existing acquired expiry and restart recovery; no schema changes
or new live-readiness claim is made.

## Verification

All provider/account inputs are synthetic. Network fixtures use loopback HTTP.
No production services, existing private data, live account or model calls are
involved. Native and integrated verification results are recorded below when
finished. Loopback peer observations establish connection reuse; they do not
measure production TLS handshake time or end-to-end user latency.

- `cargo test -p moosuri-connectors model::transport::tests:: --offline`:
  **29 passed**. This includes observed same-peer reuse across unchanged and
  rotated personal bearer, independent-client socket isolation and fresh policy
  denial despite an existing pool.
- `cargo test -p moosuri-agent subscriptions::tests:: --offline`: initial run
  **37 passed, 2 failed** in historical artifact-write fixture assertions. A
  separate frozen-base checkout reproduced the same failure: the fixture rejected
  external-report reads that the current document grant already declares. The
  fixture now admits only the exact supported read names, still forbids effects
  and browser tools, and injects an undeclared draft call for the rejection case.
  The targeted artifact-write suite then passed **6/6**, including both failures.
- The subscription run's new cases passed: actual planner/tool peer reuse through
  unchanged and rotated authorization; cancellation, disconnection and account
  epoch fencing; discovery failure/drop with a retry completing within three
  seconds; stale-nonce and dispatched-state cleanup protection. Existing signed
  refresh rotation-loss, temporary rejection, late commit and owner-isolation
  cases also passed.
- The added personal terminal-phase integration test failed against the old
  completion logic (commentary-only was incorrectly Completed), then passed all
  five stream variants with the separately integrated terminal-phase correction.
  Commentary and late commentary remain stored but produce Incomplete with
  `protocol_failed`; explicit final, late final and unphased answers complete.
- Affected-crate Clippy with `--all-targets --no-deps -- -D warnings`, workspace
  formatting and strict validation of `preserve-personal-round-transport` passed.
  The broader Clippy invocation also linted a dependency and stopped at existing
  credential-broker `local_browser` formatting and `select_gitea` argument-count
  lints; those unrelated files are outside this change.
- Integrated Linux passed all 40 subscription and 29 transport cases; later
  acquisition-ack cancellation and saturated-mailbox shutdown checks also passed.
  Independent review accepted the revised lifecycle and current authority fences.
  See the [integrated review](2026-11-15-assistant-core-recovery.md). No production
  latency measurement or live TLS/provider acceptance is claimed.
