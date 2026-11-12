## Context

The server must not gain a general decrypt API. A server-admin-controlled web client cannot resist malicious replacement of its own code; independently trusted clients are required for that stronger threat model.

## Goals / Non-Goals

Reuse previously authorized accounts conveniently through existing typed operations. Do not build generic secret retrieval, universal site login, or automatic server escrow.

## Decisions

Keep secret handling and authenticated execution on an unlocked trusted device; return adapter-specific bounded projections and authenticated receipts. Bind tenant, service, exact destination, operation/resource scope, item/key/grant version and original task authority. Persist broker dedup/dispatch intent before external effects. Pin every adopted effect to one broker identity/generation, with one local owner across tabs/workers. Reconcile the original durable journal after restart. Do not automatically transfer adopted effects between devices; a disconnected or timed-out owner may already have dispatched. Any future pre-dispatch transfer requires positive fencing and durable proof of non-dispatch. A locked/offline device pauses the original work and frees scarce execution slots. Optional limited server tokens use the existing server-connector trust model and are explicitly distinct from vault isolation.

## Risks / Trade-offs

Browser CORS and device availability limit unattended work. Password/passkey/MFA automation needs provider-specific adapters. Previous use selects a binding but does not authorize a new unrelated operation. Remote requests already dispatched cannot be recalled by local cancellation. Reconcile uncertain outcomes under the same effect ID.

## Migration Plan

Prototype trusted pairing/typed adapter and secret containment in P0/P8, then existing Google/API credential routes. Keep broker-dependent schedules waiting until a supported execution path exists. Implement CRED scenarios before declaring support.

## Trust and revocation clarification

Device-local grants cap server requests. v1 delegates request construction inside those grants to the server; client signatures/pairing do not prove a claimed task was genuinely submitted by the user. Secret non-disclosure and outside-grant rejection are the supported boundaries. Do not add repeated confirmation dialogs to pretend to solve malicious-server intent authenticity.

Use the existing writer to serialize online dispatch commitments against account/binding epoch changes. Prechecked offline work cannot create a new commitment. Earlier commitments remain in flight across revocation; broker-local revoke completion needs ACK/fencing, with upstream token revocation tracked separately. Details and CRED02/CRED03 oracles are in the credential design; this does not introduce another approval service.
