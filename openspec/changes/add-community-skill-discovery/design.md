## Context

Moosuri already has planned suggestions and capability cards, typed tools, private learned routes and multiple users. V1 should not add a marketplace service or arbitrary script runtime.

## Goals / Non-Goals

Easy Git authoring and one deliberate install/use action, accurate readiness, low maintenance. No automatic private publication, inherited authority, generic code plugin installation or separate registry account.

## Decisions

Use SKILL.md plus a small moosuri compatibility sidecar for native cards/routes. Keep SQLite authoritative for user installation/version records and export lockfiles only for portability. Publish a generated immutable catalog from reviewed Git content. Direct public GitHub links are an advanced import path; private Git sources are later work. Rank catalog candidates locally without sending private user context to the registry. Existing authorized credential bindings are reused by typed tools.

## Risks / Trade-offs

Format compatibility is not tool compatibility. Hashes prove identity, not safety. Disable unsupported scripts/hooks, validate archive paths/limits, and keep skill instructions below user authority. Manual updates with rollback are the initial policy; no silent upstream HEAD replacement.

## Migration Plan

Implement package/compatibility boundaries in P4, typed connector gaps in P6, card flow and public release in P7/P10. Keep legacy source/tests as references only. COMM scenarios are the verification contract.
