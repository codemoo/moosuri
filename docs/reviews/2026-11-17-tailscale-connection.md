# Tailscale private access · 2026-11-17

## Delivered behavior

The bilingual installer detects the existing Tailscale device, guides sign-in on
its current profile and creates private HTTPS access after account setup. Existing
installations use `moosuri tailscale connect`, `status` and `disconnect`. The native
local API follows the released Tailscale v1.104.7 contract; no CLI subprocess
receives account tokens. `--no-browser` also applies during Tailscale sign-in.

An exact origin and installation/node-bound receipt join external Serve changes
with local origin changes. Config writes retain the original ETag and preserve
unrelated fields, ports and routes. A root route is never adopted without an
ownership receipt. Interrupted changes block start/update until connect or
disconnect resolves them. Disconnect restores the original origin and deployment
examples. Uninstall validates ownership and reuses the saved custom socket;
interrupted external removal can resume without a duplicate write.

Moosuri account login remains required. HTTPS configuration and actual readiness
are distinct. Phone/PWA access requires the client device to join the tailnet.
Windows-host Tailscale with WSL2 uses an explicit manual route; Moosuri does not
install a second daemon. See the [usage and compatibility guide](../tailscale.md).

## Verification

Native installation contracts passed (28), including native discovery boundaries,
route preservation and the existing signed installation contracts. Eight initial
real-binary PTY scenarios passed. After independent review, all three affected
Tailscale scenarios passed: external-write and local-publication interruption,
ETag conflict/unowned-route preservation, and no-browser plus owned custom-endpoint
removal after interruption. They preserve synthetic account/database bytes and
unrelated HTTPS/Funnel configuration. The managed server also passed normal
login, Secure-cookie, forged-forwarding-header and wrong-Origin checks using the
configured tailnet origin.

Independent review identified missing removal ownership validation, an unsaved
custom endpoint and browser preference loss during sign-in. Each was fixed and
covered by the removal/login outcome scenario. Read-only follow-up found no
remaining concrete blockers in the affected paths.

Native scoped Clippy and signed install/update/recovery/removal passed. Isolated
Linux scoped Clippy, installation contracts and all nine real-binary PTY scenarios
passed. Four release builds, signed packages and public-bootstrap acceptance are
being finalized.

## Acceptance limits

All Tailscale daemon scenarios use a synthetic Unix local API and synthetic
accounts. They do not establish real Tailscale login, certificate issuance,
phone-to-server access, macOS GUI-daemon variants or Windows/WSL acceptance.
The API is upstream-unstable; unsupported schema or missing ETag fails without
mutation. Actual Tailscale host configuration, personal accounts, providers,
services, collectors, cron and databases were not used. CI remains disabled.
