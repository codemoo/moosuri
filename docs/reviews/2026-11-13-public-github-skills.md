# Public GitHub instruction acquisition — 2026-11-13

The isolated slice extends curated instructions with explicit public repository/directory/ref preview and single-action per-user installation/update. It does not add any executable adapter, Git subprocess, archive extractor, credential connection, model call or external effect. Old curated package bytes/digests/pins retain their version1 codec. External version2 envelopes have their own normalized repository+directory identity and domain-separated SHA-256; an external name never replaces a bundled twin.

## Verified boundary

- Four synthetic real-loopback HTTP adapter cases exercise ref resolution once followed by immutable tree/blob paths, anonymous headers, canonical source/path rejection, redirects, truncated trees, symlink/submodule/executable/extra files, declared/aggregate body bounds, mismatched object IDs and decoded sizes. Git object SHA-1 consistency is separate from the app package SHA-256 and is not safety certification.
- Store cases cover versioned codec separation, repository/path/name collisions, preview without installation, account isolation, manual update/rollback/remove, native-tool non-expansion, actor availability while acquisition is held, and revoked-context rejection before staging. A real admitted run reads its original external snapshot after update; remove rejects held late output and does not remove the bundled baseline. Existing curated lifecycle/pin regression cases continue to pass.
- The synthetic HTTP boundary rejects unauthenticated/stale-account preview and invalid destinations before network work.
- React checks cover preview/source/missing connections, one explicit install/update with exact expected revision/digest, late preview after source edit, account switch during held installation, existing curated behavior and App/chat regressions. The repaired focused five-file run passes53 tests; TypeScript and production build pass. Clippy over connectors/store/server/agent all targets with warnings denied passes.
- An explicitly authorized read-only probe fetched codemoo/moosuri `community/skills/source-briefing` at `7f91c0ad8ef38049c5e0e601658da9b99b5f824e`: four files,2,977 decoded bytes, with every blob verified. An opt-in HTTP integration case used only a fresh local synthetic account and that public source: preview did not install/run anything; explicit activation returned the same pinned digest. No private context, provider credential, model or mail was sent. This network case is ignored by default.

## Limits and integration

Public GitHub only, explicit nonempty ASCII directory path, at most six directory components and the exact existing four-file/Apache-2.0/instruction-mode compatibility subset. No generic plain SKILL import, archive/assets/scripts, private authentication, automatic upstream refresh, publisher/export or cache GC. Upstream API rate/body/time limits fail visibly without changing active versions. At most64 external identities including tombstones remain per user. Public immutable staging bytes are currently retained.

There is no schema migration. The only dependency change is the direct edge to the already-locked `sha1=0.10.7`; no new registry package is resolved. Integration preserves localized curated names, draft-preserving starter actions, `/app/skills` routes, backup dependencies and schema13.

## Independent UI review repair

Independent review of the first frozen slice reproduced a P2: an older initial catalog GET could replace the successful import snapshot and hide the installed procedure controls. Import and existing-card mutations also had separate request owners. The repair makes the parent the sole owner of every install/update/remove/enable/hide catalog mutation. A synchronous owner reference serializes parent/import actions even before React renders shared busy state. A catalog generation invalidates and aborts older initial/refetch GETs before mutation; only the current owner/generation may admit a snapshot. The importer retains its separate source-preview/account fences and delegates activation to this coordinator. Preview/install still never start a task.

Five actual parent-component regressions cover the independent delayed-GET/import reproduction, both simultaneous lifecycle/import orderings, a delayed explicit refresh after removal, and account switch during importer activation. Together with importer, curated card, App and chat tests,53 worker tests pass; TypeScript and production build pass. No Rust/API/authentication/App.tsx/static-route source changed for this repair, so the earlier339-pass Rust regression and separately selected public fixture evidence remain applicable. The independent reviewer reran the original reproduction and five new regressions:6 passed, with no remaining concrete finding.

## Integrated acceptance

The merged source passes4 connector cases,6 community store cases,4 run-pin cases
(one overlaps the community filter),1 HTTP boundary case and1 actual CLI installation
journey on macOS and pinned Linux Rust1.89. The opt-in public-network HTTP case is
skipped in these integrated runs; its earlier public fixture result is separate.
Linux uses verified read-only source/vendor mounts, no network and a nonroot user.
React passes33 component/App tests plus23 chat tests; typecheck, production build,
generated Rust/TypeScript DTO comparison, workspace all-target clippy, fmt and
plan validation pass. Strict OpenSpec initially found a missing scenario under
the installation requirement; the scenario was added and validation rerun.
No fresh live model, private account or production deployment was used.
