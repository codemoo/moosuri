## Why

Single-turn answers do not show whether an assistant preserves completed work
through new instructions, owner clarification, exact replay or reminder controls.
A small reusable public synthetic pack should make those failures visible without
mixing deterministic protocol evidence with live-model semantic quality.

## What Changes

- Add three HTTP→Runtime→SQLite everyday continuity workflows with reusable owner-visible fixtures.
- Consolidate stage observation and existing/new chat terminal polling; retain failure and not-run rows.
- Add per-stage model/tool ceilings, zero-call wait/replay contracts and independently tampered outcome controls.
- Publish report format 3 with additive stage records; preserve existing metrics and truthful token missingness.

## Impact

Only the Rust public evaluator and its documentation change. No product schema,
React UI, operational services, live credentials, legacy DB, scheduler or CI changes.
