# Curated community skills — 2026-11-13

Six public instruction packages can be added from `/app/skills`, with per-user
install, update, rollback, enable, disable, remove and hide controls. Installation
never starts a task or grants a new tool. Starting a ready card preserves the
unsent composer draft and leaves sending to the user.

## Storage and execution

SQLite migration0011 stores immutable four-file snapshots and atomic per-user
active/previous pointers. Source commit and length-framed content digest are
pinned; existing runs read their admitted snapshot and discovery metadata.
Managed installs replace their curated bundled twin. Disable affects new runs;
remove also invalidates admitted work and blocks retries without erasing completed
external receipts. Another user's installation remains independent.

## Verification and corrections

- macOS and Linux x86_64 full Rust workspace:344 passed,1 live-model opt-in ignored. Source
  hashes matched the frozen read-only test snapshot before and after execution.
- React139 behavior tests, TypeScript checks and production build passed. Two
  App-level cases cover blank/multiline draft preservation and zero automatic POST.
- CLI integration requests `/app/skills` directly and receives the embedded app.
- Package tests cover bounds, paths, metadata, capability/license/example checks;
  store/runtime tests cover interrupted activation, rollback, user isolation,
  pinned old runs, tool non-expansion, disable and durable remove/retry fencing.
- HTTP tests cover authentication, expected-account checks and no task admission.
- Independent review found the starter overwrote an existing draft. The fix and
  missing direct SPA route were rechecked against the integrated PWA/recovery
  source. Independent focused web verification passed29/29; no concrete findings
  remained. That review is distinct from full workspace execution.

## Limits

This slice uses six public packages bundled from commit
`7f91c0ad8ef38049c5e0e601658da9b99b5f824e`. It does not yet fetch external GitHub
packages, generate releases, export private learning, integrate schedules or
remove unused snapshots. A matching digest is byte identity, not a safety or
quality certificate. No actual email, model, private account or production
service was used for these checks. GitHub Actions stays disabled.
