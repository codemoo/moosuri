# Design

`ModelClient::renewed_chatgpt_plan` replaces the bearer and capabilities while
retaining its explicit predecessor's HTTP cache. The cache contains a reqwest
client with no default credential headers. Each request still resolves DNS,
validates every bounded address, checks origin/address-set equality and applies
its own sensitive authorization header. Independent clients and changed limits
retain independent caches. Only the personal foreground run and its isolated
planners use this lineage; SubscriptionService stores no transport or token cache.

Each continuation still enters `subscription_access` with the current RunLease
and exact binding and uses existing refresh admission, dispatch, completion and
nonce/generation CAS. Durable model-step admission remains after authorization.
No inference is retried by this change.

A discovery guard holds the acquired refresh admission until dispatch is
acknowledged. Known discovery failure awaits acquired-only cleanup. Dropping the
caller schedules the same cleanup through the store actor; cancellation while
that actor admits dispatch cannot reset `dispatched`. Cleanup matches owner,
binding, revision, generation, nonce and `acquired`, so stale cleanup cannot
release a newer admission or replay an uncertain rotating token. Process/runtime
loss retains existing restart recovery and acquired-expiry behavior. No migration
or new persisted credential format is required.
