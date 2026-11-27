# Installed shared Google app — distribution and recovery

These six supporting journeys expand S01/S03/S05 and R05/R06 in the existing
61-journey plan. They use a synthetic app registered through the real admin API
in the retained marked Ubuntu24 installation. The gateway, GWS relay, two member
containers, private files and browser UI are real. No Google login, consent,
token, mail or model inference is involved. Automatic Console app creation and
two real members' Google consent remain open.

| ID | User journey or injected interruption | Observed result |
| --- | --- | --- |
| GA01 | Anonymous/member registration, foreign member assertion, wrong callback; then current admin. | 401/403 and exact callback rejection; no app written by denied requests. Passed. |
| GA02 | Admin registers the installation app while both members already run. | Both receive the same app automatically, with different private capability keys, registration revisions and owning UIDs. Files remain 0600; neither account is marked connected. Passed. |
| GA03 | Reconcile a successful save as if its reply was lost, replay it, then submit a different client ID or secret. | Status/replay succeed; both conflicts return409. Original files and inodes remain unchanged. Passed. |
| GA04 | Stop and start the first member. | Sibling keeps serving; first member returns with its original registration. Passed. |
| GA05 | Source app has invalid permissions during status/save. | Admin receives503, existing member registrations survive; restoring0600 recovers status without a new app. Passed. |
| GA06 | Restart gateway and GWS relay, then reopen both members. | Same registrations survive; neither falsely gains consent, cross-member assertions return401. Tested journal and current argv contain no synthetic app secret. Passed in a separate resumed run. |

GA01–GA05 passed in the initial run. GA06 encountered a harness TypeError when
a transient restart response contained bytes instead of JSON. After the harness
accepted both response forms for its secret check, a bounded second run passed
GA06. That second run took a new registration snapshot: it proves preservation
across **its own restart**, not continuous preservation across the gap between
runs. The original failure and both reports are retained. This is not a claim of
one uninterrupted six-case pass.

The independent official Claude Sonnet5.5/medium source review identified that
resume gap, overly broad retry admission and early renaming of the report. The
public harness now records registration digests/inodes/permissions/UIDs and script
and fixture hashes. Resume checks the prior registration snapshot, permits only
the recorded TypeError class and copies prior evidence without removing it.
Legacy reports without that snapshot cannot use the new resume path. These
evidence-handling additions received syntax validation and focused review; they
do not retroactively strengthen the historical runs. No product code changed.

Actual Linux Chromium then passed three UI checks without mocked APIs: the admin
sees the registered installation app; the admin and ordinary member both retain
“Google 연결” after reload and have no disconnect action or inherited account.
The ordinary member cannot open installation setup. Opening/reloading details
made no Google POST. Desktop and390px mobile screenshots were captured; the mobile
screen was inspected and has no horizontal overflow.

Gateway SHA-256: `445e2308728df991aae67f36f22b0e2fbecd499228d254b97f0ed508cbe675f8`.
GWS relay SHA-256: `7b7d5f4fbb409ee677e5f8428f0381bc2bee0820e8350bd93909a5d8f5f9fc80`.
Both member images: `sha256:c9fdfbea39313e4f5a299021c9f2f63f0c3edcd1688419b8fb5c19886027c188`.
This remains an overlay on the earlier accepted installation, not a matched
final-source reinstall. The synthetic app stays in this disposable fixture;
production remains dee84754 and is untouched.

Reproduce on a fresh marked two-member fixture with:

```sh
python3 installed-google-sharing.py --fixture /root/moosuri-clean-installed.py \
  --output-dir /root/new-google-sharing-evidence
```

Then run the Linux host's `clean-installed-browser.py --vm-root <marked-guest>
--output-dir <new-ui-evidence> --google-sharing-journeys`. No private fixture
password or app secret is printed. The privacy evidence is scoped to observed
API replies, the queried service journal and a current argv snapshot; it is not
a claim about all possible logging surfaces or real provider secrets.
