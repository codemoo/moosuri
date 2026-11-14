# Explicit forward recovery allowance

Schema49 adds one stopped operator allocation and permanent SQLite dispatch
commitments. It does not reconcile the immutable historical accounting gap,
restore billing totals, initialize unknown usage to zero or refund unknown calls.
Allowances limit new inference request count and validated STT input duration;
existing output/response/time and six-model/twelve-tool per-run bounds remain.
The SIWC transport does not provide a provider token ceiling.

The joined source contains the separately accepted historical restore baseline.
Budget dispatch guards replace its blanket spend guard; historical usage guards
remain. Any ancestral unknown receipt stays effective. A grant matches only the
current installation/restore ID, so another historical or sealed restore cannot
reuse copied grants. Allocation uses original Store writer admission; a live
server rejects it. Repeated exact request UUID/caps are idempotent.

Chat/agent/memory/STT commitments share their existing dispatch transaction.
Capability probes reserve each real round after exact binding/authority checks;
a two-round tool probe consumes two. Catalog/auth/token refresh do not consume
inference requests. Identity-only binding/heartbeat validation lets the last
permitted call finish. No duplicate reservation authorizes replay. All uncertain
outcomes consume allowance permanently.

Account purge explicitly exempts immutable forward commitments, like existing
model-call receipt lower bounds. They contain only opaque IDs/nonce, kind,
timestamp and duration; no content, name/email, credential, provider or destination.
A real charged-account deletion and latest-journal replay prove that retained
accounting does not prevent private tenant purge or restoration.

## Actual selected evidence

- Native macOS arm64 joined app gate: `cargo test --locked -p moosuri --features
  test-fixtures --test recovery_budget -- --nocapture`, **1 passed /9.74 s**.
  `/private/tmp/recovery-budget-joined-final.log`.
- The app gate uses actual encrypted export/journal/historical restore, stopped
  CLI allocation, real local HTTP probes/STT and Runtime execution. It checks
  blocked fresh credentials; allocation idempotence, changed caps/wrong restore
  ID/live writer rejection; shared text1/chat1/tool-probe2/memory1/audio-probe1/STT1;
  concurrent final-slot admission; no refund after held request/shutdown;
  charged owner deletion with completed purge; matching restore retry preserving
  nine commitments; another historical restore replaying latest deletion and
  rejecting imported allowance under its new identity. Original source stays usable.
- Actual personal SIWC signed fixture/service+runner:
  `cargo test --locked -p moosuri-agent --features test-fixtures
  historical_forward_personal_proof_and_final_chat_use_new_allowance_without_clearing_unknown
  -- --nocapture`, **1 passed /3.06 s**. Registration/catalog consume no inference
  slots; new credentials alone stay blocked; explicit grant permits proof1 and
  final chat1 to complete; a third call rejects and old usage remains unavailable.
  `/private/tmp/recovery-budget-personal.log`.
- React typecheck passed: `/private/tmp/recovery-budget-web-typecheck.log`.
- Existing historical rotation/deletion/source-continuity regression passed:
  `cargo test --locked -p moosuri-store --features test-fixtures
  historical_export_latest_deletes_rotation_and_source_continuity -- --nocapture`,
  **1 passed /8.01 s**, preserving unavailable historical usage while using the
  new distinct dispatch-required error. `/private/tmp/recovery-budget-historical-regression.log`.
- Final React build passed: `/private/tmp/recovery-budget-web-build.log` (existing
  bundle-size warning only). `git diff --check` and edited Rust rustfmt passed.

The final selected gates use only synthetic accounts, credentials, encrypted data
and local fixture transports. No real provider calls, private source, legacy DB,
collector/cron or service setting was touched. CI remains off. Independent root review accepted R2 after the account-purge exemption and
indexed identity-only heartbeat repair. Joined Linux x86_64 actual recovery/CLI/
HTTP/Runtime gate (1), signed personal SIWC gate (1) and selected historical
regression (1) passed. Clippy, the default embedded React build, exact generated
TypeScript comparison and plan verification passed; the integrated React bundle
also built. These synthetic/native/Linux results are not official arm64/release
or production recovery acceptance. There is no automatic/off-host
journal publisher or independent usage ledger in this change.

R2 independent-review performance repair: binding/heartbeat admission now checks
only historical/current restore identity and indexed allocation existence; it no
longer aggregates retained commitments on each heartbeat. Unpublished schema49
adds indexes for current allocations and allocation-linked commitments. No native
gate was repeated for this narrow query/index change; existing joined Linux gates
remain the integration verification.
