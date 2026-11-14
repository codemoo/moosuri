# Vault off-host journal publication evidence

Bounded P8/F51 implementation based ond7ec6a3. No DB schema, dependency, CI, legacy/private deployment, web surface or infrastructure change. Existing live source capture and historical MVJC format remain authoritative. No recovery identity or plaintext vault secret is generated/read by source worker or receiver.

Source worker shares the exact Arc<Mutex<Database>> used by HTTP, captures actual SQLite/journal, ends the mutex guard, then verifies frozen semantics and dispatches fixed pinned OpenSSH argv. Receiver independently pins source/baseline/recipient, validates complete typed chain and exact old-byte-prefix SHA256, streams immediately into age, fsyncs an immutable generation and atomically publishes its linked ledger receipt. It retains no raw journal validation proof files at rest. Existing historical restore still independently verifies semantic replay and exact baseline prefix.

Passed native checks:

- Publisher library8: encrypted-only generation contents; monotonic extension; ACK loss after durable receipt returns original checkpoint/full receipt; stale/fork/foreign/malformed/truncated/extra-tail/recipient/baseline rejection; corrupt current archive/ledger fail-closed; interrupted pre-receipt generation/orphan handling; private/absent/occupied/lock/source admission; fixed argv injection rejection; exact ACK checks; bounded blocked-process deadline and cancellation; worker stop before first capture; exact shared writer released during stalled transfer and subsequent append/reopen preserved.
- Publisher CLI flags1: opt-in complete source settings and receiver commands without a source data directory.
- Actual binary receiver/status/historical integration1: explicitly initialized isolated HOME/roots; repeated receiver request retains same checkpoint; independently queried exact head pin verifies retained bytes; source/local journal removed from active paths before historical restore; fresh destination accepts new work; receiver generation retains only journal.age/head.json and no identity/raw journal.
- Existing historical unit rejects wrong pin, old/fork/foreign/truncated/impossible transcript and corrupt ciphertext. Existing historical CLI requires a pin. Existing consequential historical integration retains rotation/deletions/nonce reservations/receipts and fresh work. The new publisher historical roundtrip also passed in that filtered command.
- moosuri-vault-service all-target Clippy with warnings denied passed; git diff whitespace checks passed.

Synthetic fixtures only: generated test identities, random temporary source/receiver roots, counted local process commands and actual receiver CLI. No real SSH target, provider, credential store, old service, external write, deployment or physical crash test. Loopback/synthetic semantics do not establish off-host failure-domain separation, authorized_keys restrictions, host-key setup, device readiness or hardware durability.

Bounds:1GiB full journal,512MiB captured DB,16KiB request/head/receipt, existing32MiB intent bound,100000 accepted generations and30..604800second post-attempt interval. Source preparation (capture, journal scan/replay, table comparisons and digesting) is file-size bounded but has no overall elapsed-time limit or cancellation guarantee. Partial checks in existing capture do not bound the complete preparation. Worker shutdown waits for that admitted preparation to drain and can wait for an extended period. Cancellable/deadline-checked preparation remains a future operational improvement. The60second deadline and cancellation cover SSH transfer only, after preparation. Receiver streaming has separate60second checked processing and a65second receiver CLI watchdog; filesystem stalls/fsync are not hard realtime. Recipient/baseline rebasing, retention/compaction and zero-loss synchronous remote write admission are outside this unit.

Independent review's sole P2 identified the earlier inaccurate preparation deadline/cancellation wording. R1 adopts the existing draining contract explicitly in operator docs/design/evidence. Runtime and all previously tested source hashes remain unchanged; no Cargo rerun was performed for this documentation-only correction.

Status proves only the independently retained receiver's accepted boundary. Periodic gaps, later deletions/nonce use, whole-target/all-pin rollback and independently runnable source/restored copies remain explicit limitations. Actual operator deployment remains pending.

Root integrated the frozen change with the current v1 vault import and built web
assets. All14 R1 file hashes matched before common documentation updates.
Independent review accepted the R1 draining contract; runtime/test hashes remained
identical to R0. The pinned offline Linux source capsule passed publisher library8,
CLI flags1 and actual receiver/status/historical roundtrip1; the historical filter
also passed its existing unit/CLI/consequential rotation-deletion-nonce checks
(one roundtrip overlaps those filters). All-target vault-service Clippy with
warnings denied and the default embedded vault-service build passed. The Linux
capsule preceded only the R1 documentation clarification; those successful runtime
checks were not repeated for unchanged code. This evidence does not exercise a
real remote SSH target or alter a deployed vault.
