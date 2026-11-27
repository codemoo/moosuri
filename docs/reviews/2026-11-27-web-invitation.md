# Web invitation completion on installed Linux

The prior invitation UI reserved a member but required a root CLI to supply its
password. The candidate lets an administrator copy a private invitation link and
the member choose a password on `/join`. The existing timer prepares the UID,
volume and sockets; the existing gateway finishes that same claim. No new queue,
daemon or emailed password is introduced. Vault setup remains the member's next
separate step. [Requirements and journeys](../../openspec/changes/automate-installation-google-app/journeys.md)

## Environment and provenance

An untouched Ubuntu 24.04 amd64 VM started without Moosuri, Kiri, Docker, Node,
Rust, GWS, gcloud or provider credentials. It cloned public revision f0812c7a
over HTTPS and completed the source installer. Bundle hashes/revision, two
members, separate encrypted Vaults in actual Chromium, real reboot, and six
proxy egress/isolation checks passed. That baseline used the former root CLI
for the second password; it is infrastructure evidence, not web admission.

After baseline acceptance, all host registry consumers were switched together
to candidate binary SHA256
`b1c8a28dee1dc8c57b8efbf68f7daeba57d84f80d81a06edf1c84c61ec96d4bb`.
The two enrolled members were preserved. Member/browser images stayed at the
baseline revision for this disposable integration run. This does not constitute
a final matched bundle or production deployment.

## Observed user journeys

The actual installed gateway, private password initializer, timer, SQLite,
systemd services, TLS proxy and Chromium were used. No API reply was mocked.

| Journey | Observed result |
| --- | --- |
| J01 | Admin reserved a member and made a link in the UI. Mobile viewport joined with a member-chosen password. First login required no password change. The web-created members later opened and saved their own Vaults. |
| J02 | Replaced and deliberately expired links were refused. New links retained the same allocation. Malformed-token cases additionally passed store/HTTP checks. |
| J03 | The harness let the real redemption HTTP request finish, then aborted only its reply. Status reported completion; the original password logged in. |
| J04 | A root watcher stopped and killed the gateway at the recorded `initialized` stage before activation. The route stayed reserved through the timer. A valid link resumed with the original password and allocation. Real VM reboot retained both web-created members. |
| J05 | Actual admin UI suspend/resume invalidated the old link permanently; a new link completed the same allocation. Issuer demotion/replacement boundaries were verified by registry tests, not an additional installed admin account. |
| J06 | Full capacity returned a pending state and cleared the submitted password. Increasing both UID and disk capacity allowed the timer to prepare the same reservation. |
| J07 | Completed replay with a replacement password reported `password_set=false`. Separate members had different UIDs/volumes and no admin access. After reboot, original-password login and exact HTTP401 rejection of the replacement passed for both. Simultaneous-submit exclusion additionally passed component tests. |
| J08 | The join fragment was removed; passwords/capabilities were absent from persistent browser storage. Reload requested the original link. Foreign-origin status and member administration were refused. |

The first test runner incorrectly counted occupied UIDs as available when
increasing capacity. The marked fixture's UID pool was corrected during the
waiting run, and the harness now counts unoccupied UIDs. This was a test fixture
defect; no member was removed/recreated and no product credential step was used.
The initial replacement-password check accepted any non200; the post-reboot
check strengthened it to exactly401 and passed. Later fresh-install acceptance
also uses the web invitation API, not the former CLI password step; that revised
full-install harness still needs execution against a new matched bundle.

## Review and checks

Official Claude Sonnet5.5/medium independently reviewed the implementation
(37 turns), then the concurrency/expiry corrections (14 turns). Blocking child
work moved off async workers; status no longer takes the initializer permit;
expiry is sampled after the registry lock. The first review's cursor-starvation
finding was refuted by the actual empty-page wrap and installed capacity recovery.
The follow-up found no material defect. Distributed invalid-token load on the
blocking pool remains a low-priority hardening item. If expiry occurs after
password save but before activation, an administrator reissues a link; expiry
is not relaxed and the existing password is retained.

Linux evidence: registry51/52 initially, followed by correction and passing of
the sole stale schema-version expectation; link-store10, member-chosen
password1, gateway4, worker3, installer15 and frontend16 passed. TypeScript,
web and host builds passed. Plan/link verification and strict changed-OpenSpec
validation passed. Workspace formatting still reports pre-existing differences
in unrelated files; none of the changed Rust files appears in those differences.
The changed public-file scan found no secrets. No dependencies or external code
were added; new work follows the repository's MIT/Apache-2.0 licensing.

## Remaining acceptance

The candidate is not yet deployed to production. A final same-revision bundle,
new-source installation and compatible registry12 host upgrade remain required.
Google's fresh-login rejection, actual automatic OAuth publication/recovery,
two real member consents and a valid product model connection for natural-language
Gmail are still open. This report does not turn API, synthetic or static results
into those end-to-end successes.
