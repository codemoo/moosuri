# Google connection reload and reentry — candidate acceptance

RE01 reproduced on the actual installed Linux candidate9f5e0e1a: after selecting
Google connection and unlocking Vault, reloading the assistant loses the job in
UI while its key-only capsule remains. Another Connect attempts a second Start,
receives409, and offers neither resume nor cancellation of the first job.
No credential was delivered. The diagnostic finalizer removed its owned key.

This unit expands R01/V06 within the existing61 primary journeys. It retains only
bounded job/member/login/service/continuation identifiers in sessionStorage;
the existing server status and authorization remain authoritative. No password,
sealed packet, Google identity, callback URL/code or completion handle is stored.
The explicit click reserves a blank popup before asynchronous status reads, so
Chrome activation is retained; only a current Vault state receives a new handoff.
The UI must allow the same user to resume or cancel, with no new Start on resume.

| ID | User journey and observable result | Status |
| --- | --- | --- |
| RE01 | Reload before account choice; resume the same job/key and choose only after fresh handoff. | Actual installed pass after correction |
| RE02 | Start reaches server but reply is lost; reload/resume reconciles it without another Start. | Actual installed pass |
| RE03 | Delivery or later status reply is lost; canonical status continues that job without reauth or automatic credential resend. | Actual installed preflight status-loss pass; accepted-provider branch synthetic only |
| RE04 | Cancel saved work, or discover it already expired/cancelled; confirm terminal status, clear only that record and allow a fresh attempt. | Actual installed saved-cancel/already-gone pass |
| RE05 | Different member/login/service or late older callback sees metadata; no authority adoption or deletion of newer work. | Actual installed other-login/member pass; service and late-record cases passed in focused tests |
| RE06 | Completion acknowledgment is lost or conversation changes; recover exact completion and original thread/request, with no unrelated read. | Synthetic one-use completion/status recovery and conversation checks passed; real provider completion open |

Source implementation was delegated to one isolated official Claude Sonnet5.5/
medium worker with a four-file UI allowlist and one optional pending helper.
Main owns integration, specification, Linux build/installed testing and independent
review. Production remains dee84754. Final-source installation and authenticated
Google/model/two-member acceptance stay open; preflight success cannot close them.

## Implementation and independent review

The isolated official Claude worker completed40 turns; only its four permitted
UI files and one pending helper were integrated. Main corrected uncertain-Start
cancellation, optional-callback-dependent persistence, delayed popup creation,
wrong-job receipt disposal and stale conversation/completion callbacks. Explicit
cancellation now awaits an acknowledgment before reporting success. A late Vault
message after a paused/finished client run is ignored.

Independent official Claude reviews (13 turns, then5) found lost completion
acknowledgment and cancellation-during-completion gaps. The one-use handle may
already have committed: the UI now rechecks the current member/service mailbox,
clears only the matching pending job when that mailbox matches, and issues no
mail read from this fallback. It shows recovery controls even if another tab has
connected. Cancel is hidden while the final exchange is pending. A minor initial
resume-status abort path was corrected per the closure review and its focused
regression passed; it now sends Cancel rather than leaving a misleading wait.
The real server GET shapes and one-use claim were inspected directly.

Linux focused64 UI checks passed, including original-vs-current conversation,
late handoff, uncertainStart and explicitcancel while initialStatus is pending.
Typecheck/web build and embedded native build passed. An earlier broader126-case run passed125 and retained one preexisting first-message Gmail draft failure: it was reproduced on the
unchanged publicabc37d1c. The diagnostic and a separate21-turn read-only Claude
investigation traced a first-thread selection-ref race. That next unit remains
open; it is not hidden or counted as a reentry pass.

## Environment and evidence limits

A native build first ran out of host space and paused the owned disposable VM
with QEMU `io-error/nospace`. Only the idle task target's incremental cache was
removed. After verifying available space, that same VM was resumed; no guest
reset, production change or borrowed credential occurred. The staging helper's
immediate PID hash check raced `exec`; the actual live hash matched afterward,
and the helper now waits boundedly for the expected executable. Final-source
fresh installation and provider authentication are still separate gates.

Final actual Chromium RE01–RE05 passed on gateway
`feae1b1c419774dd7c28d68a7d9fe622387d00ef9b8187e13ead25f05a91e52a`.
The390px resumed screen was visually inspected; no horizontal overflow and both
recovery actions were usable. All live Deliver requests were blocked and zero
delivery occurred. Final cleanup verified no remaining auth containers/Chrome.
The candidate is an overlay on the retained installation, not a fresh final-source
install. RE03 accepted-provider delivery and RE06 real completion remain unexecuted.

Run `clean-installed-browser.py --vm-root <marked-guest> --output-dir <new-evidence>
--google-reentry-journeys` from this directory's Linux harness. It uses the retained
GV synthetic multi-account fixture, preserves earlier evidence, and records only
verdicts, executable hash, explicit non-executed boundaries and cleanup status.
Strict OpenSpec and plan/link checks passed; no dependency or external asset was
added. Production remainsdee84754.
