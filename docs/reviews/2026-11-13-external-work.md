# Public HTTP research and approved Gmail acceptance

This unit connects real HTTP/Gmail transports to the existing tenant-scoped durable
agent and React app. Its public contracts are [mail](../mail.md),
[HTTP research](../web-research.md) and [external work](../external-work.md).
It does not ship the separately developed Chromium capsule or claim all assistant
functions are complete.

## Behavior

- Public pages use unauthenticated HTTP(S), bounded extraction and per-hop public
  DNS validation/address pinning. Search requires the configured fixed Brave service.
  Observed source links derive from completed persisted tool slots, not model prose.
- Personal Gmail OAuth is bound to the original login session/account/auth epoch,
  one-use state and PKCE. A cookie-less callback stages an encrypted code; the
  authenticated same-origin completion claims its opaque handle.
- Search/message/thread/draft tools use admitted account epochs and current authority.
  All IO runs outside SQLite. Cancel/logout/source or conversation deletion and
  connection revocation fence late results and inherited dependent payloads.
- Each email requires final human approval of its immutable complete preview.
  No model-callable approve/send or browser shortcut exists. Approval accepts only
  revision/digest, not replacement content. The durable worker commits once before
  sending exact frozen MIME bytes. Ambiguous outcomes are never automatically replayed.
- Gmail receipt IDs mean provider acceptance, not recipient delivery/read confirmation.
  Attachments/HTML/aliases/labels/Gmail drafts and automated Sent reconciliation remain
  unsupported. Connector OAuth is server-managed encryption, separate from the user-key vault.

## Verification

The frozen integrated implementation passed Rust1.89 workspace tests on macOS and Linux x86_64:
**244 passed**, one opt-in live-account test ignored. Counts: Store89, agent47,
connectors81, HTTP24, CLI1, installation1 and server configuration1. Root integration
also passed all-target workspace clippy with warnings denied and rustfmt.
React **78 behavioral tests**, typecheck, production build and generated API DTO
comparison passed. Linux used a digest-pinned Rust image, nonroot user, read-only
source, no network/capabilities, two CPUs and4GiB RAM. Its first apparatus attempt
tried to resolve rust-toolchain components while offline; selecting the exact
already-installed toolchain fixed that setup, and the unchanged source suite passed.

Independent source reviews covered transport, OAuth, approval/dispatch, revocation,
the HTTP vertical and React interactions. Findings fixed and re-reviewed include:

- Multiple inline MIME bodies must mark incomplete returned content.
- Connection revocation and token-refresh failure must share the SSE delivery fence.
- Both approval control and handler must reject while editing/saving a revision.
- Purged draft content must be cleared from an open editor before the next paint.
- `aria-hidden` text can remain visibly meaningful; the HTTP reader retains it.
- Cloud platform metadata addresses, including Azure168.63.129.16 and mapped IPv6,
  must be denied.

Actual desktop Chromium tested a synthetic local account against the real Rust API:
login, From/To/Cc/Bcc/full-body preview, editing disables approval, saving updates the
preview, and explicit approval moves only the local action to approved. The server's
OAuth configuration was asserted false before approval, so no mail could dispatch.
At390×844 the document width remained390 and the independent pane scrolled2489px
within768px. This is viewport testing, not physical iOS/Android testing. The owned
browser and test server were stopped afterward.

A real public HTTPS read returned example.com's title/text; private-IP and missing
search configuration cases were rejected. An additional actual ChatGPT request using
the existing official login (`gpt-6.1-terra` / `high`) completed a public-page task:
the model called `web_read`, received the real page and answered with its URL. The
persisted tool slot and observed source were inspected; it took two model dispatches
and6.633seconds through the private development bridge. This single integration case
is not a web-research benchmark or TTFT measurement. No paid Brave search, real Google
OAuth or real mail send was run. Earlier actual ChatGPT task trials and failures are
separately preserved in [task skills](2026-11-13-task-skills.md).

## Publication and limits

The current267 registry dependencies have declared licenses; OSV returned no known
advisories at this check. That is not a guarantee of vulnerability absence. The282-file
source allowlist and secret scan,18 strict OpenSpec items and planning metadata/link
verification passed. Publication also checks the new commit history and exact staged
paths before pushing. CI remains disabled. No production service,
collector, cron, operational account/database or existing deployment was changed.
