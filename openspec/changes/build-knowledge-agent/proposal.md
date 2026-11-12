# Change: durable read-only knowledge agent

## Why
Saved sources and memories need bounded, account-scoped use in actual conversations,
with truthful recovery and revocation of answers derived from withdrawn evidence.

## What changes
- Prove model tool capability through a two-round synthetic challenge.
- Persist model-step receipts, private continuations and typed local-read results.
- Bind actual evidence and inherited context to derived outputs and invalidation.
- Expose exact source references and replace stale browser history on resnapshot.

## Impact
Advances P1/P3/P4 without marking those phases complete. Rust runtime/store/HTTP,
SQLite migration5, React provider/chat/knowledge UI and generated DTO change.
Automatic extraction, memory writes, external effects and community tools remain pending.
