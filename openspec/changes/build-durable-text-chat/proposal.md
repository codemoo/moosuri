## Why

Real conversations need saved messages and execution ownership beyond a browser
connection. Lost acknowledgements, cancellation, retries and restarts must not
create duplicate prompts, silently switch models or replay charged model calls.

## What Changes

- Add tenant-scoped messages, idempotent requests, revision-pinned runs/attempts,
  durable incremental events and minimal per-call usage receipts.
- Add a bounded Rust runtime separate from HTTP request lifetimes.
- Add React streaming conversations, cancellation/retry, paged history, reconnect
  and usage views, while preserving existing account and provider settings.
- Test synthetic live HTTP behavior and crash/authorization/deletion races.

## Capabilities

### New Capabilities
- `durable-text-chat`: persistent text chat with bounded failure-aware execution.

### Modified Capabilities
- None.

## Impact

Schema3, a Rust agent crate, API/SSE routes and the React composer/history. This
implements one text vertical slice of F03–F08/F57, not tools, memory, subscriptions,
scheduling, complete budgets, backup or full P1/P2 completion. CI stays disabled.
