# Committed mission round lifecycle — 2026-11-16

Baseline: `0bf25532be04dde77f4dccf8d945716e15ca0187`.
Scope: supported mission driver and an exact guarded Store instruction read.
All authored evidence is synthetic; no live provider, account, credential, legacy
operating data, service, cron or CI configuration is used.

## Evidence and changes

- `mission_verify_at` rejects a typed blank final as `InvalidInput`. Both old
  fresh/replay branches silently returned, leaving the admitted run active.
- A verified recovered planning capsule with `steps=[]` similarly failed Store's
  plan validation without settling the run.
- Fresh malformed empty-call guided output failed before acknowledging guidance.
  Recovery instead left the assigned receipt received, cleared the proposal and
  proceeded to inference without including that assigned guidance.

`runner/mission/round.rs` now handles fresh and recovered committed rounds once.
It acknowledges only shaped guided output, admits initial/legacy plans, reuses
saved results, executes pending local/native tools, rechecks late guidance before
input-wait failure and verifies final proposals through the same guarded path.
Model transport/terminal admission and legacy unverified-capsule rejection remain
in the driver. Fresh call order is preserved; recovery keeps saved slot order.

`step_has_run_instructions` uses the guarded lease plus exact execution attempt,
dispatch step and received state. It changes no schema, DTO or receipt state.
Local validation/budget/storage failures use existing failure categories. Intentional
owner/epoch/cancel/acceptance fences remain stops and cannot overwrite new work.
The existing source/profile checks, budget, exactly-once receipts, uncertain effects,
owner review, report permission and final mail approval remain authoritative.

## Verification checkpoint

Source/test freeze: `c85604fe758e24f7211f73aaf3d3fc28c152317a`.

Locked/offline checks passed with the shared isolated Rust1.89 environment after
touching this checkout's tracked Rust/Cargo inputs:

- `cargo test --locked --offline -p moosuri-agent tests::missions::rounds`:3 passed.
  Includes paired fresh/reopen blank final, recovered empty plan and guided malformed
  output, plus exact unassigned/assigned/applied/stale-step receipt checks.
- `cargo test --locked --offline -p moosuri-agent tests::missions`:19 passed,
  including the new cases, successful saved-tool/final recovery, spent-budget
  acceptance repair, terminal/unverified-capsule fences and personal-model scope.
- `cargo test --locked --offline -p moosuri-agent run_instructions`:1 passed,
  exercising held live guidance, stored receipts and invalid stream outcomes.
- `cargo test --locked --offline -p moosuri-agent connected_missions`:5 passed,
  retaining input/source continuation, connection withdrawal, reports and narrow
  mail/Calendar effect behavior without sending or approval-dialog expansion.
- `cargo clippy --locked --offline --no-deps -p moosuri-agent -p moosuri-store
  --all-targets -- -D warnings`:passed. This is scoped lint, not a workspace claim.
- Direct targeted rustfmt and `git diff --check`:passed.

Filters overlap and are not summed into a quality score. The first compile found
an owned value moved in a pattern guard; borrowing that pending-round field fixed
it before product tests ran. No runtime test failure was concealed. The late
input-wait receipt check is shared with fresh execution and guarded in Store;
these fixtures do not deterministically schedule every possible receipt interleaving.
Independent frozen source review found no further material defect in fresh/recovery
guidance, local/native tools, or cancellation/acceptance/epoch fences. Root-owned
integration results are recorded in [household acceptance](2026-11-16-household-assistant.md). These synthetic
results do not establish live provider/device acceptance or broad assistant quality.
