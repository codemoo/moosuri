# Claude SSO window transition

This expands M05/K03/K04/K05 of the existing Google-installation journeys.
The running production bundle remains c3d18bd9; documentation publication
88fb46b8 did not change it. No provider authentication is claimed here.

## Observed failure

One bounded production retry displayed the official Claude login inside Kiri.
Before **Continue with Google**, the root Claude page was attached. After that
proof-bound action, Chrome158 created a page at `https://accounts.google.com`.
`Target.getTargets` confirmed its opener was that root Claude page,
`canAccessOpener=true`, and `attached=false`. The product lost its frame and
showed screen-waiting. The controller only tracked the original page.

The diagnostic projected target types, origins, URL lengths and opener/attachment
metadata. Full authentication addresses, credentials and page contents were not
logged. No Google identifier, password or verification code was submitted. The
attempt was cancelled through the product; its authentication container was gone.
This establishes missing SSO-window tracking, not a provider rejection. It does
not establish which underlying screenshot operation stopped answering.

## Six supporting user journeys

| ID | Action | Required result | Evidence |
| --- | --- | --- | --- |
| SS01 | Choose Google on the official login screen | Same Kiri screen displays the proven related window and accepts fresh proof-bound input | Failed live on c3; correction and Linux Chrome acceptance pending |
| SS02 | Close the provider window, then reopen it | Original login returns; prior window/proof cannot be reused; only one current window is controlled | Pending |
| SS03 | Original login completes while the provider window is present | Exact original callback is intercepted before network, delivered once privately, and all windows are cleaned | Pending |
| SS04 | A delayed click or input uses the screen shown before switching windows | Refuse without delivering or replaying it into the new document | Pending |
| SS05 | A foreign/no-opener window or a popup callback appears | Do not adopt unrelated control or callback authority; no callback code escapes into a frame or network | Pending |
| SS06 | Cancel, time out or lose authorization while the provider window is open | Original and descendant windows are cleaned; uncertain cleanup keeps the existing recovery fence | Pending |

Synthetic Chrome checks and actual provider acceptance will be recorded
separately. An implementation, a mock CDP result or a login page alone does not
complete SS01 or the primary real-authentication journey.
