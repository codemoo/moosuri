# Stopped predecessor-schema update review

Scope: P10 local pinned bundle update, original writer-lock handoff to product Store migration, durable activation/manager recovery. This slice adds no downloader, release key, service action or migration SQL. Ordinary stopped export/cutover retain their exact-schema boundary.

## Implemented boundary

- New signed target executable proves its bytes against the verified staged target before source export/migration. Old signed schema must match the stopped source; this build accepts current/immediately preceding schema only.
- Runtime-exclusive admission and the original data writer lock cover snapshot validation, source-schema encrypted export, durable pending/migration-start intent, Store migration and activation.
- The guard duplicates the original locked open description into Store admission and retains its own descriptor on success/failure. The installer does not unlock/reopen between source export and migration.
- Cross-schema activation refreshes manager, its managed hash, then current selection, before retiring pending. The retained signed target bundle remains the recovery entry point if the old manager cannot read new state.
- Recover-new reruns Store idempotently. Choose-old is refused after migration-start or schema advance; no database/backup copyback occurs. Same-schema binary selection recovery remains supported.

## Selected gates and evidence

R0 source snapshot on 2026-11-15: direct Rust 1.89 rustfmt passed for edited Rust files; `git diff --check` passed. R0 native macOS arm64 compilation and `cargo test --locked -p moosuri --features test-fixtures --test local_bundle --test schema_update` exited 0: existing same-schema lifecycle **1 passed** in 62.21 s; actual predecessor lifecycle **1 explicitly ignored / not run**. This local result is not a Linux/arm64 release acceptance claim. Log: `/private/tmp/schema-updates-native.log`.

Independent R0 review found that direct create/write of initial `pending.json` could expose a truncated public marker before migration. R1 replaces it with absent-checked private temporary-file publication under exclusive admission. A test-fixtures-only partial-write regression in the affected same-schema lifecycle checks that the old manager can still start, the export remains and a fresh-output retry succeeds. Independent R1 review accepted the repair with no remaining P1/P2 findings. Root Linux execution is recorded below.

Meaningful execution gates are:

1. `cargo test --locked -p moosuri --features test-fixtures --test local_bundle` for same-schema compatibility and runtime/data lock behavior.
2. With a real pinned previous-schema executable and embedded React assets, `MOOSURI_TEST_PREDECESSOR_BINARY=/absolute/path/to/previous/moosuri cargo test --locked -p moosuri --features test-fixtures --test schema_update -- --ignored --nocapture`. This test is explicitly ignored otherwise and must be reported not-run, not accepted.
3. The predecessor gate creates synthetic user content through the predecessor HTTP runtime, configures its real encrypted export, rejects active writer/wrong signature/platform/schema, interrupts after migration-start/Store migration/manager replacement/current publication, and recovers via target bundle or installed refreshed manager. It checks old-manager failure before replacement, choose-old refusal and preservation of newer committed work and exact source export.
4. Independent review of storage/admission, pinned executable and activation recovery boundaries; joined Linux execution after schema integration.

No real accounts, private data, credentials, provider calls, old collectors, cron or legacy databases are involved. Test failure injection exists only under the existing `test-fixtures` feature. Official signed releases, arm64 acceptance, production migration and complete P10 release remain unaccepted. Historical-export restoration and file coverage retain the backup/recovery contract's stated limits.

## Root Linux integration

The integrated target schema48 is tested against an actual schema47 executable built
from public commit026e315, with its source archive and binary hashes retained in
the private verification record. The R1 same-schema lifecycle passed once. The
predecessor fixture initially invoked a hard-linked Cargo output directly and was
correctly refused by the executable's existing single-link ownership check; root
changed the fixture to execute the owner-private signed bundle copy, matching the
operator instructions. This changed no production permission or signature check.
The real predecessor lifecycle passed all four interruption/recovery cases on Linux
x86_64 (142.95 s test time). Clippy with the affected CLI/Store `test-fixtures`
feature, default embedded-web build and plan verification passed. The first lint
invocation selected a server-only feature on the CLI/Store packages; correcting the
verification command required no source change and did not repeat the successful
lifecycle. Tests are synthetic and no installed production service was changed.
