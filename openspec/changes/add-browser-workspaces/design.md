# Design

See [execution contract](../../../docs/browser-workspaces.md) and [official research](../../../docs/research/browser-workspaces-2026-11-13.md).

SQLite owns workspace state and controller revision; a serialized Rust broker owns the actual Chromium worker. Human input is not a model capability. Takeover fences queued model operations. Completion idempotently records a visible continuation turn in the same conversation, then permits observation of the exact current document.

Profiles remain in the capsule's private ephemeral filesystem. App disconnection does not close a workspace; idle/max lifetime, deletion, revoked authority and shutdown do. Restart fences state as unknown rather than reissuing actions. This is not durable disk-profile recovery or a vault credential broker.
