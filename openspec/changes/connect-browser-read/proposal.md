# Change: optional isolated browser reads in ordinary chat

## Why
Rendered public pages require real Chromium while preserving existing R/F execution,
tenant isolation, durable recovery and human-approved Gmail-send boundaries.

## What changes
- Expose URL-only `browser_read` only for an operator-configured, pinned deployment.
- Launch a fresh reviewed Linux capsule through the existing async external slot.
- Keep browser observations distinct from HTTP pages and search snippets.
- Cancel and clean owned resources before returning on lost authority.

## Impact
Additive profile 4; profiles 1–3, HTTP behavior and Gmail approval remain compatible.
No login/action/script tool, auto-provisioning or host-browser fallback. Deployment
verification is explicit operator evidence. See `docs/browser-chat.md`.
