# Paused mission acceptance revisions — 2026-11-15

## Scope and authority

This bounded F27 unit adds an authenticated explicit form/API save for task, constraints and typed checks of a stable paused one-off mission. Saving queues a fresh plan. Additive Steer remains a separate operation. Original `missions.payload` is unchanged; schema52 stores immutable original/current acceptance snapshots in a tenant/mission sidecar. At most eight edits are admitted. The active server snapshot and revision, not model-written text, govern planning and verification.

The original run, provider/profile/Google account pins, session or durable execution mode, spent model/tool calls, elapsed deadline, expiry and total two-repair limit remain. Recognized new unadmitted mail/Calendar/public-web reads and unsupported send/browser/vault/shell wording are rejected; language detection is conservative and does not prove understanding of arbitrary prose. Existing closed typed tools and current grant checks remain the execution boundary. No provider, dependency, recurring capability, budget extension or external effect was added.

Old run leases are removed even at the saturated attempt counter. A monotonically increasing step floor and acceptance revision fence old checkpoints, planned unentered slots, recovered rounds, receipts, plans, candidates and late callbacks. Accounting and historical step records remain. Current planning ignores old unentered slots so they neither block the new plan nor execute through re-leasing.

The editor sends a direct save without a second approval dialog, preserves fields on error and retains the exact operation UUID/payload for uncertain retries. Known conflicts retire that operation and require reviewing current state. Polling a nonpaused state preserves the visible draft and disables save. History is available in the mission view. The simple form retains existing source-quote/output-contains checks while editing review/exact-number checks.

## Lifecycle and migration

Normal Store opening advances schema51 to52 with strict SQL. Offline restore previously stopped its forward hooks at50 despite current schema51; this change adds the missing51 hook and new52 hook. The real predecessor sealed restore test passes through these hooks. Eighteen historical synthetic downgrade fixture files drop the new sidecar before replaying historical migrations; production migrations were not made permissive.

Explicit mission deletion clears acceptance history. Actual account erasure purges only that account's history. Source withdrawal removes revised mission results and receipts, including idempotent control retry projections. An actual encrypted snapshot/restore retains readable revised history while quarantining unfinished execution and refusing amendment/resume. Recurring occurrence amendments are forbidden.

## Verification evidence

Native checks used the existing Rust1.89.0 aarch64 Darwin cache and preinstalled web dependencies; no installation or provider credentials were used.

- `cargo test --locked -p moosuri-store mission_acceptance_`: 3 passed. Covers schema51 migration, owner isolation/account erasure, pause/scope/recurring restrictions, immutable history/reopen/idempotency, unchanged budgets/deadlines/repair count, wrong-revision/old-receipt/late-lease refusal, and pending local-slot fencing.
- `cargo test --locked -p moosuri-agent mission_acceptance_runtime_`: 1 passed. Actual loopback Runtime replans under revision2, executes a new decimal tool and repairs a wrong candidate with the original spent allowance. Four actual loopback requests occur after revision; two earlier Store-seeded rounds yield six accounted model rounds in total. This is not six actual provider requests or real-model quality certification.
- `cargo test --locked -p moosuri-store mission_owned_search_source_receipt_withdrawal_erases_result_and_retry_receipts`: 1 passed on an amended mission.
- `cargo test --locked -p moosuri-store mission_actual_encrypted_restore_quarantines_unfinished_grants_and_keeps_readable_history`: 1 passed with revised history.
- `cargo test --locked -p moosuri-store supported_predecessor_migrates_offline_and_repeated_restore_keeps_journal_lineage`: 1 passed through actual sealed restore and offline forward migration.
- `cargo clippy --locked -p moosuri-core -p moosuri-store -p moosuri-agent -p moosuri-server --all-targets -- -D warnings`: passed. The existing ts-rs unsupported-serde-attribute diagnostics remain nonfatal.
- `cargo fmt --all`, `git diff --check`: passed.
- `npm run typecheck`, `npm test -- --run src/Missions.test.tsx` (16 passed), `npm run build`: passed. The added UI test exercises field retention, exact lost-acknowledgement retry and successful close.
- `openspec validate revise-mission-acceptance --strict`: passed.

Generated shared API types were exported by the actual Rust type exporter. Build output and the local node_modules symlink are excluded from the patch. Frozen packet manifests record source and evidence hashes. The integrated acceptance below records the later independent review and Linux gates. No production service, database or external account was changed.

## Independent R0 finding and R1 recovery repair

Independent frozen R0 review requested one P1 repair: the acknowledged planning-round recovery branch still used `mission_save_plan`, whose compatibility wrapper fixes acceptance revision1. A revision2 planning response durably committed before plan adoption could therefore fail recovery after restart. R1 changes that branch to `mission_save_plan_at` with the persisted `pending.acceptance_revision`, matching ordinary planning and candidate recovery.

The single added test `mission_acceptance_runtime_restarts_after_acknowledged_revised_plan_without_replanning` exercises that exact gap. It starts with two spent original Store rounds (including one repair), accepts revision2, receives the real counted loopback planning response through the production HTTP parser, records its usage and commits its typed mission round, then closes/reopens Store before adopting the plan. The actual supervisor consumes that stored plan and completes current criteria with one new decimal tool and the remaining repair. One actual planning request occurs before reopen; three later loopback requests complete the run, four total. Accounted model rounds are3 immediately before and after reopen and6 at completion, preserving the two original seeded rounds without repeating planning. The original R0 Runtime test shares its existing fixture with the new gap case.

R1 validation is limited to that one new restart test (1 passed), `cargo clippy --locked -p moosuri-agent --all-targets -- -D warnings`, formatting and diff checks. Previously passed UI/Store/restore suites were not rerun. R0 packet bytes remain unchanged; R1 is supplied as a three-file delta over the frozen R0 source. Independent R1 re-review and root integrated Linux checks/publication remain pending.

## Integrated acceptance

Independent R0 review found the revised planning-recovery branch still used the revision1 compatibility API. R1 passes the saved acceptance revision and adds the exact acknowledged-plan/restart outcome described above. Independent R1 review accepted the frozen fix with no further material findings.

On integrated Linux x86_64 with pinned Rust1.89 and offline public dependencies, the three new Store outcomes, Runtime revision/repair outcome, amended source withdrawal, actual encrypted restore, predecessor forward restore, and actual HTTP owner/edit/replay path passed. The three current-generation evidence-calculation checks also passed after integration; generation3 and the existing Google preview history exclusions remain. Four-package Clippy and the embedded assistant build passed. R1 reran only its new exact-gap Runtime outcome, agent Clippy and the final embedded build; all passed. The built Linux exporter exactly matches the committed shared TypeScript declarations. The unchanged tested React build is embedded, and its existing16-case/type/build evidence was retained without another suite run.

The source archives and affected product hashes were recorded privately. These are synthetic storage/runtime/HTTP and platform results, not real-model intent quality, recurring criteria editing, external write support or production deployment. CI remains disabled.
