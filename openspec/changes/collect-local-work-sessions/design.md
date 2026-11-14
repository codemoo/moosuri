## Scope decision
Support only canonical Codex JSONL with public schema verified at
openai/codex b1573f102413e96a6481087922bf425231222dee. Require root CLI/user
session and explicit user-triggered root turn attribution. Select only literal
user_message prose, never response_item/tool/assistant/config/attachment data.
Unknown/automation/subagent/fork/inherited history does not gain compatibility
by inference. Finished marker versus idle partial describes record lifecycle,
not task success. Current docs link each authoritative source.

## Persistence and transport
Use existing custom source endpoints and one dedicated expected connection.
Private durable state first pins file/semantic cursor and request intent, then
run ID and exact serialized body. Unknown batch/finish ACK recovers the original
receipt/body without scanning changed text under an old UUID. Advance local
cursor after receipt+finish, or exact original receipt plus current after-position
and no active run. Preserve restart-fenced unknown run history; never rewrite it
as completed. Conflicting active run/position stops with pending state intact. Keep no token in state and clear pending prose
after confirmation. Hash local identifiers/paths; retain no full transcript.
All production HTTP is bounded, exact-origin HTTPS, no redirects/proxies/retries.

## Local safety and limits
Explicit paths only, private owner directory, anchored no-follow regular file
opens, root/current-UID + nonmutable-or-sticky ancestor trust (sticky requires next owned
component), before/after inode/stat/hash stability, quiet/stale partial delay, bounded
rotating scan and semantic dedup. No absent-file delete inference or local tool
execution. Prose filtering deliberately sacrifices some technical request coverage.

## Known failure boundary
The existing run-start endpoint has no client idempotency key. Lost start ACK
stops with a durable intent; do not adopt an unproven active run. Reliable start
reconciliation is a later connected contract. Initial tests use a mock custom protocol. The bounded R1 regression uses the
actual Store/router across reopen and proves unknown-run receipt recovery without
duplicate ingest; it does not establish real client/account/carrier validation.

The subsequent [recover-custom-ingest-start](../recover-custom-ingest-start/proposal.md)
change supersedes the unkeyed-start limitation for schema48 typed intent. Legacy
unkeyed uncertainty and already-staged inactive bodies retain their conservative boundary.
