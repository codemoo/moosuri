## Explicit route

`ModelClient::chatgpt_plan(Secret, TextCapabilities)` uses fixed `https://api.openai.com/v1/responses` and normal protected bearer transport. It accepts a caller-resolved credential; it supplies no OAuth or ownership claim. Existing `Protocol` and ordinary `hosted` behavior stay unchanged. `ResponsesDialect` opts in explicitly and is frozen in continuation checkpoint identity. A copied ordinary API checkpoint cannot become a plan checkpoint through the public parser.

## Wire and state

The text encoder maps internal system turns to developer at the same positions, sends input array/store false/stream true and omits max_output_tokens. The numeric local request budget remains a local validation bound. Input image attachment fails explicitly for this increment. Account catalog uses models/slug/display_name/visibility, preserves order, rejects duplicate IDs and malformed/oversized metadata, returns at most 512 visible models and reports truncation. No API-shape/catalog fallback exists.

Plan function definitions are nested under one fixed moosuri namespace. Complete call state and replayable raw history retain namespace/name/call_id; every initial, result and restored checkpoint path validates dialect and namespace. Tools require explicit capability and exact model identity. Partial/failed/incomplete streams do not yield executable calls. Known plan error codes are enum values; arbitrary bodies and messages are not diagnostics. No inferred effort/tools/images or login availability.

The existing HTTP client supplies address validation/pinning, no redirects/proxies/retries, request/response/deadline/cancel bounds and bearer sensitivity. Plan text also waits for clean bounded HTTP EOF, so a trailing contradictory/error event cannot silently pass. Local cancellation never claims remote generation stopped or charges vanished.

## Deferred integration

Personal owner/revision selection, OAuth/JWKS/state/nonce/PKCE, refresh/revoke, VM transfer and UI are intentionally outside this increment. No shared provider enum or storage is extended. Before user availability those layers and per-account capability probes are required. The official preview fields/schema were checked 2026-11-13; do not use backend-api, cookies or guessed namespaces.

## Review repair: response identity

Plan failed-frame usage requires an established response and explicit exact response ID/model before it can become an accounting snapshot. Full created/progress/queued/completed/incomplete snapshots require explicit valid identity and every snapshot after creation must match the established response. Standalone sanitized errors with no usage remain usable; contradictory supplied identity still fails. This boundary is plan-only and applies to text and tool forwarding before a successful terminal or executable round. Regression coverage includes pre-created/missing/null/foreign failure identity, foreign progress followed by a valid terminal, preserved prior visible text, valid attributed failed usage and ordinary API behavior.
