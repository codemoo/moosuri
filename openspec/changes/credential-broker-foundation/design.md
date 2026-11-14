## Context

The standalone vault has ciphertext authority, not a device identity, credential
scope or external-dispatch journal. Existing Gmail/agent/browser paths are separate.

## Goals / Non-Goals

Goals: real writer pairing/authentication, immutable invocation adoption, local grant
ceilings, process ownership, crash/replay and signed private settlement.
Non-goals: production adapters, credential unlock/migration, generic authenticated
HTTP/fill, model tools, background work, signed companion packaging or full v1.

## Decisions

Use native-only ring0.17.14 Ed25519 and explicit bounded domain-separated binary
encoding. Keep protocol serde-only. Reuse current UserContext/session/epoch/restore
fences. New routes require an explicit nondefault feature. A typed synthetic fixture
is the only invocation minter. Local grants cannot be created/broadened by server
metadata. Reopened retained work cannot dispatch. Signed receipts settle privately
without conversation resurrection. Migration37 quarantines restore enrollment while
retaining non-authorizing evidence.

## Risks / Trade-offs

A malicious paired assistant can invent an in-scope task. Local executable/OS trust,
unsigned distribution and coordinated full-directory rollback remain explicit limits.
Fail-closed partial journal writes favor integrity over availability. Native HTTPS
client/real adapters and independent protocol/storage review are follow-on gates.
