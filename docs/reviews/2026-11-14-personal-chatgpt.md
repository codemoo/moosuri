# Personal ChatGPT text connection

P1 connects same-computer official loopback authentication to owner-scoped sealed
credentials, restart/refresh, exact text capability proof, personal model/effort
selection and durable streamed chat. Shared API/local connections remain separate.
An unavailable explicit personal selection never silently falls back to shared.
The normal-account CLI helper does not extract existing Codex login files or allow
administrator account-ID impersonation. Server-managed credential sealing is not
the independently keyed end-user vault.

## Independent review repairs

PSR-1: a successful rotating-token response followed by a temporary JWKS failure
was treated as a safe token-endpoint rejection, retaining the old refresh token.
The connector now classifies every post-success processing failure as unknown
rotation. Durable storage clears the usable carrier; no retry or restart resends
the old token. A separate known temporary token-endpoint rejection retains its
bounded retry behavior. Signed loopback fixtures reproduce the original failure
and verify token200→JWKS429/503, retry/restart no-replay and safe rejection recovery.

PSR-2: attachment-free personal followups inherited raw documents from earlier
shared turns. Admission now checks inherited metadata, including queued ancestors;
claim/retry guards repeat the check before materializing bodies, and the runner
checks before plan IO. The actual loopback regression rejects queued/completed
shared-document followups and preserves subsequent explicitly selected shared
document behavior. Image metadata follows the same policy; the independent SQL
probe covers queued/running/completed image rows without claiming an image HTTP
test. Re-review accepted both repairs against the frozen source and raw evidence.

## Integrated evidence

Rust1.89 Native focused gates passed: auth20, personal lifecycle7, shared files7,
media4, chat filter42 and affected all-target Clippy. Linux passed auth20 and three
compile-fail carrier documentation tests, then restore11, private files10, full
agent94/1 explicit live-network test ignored, HTTP35/1 network test ignored and
six-crate all-target Clippy. These overlapping filters are not summed into a
product quality score. One legacy schema29 fixture mistakenly dropped schema31/32
objects twice; the initial failure is retained and only that duplicate test
cleanup was removed. The existing memory retry failure was independently repaired
in its [separate unit](2026-11-14-sealed-memory-retry.md), retained during integration.

Root React settings13, TypeScript, production web build and exact generated DTO
comparison passed. A fresh Chrome390×844 profile with actual Rust/SQLite verified
the personal empty-state/settings limits, shared selection persistence, draft
continuity across settings/reload, one explicit streamed chat send and logout
clearing. There was no personal auth/probe side effect, horizontal overflow or
private API Cache entry. Database/provider receipts show one chat request and two
scripted shared calls (probe plus chat). This browser flow did not sign in to
ChatGPT; signed local OAuth/plan fixtures are separate backend evidence.

The isolated server and browser were stopped. The pinned test executable and
disposable browser profile were removed after verification; source, synthetic
database and evidence remain. CI was not enabled and production was not deployed.

## Remaining boundaries

P2 protected local-helper→Linux/VM handoff, P3 personal tools, attachments/media,
background work, live account eligibility/OAuth and real provider quality remain
unaccepted. Token storage/refresh is owner-scoped and restore quarantines it, but
this P1 does not make a remote VM login usable. The public preview documents the
same-computer scope and keeps the broader roadmap open.
