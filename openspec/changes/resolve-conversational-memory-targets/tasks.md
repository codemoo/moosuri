# Tasks

- [x] Implement sealed bounded candidate plans and index-only selection after literal misses.
- [x] Preserve literal ambiguity, current-request authority, retry/purge and journal contracts.
- [x] Add synthetic Store and scripted-runtime regressions across topics and failure boundaries.
- [x] Record final focused Rust/fmt/clippy/OpenSpec validation results.
- [x] Independently review and integrate migration0012 after community0011.

Live-model semantic evaluation is intentionally separate; this change does not
replace the recorded7/10 result or claim the full assistant roadmap is complete.

Verification on Rust1.89.0 in the isolated worker:

- `cargo test --locked -p moosuri-store memory_commands --no-fail-fast`:31 passed.
- `cargo test --locked -p moosuri-agent memory --no-fail-fast`:21 passed,1 live-model test ignored.
- `cargo clippy --locked -p moosuri-store -p moosuri-agent --all-targets -- -D warnings`:passed.
- `cargo fmt --all --check`:passed.
- `cargo run --locked -p xtask -- verify-plan`:passed.
- Pinned OpenSpec strict validation:24 items passed; the final changed spec also passed focused validation.

These are focused synthetic checks. Existing provider/chat migration tests still
expect schema10 in this worker and must be reconciled with integration schema12;
the full workspace suite was not claimed or run for this tranche.

Independent review found and repaired candidate-snapshot revocation: all candidate
memory/evidence revisions now register dependencies and pin execution generation;
real forget/source-delete purges plans/checkpoints before retry. Resume and
step admission validate complete snapshots before exposing data or dispatching.
Regressions cover unselected-candidate withdrawal both before and after selection,
time-only drift, and retry's execution-generation reset. Final verification above
includes the repair. Review recheck is tracked separately before integration.

Integrated closure: shared schema12 and migration order were independently rechecked; Linux workspace362 passed(1 live opt-in ignored), macOS Store31/agent21 plus schema and HTTP checks passed. The12-case frozen holdout was not attempted because both real protocol tool probes returned empty final messages; no capability proof was bypassed. See [acceptance](../../../docs/reviews/2026-11-13-memory-target-resolution.md).
