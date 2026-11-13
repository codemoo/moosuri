## Why

F29/F30 need explicit recurring agent tasks separate from literal reminder text. F31 needs an honest narrow equality policy rather than semantic claims about arbitrary model prose.

## What Changes

- Add owner-authenticated recurring read-only mission templates, time-rule edits, pause/resume/revoke and observed history.
- Reuse reminder time/occurrence semantics while isolating all generic reminder routes and chat capture from recurring authority.
- Pin model/profile/owner epoch at explicit save; atomically admit at most one real mission per occurrence, with the original due-based execution window.
- Publish content-free mission-history notices and optionally suppress only exact successful decimal/source-backed results.

## Impact

Rust Store/core/server/worker, React mission and inbox views, SQLite schema0028. No dependencies or external scopes added. Conversational recurring-task capture, broad evidence normalization and semantic equality remain pending; this change stays unarchived until the broader explicit follow-up scope is reconciled.
