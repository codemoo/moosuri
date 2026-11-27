# Claude authentication inside Kiri — Linux acceptance

Baseline: public 1ba675d0, production dee84754. This unit removes external login
window/code-copy requirements from the administrator's existing subscription
flow. Preparation, lifecycle and prior synthetic Active results remain valid
within their recorded scopes. No real provider credential is reused for testing.

The current portable Rust capsule path was inspected. The earlier legacy SSH
bridge proposal was rejected. The selected path reuses the pinned headful Chrome
image, shared Google startup flock/inventory, existing frame proofs and private
CLI child; it does not introduce a general authentication automation framework.
Independent implementation uses official Claude Sonnet 5.5 / medium in three isolated
source copies; main owns integration, actual Linux tests and review acceptance.

These six supporting journeys expand M05/M06 in the existing 61 primary journeys:

| ID | User journey and acceptance | Current evidence |
| --- | --- | --- |
| K01 | Empty preparation → official login screen inside Kiri; no external tab, copied code, or developer setup. | Installed API and actual Chromium show the official Claude login page; final-source empty installation remains open. |
| K02 | Reload same session → same browser; another session/member cannot see or type into it. | Installed API and Chromium reload/sibling-session/member refusal passed. |
| K03 | Click/type/scroll on desktop/mobile; stale frame, duplicate request and lost response never replay an uncertain action. | Installed stale/duplicate scroll refusal passed; UI unit tests cover scaled click, keys, lost replies and non-replay. Real credential typing is open. |
| K04 | Exact official callback → one private code delivery; wrong state/origin/duplicate parameters refused; no URL/code in API, receipts, chat or logs. | Official artifact and synthetic wire/CDP checks passed; real Chrome valid/wrong-state callback cases passed with synthetic data. Real provider callback remains open. |
| K05 | Cancel/logout/revoke/restart → owned browser and CLI cleanup, then fresh retry; unknown cleanup retains a visible recovery state. | Installed cancel, restart/recovery and logout cleanup passed, each followed by a fresh attempt. General role/generation revocation remains component evidence. |
| K06 | Real owner login → verified account/model → connected state → actual inference and authenticated reboot. | Unexecuted; authorized fresh account input still required. |

No journey is closed by implementation, test count or official login URL issuance.
Final source installation, two actual members, Google automatic app setup,
natural-language Gmail and matched deployment remain open in the parent change.

## Integration evidence and retained failures

The first Linux connector compile found a non-Send `Cell<bool>` crossing an
async startup boundary; ordinary mutable state fixed it. Gateway test compilation
then found a `SessionAllocation` clone dereferencing to its underlying allocation;
the test now preserves the exact session epoch. A legacy disconnect assertion
still expected the removed public `code` field to deserialize; it now requires
rejection at deserialization. These were not successful native test runs.

UI checks initially reproduced continued polling after session loss and reuse of
a consumed frame proof. The fixes stop both status/frame polling on authority
loss, discard late hidden-tab frames, and do not replay an uncertain action.
The final two affected UI suites passed 25 tests, type checking and the web build.

Independent official Claude Sonnet 5.5 / medium reviews (21 and 39 turns) found and
verified the URL/early-Ready/deadline corrections, then found three further
boundaries: ambiguous startup cleanup losing its durable pending marker,
cancellation during frame polling being labelled failure, and generic navigation
URL limits rejecting long SSO/error pages. The candidate maps every post-spawn
cleanup failure to unconfirmed, preserves the cancellation cause and uses a
narrow private identity policy with the same frame/input proofs. The 17-turn independent closure found no material error in those corrections.
Installed API acceptance subsequently passed all six checks.

The new real-Chrome test runs inside the pinned Google image with no network and
synthetic documents/codes. Six selected processes passed: valid top-level callback
captured once and replaced with blank, wrong state refused, iframe/popup/subresource
not adopted, and cancellation. These exercise Chrome's Fetch behavior, not a real
Claude account or a provider-origin SSO round trip. The popup case does not prove
handling of a user-opened SSO window. Installed screenshots and actual provider
authentication are separate gates.


## Installed candidate and scope

The credential-free candidate gateway is
`f273524740a56959904546463b4bdcf6fe556bac7a7f013c9d405f9c103dbf44`.
Its worker image is
`sha256:fef019a82023c23b19e44c5f032a918cee215d7471ffb361feef85c5a7c72ae2`.
The existing marked Ubuntu24 guest admitted the candidate through the real Chrome
installation probe. This is an overlay on the retained pinned image, not a new
final-source bundle or a production deployment.

`installed-claude-kiri.py` passed K01/K02/K03/K05a/K05b/K05c through the installed
HTTPS API. Root-private synthetic member credentials stayed in the guest. It
checked no public authorize/code operation, initiating-session ownership, stale
and duplicate input, exact container absence after cancel/restart/logout and
unchanged model configuration. Chromium U01–U03 then passed desktop/mobile
layout, one-tab private screen, reload, foreign-session refusal and cancellation.
Visual inspection found the phone frame too small; an expand/pan control was
added. Final Chromium U01–U03 passed again on the asset-updated gateway
`9f5e0e1a466b330c84fc96195ad68a6c92ab193f13f16e892a7c19166d58b385`.
The mobile run checked centering, internal scrolling without page overflow,
return to fitted view, reload, sibling-session refusal and cancellation. The
expanded screenshot was visually inspected; the provider controls are readable.
The Rust authentication behavior and worker image did not change in this UI fix.

Final native checks: 166 relevant browser tests (165 plus the corrected empty-origin
assertion), 53 model-setup tests and 3 installer-render tests; the gateway and both
browser examples built on Linux. UI 25, type checking and the web build passed.
OpenSpec strict validation and 1800 plan links passed. Changed source contained no
new dependency or third-party asset; the final 36-file allowlisted scan found no secret.

The Chrome smoke's negative popup/iframe/subresource cases prove only non-adoption;
they cannot establish interception or blocked egress by themselves. Valid and
wrong-state cases do exercise interception. Setup currently repeats the private
Fetch rule in its test fixture. A real user-gesture SSO popup and provider callback
remain unverified. No real account, authentication token or inference was used.


Targeted Clippy completed; the added test's redundant clone was removed. Remaining
warnings predate this unit, including the existing render constant and receipt
nested condition. No warning cleanup broadened the production change.
