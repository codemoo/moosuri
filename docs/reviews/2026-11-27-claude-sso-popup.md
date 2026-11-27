# Claude SSO window transition

This expands M05/K03/K04/K05 of the existing Google-installation journeys.
The accepted installation baseline is c3d18bd9. The 3fd8e586 gateway, Google
handler and auth worker patch is now deployed over it. Member/Vault and ordinary
browser components remain the accepted baseline. No provider authentication is
claimed here.

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
| SS01 | Choose Google on the official login screen | Same Kiri screen displays the proven related window and accepts fresh proof-bound input | Failed live on c3. Corrected real Chrome synthetic user-gesture popup, click and text input passed; installed official Google-window display passed; production retry still failed before any account input |
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
15-second end-to-end promise. At source publication the matched deployment and
actual provider acceptance remained open; the subsequent installed and production
results follow below.


## Installed patch and production retry

The release inputs matched all 2,689 public tracked files at `3fd8e586`. An
independent web build matched all 58 embedded assets. The pinned accepted Google
image was extended offline by replacing only its browser worker. The gateway and
socket-activated Google handler were switched together so no old parser receives
the new uncertain-input variant. The accepted c3 installation receipt is retained;
this patch does **not** establish an untouched final-source installation.

On the existing two-member Ubuntu24 fixture, installed K01, K02, K03 and
K05a/K05b/K05c all passed with the new gateway: private origin-only frame, exact
initiating session, foreign-session/member refusal, stale/duplicate input refusal,
explicit cancellation, gateway restart/recovery and logout cleanup. A separate
installed API journey opened the official Claude login, clicked its visible
Google button, displayed `accounts.google.com` inside the same product frame,
and cancelled with the owned auth capsule confirmed absent. No identifier,
password, code or token was submitted.

The first transition observer timed out despite a healthy patched gateway. Its
failure record is retained. A separate read-only observation established ten
stable seconds, matching executable, HTTPS and unchanged data/keys/confinement.
Independent Claude review identified the observer race: a forked PID can remain
unchanged between an initial nonmatching executable and the completed exec; the
stability timer must start on the first good observation even if the PID/state
has not changed. The production operator contains that correction.

Host disk pressure paused the disposable VM during image inspection. Completed
compiler caches were removed, and the VM resumed after space was available. A
superseded inactive test VM disk was subsequently retired while retaining its
records and the current accepted fixture. This is environmental recovery, not an
authentication pass. Kiri's image inspection also differs from Docker: it omits
labels and the original archive exposes compressed layer digests. The image ID,
canonical build prefix and actual worker bytes were verified instead of treating
those API representations as equal.

Production preflight additionally encountered an existing directory-creation
ExecStartPre and an absent optional enrollment file for the original static
member. Both were handled explicitly before services changed. An initial empty
operator-file transfer executed no actions; content/hash verification corrected
it. These failures and the unchanged old service/config evidence were retained.
The final reviewed operator stopped both configuration consumers before its
shared-owner probe, preserved old proxies/configs, used raw file hashes without
opening member SQLite files, and checked the measured candidate receipt.

The matched production patch then passed stable execution, HTTPS, unchanged member
container/PID/mounts, encrypted Vault, registry/Google grant files, keys and model
configuration, unchanged confinement, and unchanged unrelated service processes.
The image is `sha256:ff9a69a17272a52c59774b944434e1e56f3a5e5a6d091e519cc2781b14722d8f`;
the gateway SHA256 is `fcb06ff0a10c9de824ec213be7fe9b2571433a30a58da22e7002e9ab19583a74`.
Independent reviews used the observed official Claude Sonnet5.5 / medium.

A single subsequent production UI attempt reached the official Claude screen.
After the visible Google button was clicked, the product lost its frame and the
operation became `failed`; its root attempt was retired and the authentication
capsule was absent. No Google account input occurred. Unlike the installed
fixture result, this is **not** a production popup or authentication pass. The
existing diagnostic surface retains no specific post-start cause, so the next
bounded step is to distinguish browser/input/poll failure from CLI termination
without logging URLs, input, codes or raw provider output. The same login will
not be repeated without that new evidence. Actual Claude inference, conversational
Gmail, new OAuth registration and two real member consents remain open.
