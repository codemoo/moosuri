# Claude connection product integration — in progress

This candidate connects the existing administrator model setup to the root
preparer, private official login bridge and owner-bound worker. This is development integration and is not yet deployed. The production bundle remains dee84754. No real provider
account authentication or inference has been performed for this candidate.

The primary acceptance set remains the existing61
[user journeys](../../openspec/changes/automate-installation-google-app/journeys.md).
The checkpoints below expand M05/M06 without turning preparation checks into
completed end-to-end journeys.

## User journeys and evidence

The existing marked Ubuntu24 installation has two synthetic members, real TLS
with a supplied synthetic certificate, separate member containers and Vaults.
Tests use its real gateway. No developer account, token or private configuration
is copied into the product. Actual provider login and natural-language Gmail
acceptance remain distinct gates.

| Journey | Actions and expected outcome | Current evidence |
| --- | --- | --- |
| S01 | Ordinary member starts/reads setup; anonymous and foreign-owner requests are refused before host preparation | Passed on actual installed gateway; 2026-11-27 |
| S02 | Admin starts on an unconfigured installation, retries the same request, changes an input, reaches the official URL and submits malformed code | One worker/request; changed input conflicts; invalid code refused; Passed on installed candidate 7d3a9862 |
| S03 | Reload the initiating page, then open another session of the same account | Original session resumes URL; other session sees progress but cannot submit or cancel login; Passed on installed candidate 7d3a9862 |
| S04 | Cancel while the official page awaits a code | Confirm owned environment removal before a successor; model config unchanged; Passed on installed candidate 7d3a9862 |
| S05 | Restart the gateway while awaiting a code; reload, attempt a new setup, clean interrupted setup and retry | Persisted full-owner receipt blocks replacement until confirmed cleanup; Passed on installed candidate 7d3a9862 |
| S06 | Cancel during host preparation | Await host termination; do not release the setup slot while a child still runs; Passed on installed candidate 7d3a9862 |
| S07 | Log out the initiating session during login | Recheck session, cancel and clean; no model connection is published; Passed on installed candidate 7d3a9862 |
| U01 | Switch API/subscription, inspect defaults and consent gate at desktop/mobile widths | Passed in actual Linux Chromium on 7d3a9862 |
| U02 | Start from the page, reach URL, refresh, inspect private code field and cancel | Passed in actual Linux Chromium on 7d3a9862 |
| U03 | Ordinary member opens the management route directly | Neither credential form is exposed; passed in actual Linux Chromium on 7d3a9862 |

The H01–H06 Linux supporting checks passed on candidate
`6cf5e410b145b16b580c659281b817dd7bfee034fcff81e3604abdb56597bcbe`:
bounded2GiB home and actual service limits, interrupted storage-record recovery,
foreign pending-record refusal, root0440 file read/write boundaries, native
admission with the bounded home, and home ENOSPC followed by exact replay.
H04 initially failed because the test's umask made its synthetic directory0700;
explicit0710 fixed the fixture. The failure remains in the report. Passed H01–H03
were retained when H04–H06 resumed; they were not counted twice.

The initial integrated code passed40 selected Rust checks and17 UI checks.
The recovery follow-up passed19 UI checks, typecheck and web build. These are
supporting checks, not successful account authentication. Those were intermediate results; final focused checks, installed gateway/browser
acceptance and independent closure are recorded below.

## Recovery and authority

The root record distinguishes a worker that answered from a connection actually
published to the broker. Exact config intent is persisted as a digest before
publication. Unreadable or mismatched config is never treated as absent. An
orphan may only be cleaned by a current session of its exact full owner; another
administrator's role is not permission to use that owner's credentials.

Cancellation deadlines reach the host command process group; the gateway waits
for the host result. Short native user allocation remains an atomic bounded
systemd transaction. Root-generated profile/email/worker inputs are read-only to
the dedicated worker. The exact helper binary is retained by content hash, and
admission also binds the daemon stack. Cleanup records environment retirement
before deleting its remaining configuration files.

