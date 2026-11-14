# Scheduled skill lifecycle and snapshot collection — 2026-11-14

This bounded P4/F66 change adds actual-read recurring instruction dependencies,
needs_skill recovery and conservative collection of unreferenced public snapshots.
It does not add private executable route scheduling, install/upgrade automation,
new approval steps, a scheduler framework or a schema migration.

The original recurring model/profile stays frozen. Separate private JSON records
successful exact SkillRead pins in the actual tool transaction; older retained
successful typed receipts backfill once. Discovery/preflight alone is not use. A
concrete stored read denied by a racing withdrawal records only that attempted
admitted pin for recovery. Current read bounds apply before successful recording.

Disable/remove pauses only dependent parents, fences pending occurrences and
unfinished children, and preserves accounting/tool receipts and original budgets.
Enabled active/previous exact snapshots remain eligible across update/rollback;
new child profiles are filtered within the original frozen pins and gain no new
version. General foreground disable/remove retain their admitted-run/retry
contracts. Recurring discovery-only children do not become false dependencies.
The UI names the missing recipe/version and requires explicit future-only resume;
re-enable alone does not replay missed work. Existing restore quarantine remains.

Explicit lifecycle transactions may delete at most8 unreferenced package rows.
All users' active/previous pointers, historical/admitted chat profiles, tool
inputs/results/checkpoints, original recurring profiles/used pins and cached
catalog offers retain exact bytes. Tombstones/history/receipts are never collected.
Conservative reference reads scale with history and may retain extra bytes; there
is no general history GC. Uninstalled uncatalogued previews are evictable caches
and require exact re-preview on cache miss, never substituted installation.

Validation performed in isolated synthetic state:

- `cargo test --locked -p moosuri-store scheduled_skill_lifecycle -- --test-threads=1`:1/1 passed. Actual read→update/rollback→disable fences used child/parent while leaving unrelated/no-prior-run schedules and a second owner unchanged; explicit re-enable/resume; queued occurrence removal; actual post-removal child profile filtering; orphan deletion with historical/template roots and tombstones retained; unchanged original counters/deadline.
- `npm --prefix web test -- Missions.test.tsx`:15/15 passed, including missing-name/version recovery, no automatic resume and retained draft.
- `npm --prefix web run typecheck`:main and worker TypeScript passed.
- Pinned OpenSpec2.0.0 strict change validation, rustfmt and diff whitespace passed.

No live provider/device, credentials, external messages, service/cron/data, CI,
commit or push was used by this worker. Root owns independent review, integrated
canonical type export, Clippy/build/Linux acceptance and publication. The known
base parcel-size Clippy repair already exists in root and is not repeated here.
