# Current public source: empty Linux installation and cancellation failure

An empty Ubuntu24 guest cloned public revision `46b76b44`, built its host and
member/browser artifacts, and completed the actual terminal installer. Baseline
checks found no product/provider state or preinstalled Kiri, Docker, Node, Rust
or GWS. Supplied synthetic TLS was used; this does not prove ACME issuance.
The later `88b407bc` changes only connection documentation.

## Observed journeys

| Scope | Result |
| --- | --- |
| I01/I08 public source and terminal setup | Build and installation completed without a restart or developer credentials. App and separate Vault HTTPS were usable. |
| I04/J01 invited second member | Public web invitation API accepted the second password; distinct UID, volume, GWS and browser recipients. No root CLI password completion. |
| I05 replay/conflicting inputs | Same inputs retained identities and authority; changed origin refused without mutation. |
| I02 and member boundaries | Unconfigured model remained unavailable; only the administrator could open setup. Foreign recipients, stale handoff and logged-out access were refused. |
| V04 and two-member Vault support | Actual Chromium created both 8-character Vaults, saved encrypted synthetic items, refused wrong passwords and verified member separation. |
| I06 reboot | Actual changed boot ID; both identities and encrypted Vaults reopened; administrator/member boundaries retained. |
| Proxy confinement | Six actual checks passed, including public HTTPS, private destination refusal, no external worker route and retained egress after proxy restart. |
| Claude U01 | Mode choice, exact Sonnet5.5/medium defaults, hosting consent and mobile form passed. |
| Claude U02 | **Failed:** cancellation left `cleanup_pending` with browser intent retained. Docker inventory was empty, but that alone did not override the missing shutdown receipt. |
| Same-owner cleanup recovery | A separate actual browser run used **정리 다시 시도** and reached cancelled. Original failure evidence remains retained. |
| FM01–FM03 | A separately resumed installed Chromium run passed one first-message read, explicit reload resume and preservation of a newer different/identical draft. Interpretation and Gmail replies were synthetic; no model inference or provider authentication. |

The actual Claude screenshots show the provider's Cloudflare security verification
page inside Kiri. Desktop/mobile frame display, reload and expansion are visible;
this run does **not** establish arrival at the account form or authenticated login.
The original coordinator remains failed. Neither the successful recovery nor the
independent FM run converts it into a passed release proof.

## Cancellation correction

The host actor dropped its stdout reader before waiting for worker shutdown.
Cancellation can arrive while a frame is being written: dropping the pipe can
break the worker/Docker output, while keeping it open without consuming it can
block shutdown. A Linux regression using a delayed 256KiB response reproduced
`CleanupUnconfirmed` on the unchanged code.

The correction retains stdout from startup through cleanup and discards late
output privately in a fixed-size zeroizing buffer. The existing cleanup deadline,
required successful worker exit and exact container reconciliation remain in
force. Late frames/codes are not parsed, adopted or replayed. All11 focused
connector tests passed, including the previously failing regression and the
stalled-worker refusal. Independent official Claude Sonnet5.5/medium reviewed
the frozen change and found no material regression.

The corrected native gateway
`90951364617f90bb4db1610df8b71c9da698ab7e0620ea3cd21603cf17764ed6`
was then selected in the same disposable installation. Actual Chromium U01–U03
passed, including confirmed cancellation without a cleanup retry. The six
installed API checks K01/K02/K03/K05a/K05b/K05c also passed: private frame,
session/member boundaries, stale and duplicate gestures, explicit cancel,
gateway restart/recovery and logout. The provider screenshot still shows
Cloudflare verification; no account authentication or inference was performed.
This is a diagnostic host-gateway overlay, not a rebuilt installation bundle;
the original installer receipt remains `46b76b44`. An immediate post-restart
hash observation raced process execution; subsequent active/running state,
zero restarts and exact binary hash were verified before the UI/API checks.

The runner now reports failures at the Claude UI module's own line rather than
its dispatch call. Production remains `dee84754`; no reviewed `46b76b44` staging
or transition was executed. Final corrected-source installation, matched rollout,
real Google/Claude authentication, automatic OAuth setup and two real member
consents remain open.
