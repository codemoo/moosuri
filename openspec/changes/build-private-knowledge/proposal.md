## Why

The isolated text assistant has no user-managed facts or evidence. Add private text
sources and explicit editable memory before connecting retrieval or extraction to chat.

## What Changes

- Add bounded idempotent text ingestion, tenant-scoped lexical search and evidence reads.
- Add explicit memory assertions, exact source evidence, revision/history and forgetting.
- Journal destructive scope changes and replay before readiness; expose clear keep/delete behavior.
- Add React source/memory management. No automatic extraction or chat recall is claimed.

## Capabilities

### New Capabilities
- `private-knowledge`: Tenant-scoped source ingestion and explicit memory management.

### Modified Capabilities

## Impact

Rust core/store/server, SQLite migration4 and private knowledge journal, React routes,
shared DTOs and meaningful synthetic storage/HTTP/browser acceptance. No collector,
credential, production data or legacy service is used. CI stays disabled.
