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
| SS01 | Choose Google on the official login screen | Same Kiri screen displays the proven related window and accepts fresh proof-bound input | Failed live on c3. Corrected real Chrome synthetic user-gesture popup, click and text input passed; actual provider retry remains open |
| SS02 | Close the provider window, then reopen it | Original login returns; prior window/proof cannot be reused; only one current window is controlled | Actual Chrome immediate-close/return, stale-proof refusal and reopening with an empty new input passed |
| SS03 | Original login completes while the provider window is present | Exact original callback is intercepted before network, delivered once privately, and all windows are cleaned | Actual Chrome synthetic background-root callback, one-use delivery and replacement passed |
| SS04 | A delayed click or input uses the screen shown before switching windows | Refuse without delivering or replaying it into the new document | Actual Chrome stale-window token refusal and native pre-dispatch/uncertain-delivery cases passed |
| SS05 | A foreign/no-opener window or a popup callback appears | Do not adopt unrelated control or callback authority; no callback code escapes into a frame or network | Native foreign/no-opener/depth refusal passed. Actual Chrome popup callback interception, fixed blank response and return to root passed |
| SS06 | Cancel, time out or lose authorization while the provider window is open | Original and descendant windows are cleaned; uncertain cleanup keeps the existing recovery fence | Actual Chrome cancellation with popup and original-root cancellation passed. Native unanswered-input timeout/cancel passed; installed authority-loss acceptance remains separate |

Synthetic Chrome checks and actual provider acceptance will be recorded
separately. An implementation, a mock CDP result or a login page alone does not
complete SS01 or the primary real-authentication journey.

## Correction and retained failures

The auth-only controller now follows a bounded opener chain in the same fresh
browser. A proven click/Enter can admit a related window. Chrome pauses its start
until the controller installs callback interception and the existing page settings.
The original root remains the only callback authority. The product has no new
general tab or script API, and changing the shown window clears frame proofs.

The first implementation passed native checks but timed out in actual Kiri
Chrome: `window.open` stalled the original click while the controller waited for
that click's reply before preparing the new window. The correction retains one
exact command-id/session reply while preparing and resuming the popup. It never
re-sends the input. A second actual regression returned the wrong failure after
cancellation; cancellation is now checked before reading the closed browser.

Independent official Claude Sonnet5.5/medium review found races when a window
disappeared during admission or before input validation, and unbounded new
reconciliation paths. Recovery now requires Chrome's exact detach receipt;
unrelated protocol errors still fail closed. Reconciliation, input and replacement
remain within the original cancellation/deadline. After dispatch, an absent
acknowledgement is a distinct uncertain-input result, not a claim that nothing
was sent. Its actor permits viewing the surviving screen without replaying input.

A popup callback is answered with the fixed blank document and its window closed.
Simply failing the request could allow Chrome's error page to show its address.
The synthetic callback test checks actual interception before network and return
to the root, not merely absence of a delivered code.

## Linux scope

Tests run on the existing disposable Ubuntu24 guest, in its Kiri engine with the
pinned Chrome158 Google-auth image and `--network=none`. Only an isolated test
binary was mounted; the accepted c3 installation and production were not replaced.
Four actual SSO cases passed: immediate return, popup callback, background root
completion and popup cancellation. All six original real-Chrome callback cases
also passed. The source's 182 relevant native checks and an additional private
actor uncertainty/recovery check passed; final narrowed cleanup checks passed.
The first image lookup used the wrong local Docker engine and did not run a test;
the correct engine was Kiri. Failed attempts remain in the private work log.

The final independent review found one additional authority-lifetime defect:
later text/scroll/Tab input could revive an earlier expired click's popup arm.
Every newly validated input now clears the previous arm before Click/Enter may
create its own. A regression exercised all three non-arming inputs. Official
Claude Sonnet5.5/medium independently confirmed closure with no remaining material
finding; the observed review model matched the requested one.

The final real-Chrome SSO run additionally typed synthetic text through the same
frame proof, immediately closed the window and reopened a new window whose field
was empty. All four cases passed again. Final affected native tests and the full
gateway compile check passed. The existing six callback results remain valid;
no provider account, token or mail was used in these tests.

Some close/error-receipt orderings conservatively fail or report uncertain input;
they are not claimed as seamless recovery. Each reconciliation/input step and the
replacement are bounded by the original session deadline, not by a single new
15-second end-to-end promise. Matched candidate/production deployment and actual
Claude authentication, inference and Gmail conversation remain open.
