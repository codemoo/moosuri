# OpenSpec in moosuri

Use [CONTRIBUTING](../CONTRIBUTING.md) and the pinned CLI in `tools/openspec`.

- `specs/` contains established repository/contribution contracts.
- `changes/` contains proposed or accepted work whose implementation is still pending.
- `changes/archive/` will contain completed changes after specs/code/tests are reconciled.

The current changes cover the evaluation harness, Git-based skills exposed through existing agent cards, scoped vault credential use, and complete file/schedule recovery. They are not implemented product features. `docs/status.md` is the actual-status source; OpenSpec artifact completion is not runtime completion.

Do not create another spec store/repository or custom workflow schema for v1. Work together through normal branches, draft PRs and small deltas. Direct PRs are sufficient for minor nonbehavioral fixes and ordinary compatible recipe additions. Significant behavior/security/compatibility changes use one named change.

Maintain links to R/F IDs and the relevant scenario IDs instead of copying the full planning documents. `## MODIFIED Requirements` must preserve the full requirement and retained scenarios; removed requirements need a reason and migration. Never silently delete an accepted behavior to make a new implementation pass.
