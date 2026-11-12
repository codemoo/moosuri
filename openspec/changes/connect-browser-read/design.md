# Design

The trusted CLI admits an immutable image/network/proxy/owner/seccomp configuration.
Its canonical SHA-256 is captured by profile 4 and the per-slot external permit.
The adapter compares the permit pin to its actual reader and supplies the account
UUID from authenticated context. The model supplies only a public URL.

The controller checks pinned resources before launch, runs one fixed `open`
request, validates bounded output and readiness identity, discards handles, and
adds its own retrieval time. Capacity is two without queueing. Cleanup is owned
independently of the caller, kills/reaps stalled CLI IO, and removes only a name
whose owner/read labels still match. Runtime authority loss cancels the operation
and waits for its bounded cleanup. Existing dispatch nonces, replay/unknown-slot
handling, budgets and completion fences prevent late publication or duplicate IO.

BrowserPage evidence derives only from matching completed typed slots, uses the
existing optional DTO, and follows tenant/source/thread purging. No schema migration
or model mail action authority is introduced. Host Docker authority remains an
operator responsibility; neither `verified` nor an evidence hash is attestation.
