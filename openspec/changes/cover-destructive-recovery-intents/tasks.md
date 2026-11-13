## 1. Implementation
- [x] 1.1 Add installation-bound bounded successor journal and migration0015.
- [x] 1.2 Route all covered operations through one append/fsync adoption boundary.
- [x] 1.3 Preserve direct command targets and one bounded source-batch record.
- [x] 1.4 Add explicit format2/five-entry export and retain format1 codec.

## 2. Verification
- [x] 2.1 Add synthetic migration, fault/replay, tenant and codec coverage.
- [x] 2.2 Complete local Rust/fmt/clippy/plan and strict OpenSpec validation.
- [x] 2.3 Independently review, repair/recheck retry durability and integrate with automatic-memory0014 as schema15 on macOS/Linux.

No task here completes historical restoration or independently latest checkpoints.

The integrated review record retains the Linux format1 fixture permission failure
and targeted repair; public reader requirements remain unchanged.
