# Private reference content preflight

2026-11-14. P0 source identity preparation, not executable baseline acceptance.

The legacy checkout's Git revision does not track its agent runtime. A fixed historical
audit manifest instead selects149 runtime, migration and dependency/lock files totaling
1,750,513bytes. `moosuri-eval baseline-pin` checks these exact bytes and refuses missing,
changed, symlinked or extra runtime files. Python bytecode is excluded; future isolated
execution must use a fresh allowlisted copy. No source is imported/executed or sealed.

The actual private source passed the CLI locally. The selected content identity is
`cdeeea40a71f6179fe3a91610c155ea34e76fd142ef1ba7254c2c496f598f23e`, bound to audit
manifest `d1bb74b49e4669e7b88fdaa75f30352576a24efd5df33ab0343931ae0bf8d469`.
The report contains no source bytes or private paths. Local checks verified0600 report
creation, refusal to overwrite an existing report, failure on a missing reference and
rejection of a relative source path. No private reference source was exported.

Independent read-only review found no material issue in this bounded preflight. Linux
ran3 mutation/privacy controls using only synthetic reference files, all-target evaluator
Clippy, the default six-world/16-grader-control CLI and plan verification. All passed;
the source archive identity and read-only source hashes were verified. No provider,
operating DB, collector, cron or service was used or changed; CI remains disabled.

The original executable requires PostgreSQL and Python dependencies. It has no common
fake-clock seam or calculator tool identical to the candidate. Runtime isolation,
source freezing/recheck, dependency installation, common fixture normalization, fixed
model/profile and paired comparison are still pending. All corresponding report fields
remain `not_run`. OpenSpec task2.1 remains open; only its content-preflight subtask closes.
