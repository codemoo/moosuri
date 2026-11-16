# Original-conversation task handoff events — 2026-11-16

Chat-origin missions could finish with no received guidance and leave their
original conversation advertising an active instruction target. The next text
would be submitted to the finished carrier and rejected until refresh.

The Store now publishes changed original targets from lifecycle writers through
one tenant/account-scoped helper. It covers ordinary terminal transitions,
direct mission verification/tool failure/control/expiry, bounded input and
connection waits/resume, native wait publication, model-dispatch capacity and
source withdrawal. It does not change tasks, model selection, permission,
approvals, budgets or external execution. Deleted/quarantined targets disappear.

Comparison uses the latest durable state/turn projection. Receipt-only state
updates and nested lifecycle hooks cannot duplicate an unchanged target; a
pruned projection gets one replacement at the next affected lifecycle mutation.
SSE reads are unchanged and read-only. Existing Runtime signals and the server's
250ms event-read fallback deliver committed changes to an open conversation.
This does not add Web Push or establish live-provider acceptance.

## Synthetic evidence

The scoped Store regressions seed a typed admission receipt, then exercise real
Store lifecycle methods and consume durable `chat_events` after the pre-action
cursor. Fresh conversation snapshots are not the outcome oracle. They cover
terminal zero/all-applied guidance followed by ordinary next-turn admission,
input wait/resume on the same carrier/current attempt, cancellation and replay,
model-call capacity, target deletion after event retention loss, and unchanged
polling. Provider/model IO, production services and private operating data are
not used.

The three new tests failed on unchanged baseline `9d72356` because the original
conversation received no eligibility event (zero-guidance completion, input wait
and model capacity). The candidate passed all three tests, including the
all-applied-guidance branch, resumed attempt, cancellation replay, deletion after
retention loss and ordinary next-turn admission.

`cargo test --locked -p moosuri-store missions:: -- --nocapture` passed123 of133
selected tests. All10 failures were reproduced with their exact names against
unchanged baseline production files: seven old-schema downgrade/reopen fixtures
returned `Unavailable`; three unsupported-effect fixtures rejected admission
with `Forbidden`. These are retained baseline failures, not a passing suite or
new handoff regressions. No unrelated fixture or authorization changes were made.

Scoped strict Clippy passed with
`cargo clippy --locked -p moosuri-store --all-targets --no-deps -- -D warnings`.
The initial invocation without `--no-deps` stopped in unchanged credential-broker
code (`local_browser.rs:673`, suspicious else formatting;
`policy.rs:121`, too many arguments). Existing ts-rs serde-attribute notices
remain. Source was formatted with Rust1.89 rustfmt.

Independent frozen source review found no material issue in ownership,
visibility, retention, event cursors or the covered lifecycle hooks. Integrated
repository and public-source Linux gates are recorded in the
[coordinator review](2026-11-16-coordinator-routing.md). No actual browser run is claimed.
