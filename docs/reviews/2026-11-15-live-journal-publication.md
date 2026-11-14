# Optional live journal publication — bounded local evidence

The optional source worker captures complete applied auth/predecessor/destructive
journals through the existing single writer and periodically invokes a separately
operated Rust CLI receiver over a fixed OpenSSH command. Receiver verifies baseline,
lineage and byte-prefix extension, encrypts validated journals with the public age
recipient and commits an independent immutable acceptance ledger. Existing
historical restore consumes its head/ciphertext without source-local state.

This is periodic publication with an explicit recovery point gap, not zero-loss
latest-deletion recovery. No real off-host target, credentials, production source,
model/provider/mail calls or existing services/collectors/cron were used. CI stays
off. Source remains active-capable, and ordinary writes do not wait for receiver
approval or availability. Receiver metadata proof files are private raw opaque
journals, not falsely claimed age-encrypted at rest. The threat/trust/RPO and setup
contract is [live journal publication](../live-journal-publication.md).

## Exact selected native gates

On2026-11-15, pinned Rust1.89, schema48 plus accepted historical restore baseline:

- `cargo test --locked -p moosuri-store live_capture_receiver -- --nocapture`:1 passed,5.13s. Real Store with two tenants, retained encrypted baseline, concurrent writer capture/forget, before/after complete journal state and unchanged checkpoint idempotence. Receiver fault injection at before-generation, after-generation, before-receipt and after-receipt boundaries verifies only durable receipts advance latest. Retry after receipt/ACK-loss returns the same generation; stale/corrupt/truncated frames and a valid real-Store fork reject. Public recipient rotation needs no receiver private key. After deleting the synthetic source root, target-only inputs plus independently retained old/new identities restore with withdrawal and unrelated-tenant content intact.
- `cargo test --locked -p moosuri --test live_recovery -- --nocapture`:1 passed,3.48s. Actual CLI target-init, receiver children through bounded pipes, receiver restart/idempotent retry, current-head query and malformed transfer rejection. It then starts the actual `serve` worker with a local test-only PATH transport shim executing the actual receiver CLI, observes `accepted_at_checkpoint` without advancing unchanged journal generation, restarts with a failing shim, observes `not_protected`, and verifies bounded graceful shutdown. These are local Runtime/process/pipe/file checks, not evidence of successful OpenSSH deployment or independent physical failure domains.
- `cargo test --locked -p moosuri --bin moosuri fixed_ssh_command -- --nocapture`:1 passed. Strict alias/ID validation rejects spaces, shell metacharacters, option injection, paths and user@host syntax; command argv has the fixed receiver command and strict host-key/noninteractive options.
- `git diff --check`:passed.

Fresh checkout initially lacked embedded React assets, and `serve` correctly
refused to start. The test used the already-built public integration `web/dist`
(ignored build output), then reran the existing build script. No production guard
was bypassed and no frontend behavior change is claimed.

The four failure points above are injected error returns, not abrupt kill tests.
Multiple real receiver child invocations verify fresh-process state reuse. Broader
OS crash/power-loss, stalled/proxy transport descendants, real SSH host-key/forced
command deployment, large-journal capture latency and remote storage durability
remain unverified. Byte/file caps and checked deadlines do not promise a hard
wall-time maximum against an OS IO stall. The reported seconds are selected test
runtimes, not capture latency or a performance claim.

## Source and integration boundary

R0 own-delta source/doc allowlist and SHA256 are in local tooling
`/private/tmp/moosuri-live-recovery-r0-freeze-20261115.json`. That is a review artifact,
not a runtime recovery input. The source freeze contains no DB schema/lib/journal
mutation: new backup/live and CLI modules reuse historical manifest/head codecs,
archive writer, file safety and native no-overwrite publication through narrow
visibility changes. Root owns joined Linux verification, independent review,
accounting-budget integration, global status/public-release updates and acceptance.
At this record's creation those joined/review gates remain pending; aggregate
ongoing/latest-deletion recovery is not marked complete.

## R1 independent review repair

Independent review found a crash window in receipt hard-link/unlink publication:
a process loss between those calls could leave the accepted receipt with two hard
links, causing strict single-link file admission to reject every later query/retry.
Receipt publication now uses the same native atomic no-overwrite rename as generation
directories, followed by ledger fsync. This guarantees one published receipt name
without an intervening two-link state and still refuses replacing accepted receipts.
The focused retry gate adds an explicit accepted-receipt link-count1 assertion.
Source-equivalent joined Linux revalidation of this narrow repair is pending the
integration owner; the native gates above precede this two-file repair.

## Root joined acceptance

Independent root review accepted R1 after replacing the ledger hard-link/unlink
publication with same-parent no-overwrite rename and explicit ledger fsync. This
removes the two-link crash window that prevented current-ledger reads and retries.
Linux x86_64 on integrated schema49 passed actual live Store capture/receiver/
source-free restore (1), CLI receiver and serve-worker local transport/outage (1),
fixed SSH argument gate (1), Clippy, the default embedded-web build and plan check.
The affected R1 ACK-loss gate also confirms a single-link accepted receipt. No
unrelated suite was repeated. Transport uses a local fixture shim; real SSH
forced-command deployment, abrupt process kill and power-loss acceptance remain
not run. No production service, collector, cron, database or SSH setting changed.
