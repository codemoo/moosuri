# Design

Keep ProviderConfig and shared providers unchanged. ModelSource defaults to shared for
historical JSON and is authoritative for personal resolution and plan encoding.
Separate owner-scoped preferences have transactional database exclusion triggers.
A stale personal preference persists and never enables installation fallback.

The existing server-managed root key seals verified OAuth carriers. Trusted local
reopen authenticates the full record; it is not an arbitrary identity deserializer.
Rotating refresh uses durable acquired/dispatched admission and exact generation CAS.
A dispatched unknown outcome is never reclaimed by lease expiry. Owner/session/auth
epoch and binding revision are rechecked before token access and result commit.

Same-host native login uses official loopback OAuth and persists issued registration
before exchange. Verified subjects remain pinned across returning login. Remote VM
protected transfer and personal tool/media/background capabilities remain separate work.
Restore clears personal authority and allocates a new runtime/host. Existing shared
model, source, accounting and recovery contracts remain in force.
