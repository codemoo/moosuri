# Provider connection guidance continuity — 2026-11-16

Provider-connection wait promotion advanced `chat_runs.attempt` directly. Unlike
mission restart and bounded input/control resume, it did not transfer instruction
`execution_attempt`. Both resumed guidance readers filter the current attempt,
so received and already-applied owner guidance silently disappeared from the
reconstructed model context even though its receipt remained visible.

Connection promotion now restores queued state and invokes the existing stable
mission requeue helper inside its writer transaction. Original receipt request
IDs, text and admission attempts remain immutable. Execution ownership follows
the same bounded carrier attempt, including attempt32 lease retirement. Existing
connection selection, owner/binding checks and origin-target events remain.
Model/profile, deadline, checkpoint and model/tool/repair budgets are not reset.

This is a documented foundation-only boundary: `mission_wait_for_connection`
currently has Store-test callers, with no agent/server yielding caller. The
change does not establish automatic provider reconnect, a new native path or an
ordinary browser workflow. Native model-mail profiles do not admit live guidance
and are outside this patch. All test connections and messages are synthetic;
no real account, provider request, model inference or production data is used.

## Verification

`cargo test --locked -p moosuri-store connection_guidance -- --nocapture`
ran the same two tests against unchanged baseline `dd443d0` and the candidate.
Both baseline tests failed at attempt1: the pending/applied reader returned zero
receipts rather than the accepted receipt. Both candidate tests passed, each at
attempt1 and attempt32. They use real completed external-read receipts, explicit
wait/selection/promotion, unchanged execution snapshots, next-step application,
retired leases and cancellation of later guidance. The reader output consumed by
the mission runner is the oracle; a refreshed mission card alone does not prove
continuity.

Six exact existing Store checks passed: combined completed-read wait/resume,
current-acceptance wait promotion, oldest-ready selection behind a paused task,
wait expiry/binding withdrawal, ordinary pause/resume budget/CAS, and stable
controls crossing attempt32. No broad workspace suite was rerun.

`cargo clippy --locked -p moosuri-store --all-targets --no-deps -- -D warnings`
passed. Existing ts-rs serde-attribute notices remain. The new test module was
formatted with Rust1.89 rustfmt; `git diff --check` passed. All raw command logs
remain outside Git.

Independent frozen review found no material issue in owner/grant/current-connection
checks, retained instructions, budget preservation or retired leases. Integrated
and public-source acceptance are recorded in the
[continuation review](2026-11-16-continuation-integrity.md).