The gateway keeps /home and /root inaccessible while allowing /run/user sockets.
Only its derived systemd directory is writable under /etc. The native OS account
allocator runs in a separate bounded transient unit, preserving its account and
subordinate-ID locks. Older useradd is admitted only with configured native
subordinate-ID ranges. This code path still needs actual Ubuntu22 acceptance.

## Independent review and open work

Official Claude Sonnet5.5/medium authored isolated host and gateway patches;
main inspected/integrated them. A separate37-turn read-only host/storage review
found the gateway's hidden/run/user, reboot start-limit exhaustion, interruption
and daemon-provenance gaps. Corrections and installed acceptance are complete for the initial login/cancel
scope. Subsequent frozen gateway reviews and closure are recorded below.

Open lifecycle gates are retained explicitly: replacement/disconnection of a
published subscription, cleanup of a revoked or changed owner generation,
reclamation of the maximum64 prepared identities/filesystems, per-member
enrollment, and remote-client authentication inside the
same Kiri. The current UI uses the official link/code handoff. It is not the
finished same-Kiri flow. Actual fresh model authentication/selected-model
inference, fresh Google login, both members and final source-install/reboot/
production acceptance are still required by the goal.

## Reproduction

- `deployment/kiri/linux-vm-smoke/claude-preparation.py --product-boundaries`
  requires the marked disposable guest and a native candidate.
- `deployment/kiri/linux-vm-smoke/installed-claude-connection.py` exercises
  the installed gateway with root-private synthetic fixtures.
- `deployment/kiri/linux-vm-smoke/clean-installed-browser.py --claude-journeys`
  uses the existing Linux Chromium/TLS harness and `installed-claude-ui.py`.

Receipts contain IDs, phases, hashes and fixed result categories. Authentication
URLs, one-time codes, cookies, passwords and provider response bodies are not
retained in the published evidence.

## Installed integration pass — 12:38 KST checkpoint

The actual installed gateway refused ordinary-member, anonymous and foreign
requests (S01). The next scenario failed during host identity preparation before
any official authentication URL was returned. The attempt retired without
publishing model configuration. The original failed result is retained; the
remaining installed scenarios are not marked passed. Investigation continues
against the actual gateway namespace, not a separately privileged test launcher.

Independent gateway review found an early-failure retry dead end, polling that
could overwrite root-publication recovery state, and uncertain-fsync handling.
The candidate now leaves unseen attempts untouched, makes polling read-only,
keeps exact-but-unmarked configuration pending, and recovers the root mark through
the current full owner's authenticated session. A failed config fsync is repaired
by rewriting exact bytes through a fresh private file and rename. Terminal receipt
failure keeps the setup slot occupied; API publication checks that same receipt.
No active credential is silently abandoned to clear a pending flag.

The corrected UI passed 19 tests, type checking and web build. The first combined
web/native run failed because web output changed while Rust embedded it; builds
now run in dependency order. A later Rust run passed 58 checks and found two new
root-store fixture failures (missing synthetic /var/lib); fixtures were corrected.
Final results are still pending, and no real provider login was attempted.

Next lifecycle unit: explicit subscription disconnect/reconnect, API replacement
cleanup, restart/reboot recovery and stale-owner reclamation. A separate official
Claude source investigation defined eight lifecycle scenarios. It confirmed that
seamless subscription replacement needs a standby attempt and rollback; the
current single-attempt Active record cannot provide it. This is open product
work, not a successful replacement feature.

### Lifecycle scenarios queued after installed initial connection

These remain supporting M05/M06 scenarios; none is marked complete here.

