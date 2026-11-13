## Implement
- [x] Add bounded fixed-host public GitHub commit/tree/blob reads and Git object consistency checks.
- [x] Add versioned external package envelopes and normalized source/directory identities without changing legacy pins.
- [x] Add preview, per-user CAS activation and existing lifecycle/immutable run integration.
- [x] Add account-scoped external source/connection preview and one install/update action.

## Verify
- [x] Pass synthetic source/redirect/mode/size/hash and branch-pinning tests.
- [x] Pass lifecycle, source collision, held-account/download, immutable run and late-output tests.
- [x] Pass HTTP/account-aware UI, build/types and clippy checks.
- [x] Complete independent review, integrated macOS/Linux verification and public source checks.

Private sources/auth, archives/assets/scripts, generic instruction import, automatic refresh/release generation, private learning export and cache GC remain outside this slice.

Independent review P2 repair: centralize parent/import mutation ownership and generation-gate initial/manual-refresh catalog snapshots. Five parent-level race regressions plus existing importer/curated/App/chat checks pass53 worker tests; type/build pass. Independent recheck closed the defect. Final integrated root checks pass56 React tests including retained draft behavior, plus focused Rust and Linux checks documented in the review record.