| ID | User journey | Required evidence |
| --- | --- | --- |
| LC01 | Connect, disconnect, then reconnect | Current binding removed; exact worker/credentials retired; reconnect reuses only the same owner's runtime with a fresh login environment |
| LC02 | Restart during disconnect or final publication | Resume the same intent; no ready state before both durable config and root mark; no deletion while config is uncertain |
| LC03 | Replace a subscription with an API key | New config durable and usable; old owned worker and credentials removed; returning to subscription is possible |
| LC04 | Cancel or fail a replacement login | Show accurately whether the original connection remains usable; no orphan credential or permanently occupied setup slot |
| LC05 | Reboot connected and disconnected installations | Connected worker restores exact owner; disconnected worker cannot revive through an enabled unit |
| LC06 | Revoke/change an owner generation and reclaim its retired runtime | No active owner touched; exact identity, mount, storage claims and receipts checked before release; stale data cannot follow UID reuse |
| LC07 | Two members enroll and one disconnects | Separate worker, credentials and data; installation model remains administrator-selected; the other member keeps working |
| LC08 | Corrupt or change config during disconnect | Fail closed with a recoverable state; never remove credentials based on an unreadable or mismatching file |

Actual Ubuntu24/systemd255 reproduced the allocator failure: the transient unit
with `ProtectSystem=full` plus `ReadWritePaths=/etc` still returned EROFS when
creating a harmless file under /etc. With `ProtectSystem=strict`, /etc is writable
and /usr stays read-only. Only the short fixed useradd transaction uses this
exception. The gateway retains its separate restrictions. The installed retry
must prove the product path, not just this diagnostic.

Actual Chromium U01/U03 passed on the installed gateway: correct model defaults,
terms gate, mobile width and no credential forms for an ordinary member. Visual
inspection found oversized radio/checkbox controls; the candidate now reuses
the management toggle styles. U02 remains pending until host preparation works.

## Installed results — 12:56 KST

S01–S07 all passed against candidate
7d3a986250a271acce2f8087b8d591bde98e1dff4a832954071bc0cf4bb81ba6.
This includes actual official CLI URL generation and cancellation; exact-session
control, failed-input/idempotency boundaries, process restart, explicit orphan
cleanup, immediate prepare cancellation and initiating-session logout. Every
cancellation left model configuration unchanged. Prior failed executions remain
in separate root-private result files and are not overwritten.

U01–U03 passed in actual Linux Chromium against the same candidate. Screenshots
were inspected for desktop/mobile controls, the empty password code field,
reload and completed cancellation. Authentication URLs/codes and credentials
are not shown in screenshots or published evidence. Subsequent visual cleanup
keeps radio text beside its control and removes obsolete instructions to manually
prepare a host worker from the product screen.

Official final closure (18 turns, Sonnet5.5/medium) found the recovery corrections
sound and identified two follow-ups: a later code-handler persist could regress a
terminal receipt, and replacing a ready subscription with an API key could leave
its old Active worker behind. Persist now snapshots under the receipt lock and
preserves terminal disk state. Until explicit retirement is implemented, both
the API route and UI refuse that unsupported replacement. Linux model-setup
checks passed 36 cases after these corrections; moving the guard before provider
catalog IO and the final UI output need their last focused check.

Production remains dee84754; the source integration is a development unit.
Actual provider authentication, same-Kiri rendering, Active disconnect/reconnect,
member enrollment and final-source deployment are not closed by these results.

## Review closure and final checks

The narrow final independent review (13 turns, official Sonnet5.5/medium) closed
both terminal-persist regression and unsupported API replacement findings without
a material regression. Main inspected the full API-preservation assertions that
the reviewer did not finish reading. Existing authenticated subscription changes
are blocked at both the pre-catalog check and locked save until explicit
retirement is integrated.

Final focused Linux model-setup tests:36 passed; UI19 passed; typecheck, web build
and native build passed. Current xtask verified1792 local links. Strict OpenSpec
initially flagged a long requirement paragraph; the paragraph was shortened and
the acceptance scenarios retained for the final rerun. The changed-source scan
covered43 files: one unchanged invalid-PEM marker in an existing test fixture,
zero new findings. No dependency or external asset was added.


Final source binary122b587467b1d65d749fe7774db1f16f8cf7b13007dccd3e0951377319c79926
also passed actual U01–U03, including the late-persist/API guards and corrected
layout. Screenshots were inspected after manual host instructions were removed.
Strict OpenSpec and final plan/link verification passed. The final source scan
has zero new findings; its one unchanged invalid-PEM test marker was checked
against the already-published source. No production change was performed.
