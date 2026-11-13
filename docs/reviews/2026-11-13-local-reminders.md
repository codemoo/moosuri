# Local reminders — integrated verification

Date:2026-11-13. Fixed local reminders/inbox, integrated as schema17 after personal
artifacts16. The initial isolated implementation was based on df450f8/schema15.

The runtime polls SQLite every5seconds and never invokes a provider/model/mail/Push
API for reminders. Owner grants use tenant/account/auth_epoch/current revision at
enqueue/claim/final atomic inbox+receipt commitment. Current account-status checking
is a subsequent account-lifecycle integration requirement at the single eligibility predicate.

Pure and full Store fake-clock tests use exact America/New_York2027 gap/fold UTC
oracles, anchored intervals and multi-year coalescing. Actual SQLite close/reopen
tests distinguish uncommitted claims from committed notification receipts. Owner lists show an epoch-stale grant as paused immediately, even before its next
due poll; explicit resume rebinds current authority. Held
claims are fenced by pause/edit/revoke and password epochs; logout does not create
a new session or revoke the separate grant. Queue bounds and persistent two-owner
round-robin are checked. Inbox pagination, quiet overnight boundaries and read/
dismiss tenant isolation use real Store methods. The local publication fault fires
after the notification insert, before delivered receipt update, so rollback must
remove the partial inbox effect. Expiry/paused one-shot and restore-quarantine
cases explicitly reject false delivered states.

The HTTP test creates through actual authenticated routes, waits for the supervised
five-second worker to deliver a fixed notice, changes controls/settings, shuts the
runtime down, reopens the SQLite Store and verifies the same read receipt without
another notification or chat run. Other-account control/history/inbox mutations
fail. The React tests exercise creation/preview/control revisions, quiet/read/
dismiss without Push permission, late-account mutation responses, stale refresh
after pause and preservation of unsaved quiet-hour fields with their original CAS.

Focused Store9 and authenticated HTTP worker/restart1 cases pass; React page,
workspace and public-shell cases are recorded with the final source/log hashes in
the private delivery artifact. The UTC-second/duration editing regression keeps
original anchors exact, including non-whole-minute intervals. Native deep-link
and service-worker navigation allowlists include /app/schedules; private inbox/
schedule APIs remain network-only. Early compile/test logs remain separate: a queue test
initially held a claim past its30second TTL, a HTTP fixture used a private field
instead of its public accessor, and a UI selector matched two labels. These were
corrected without weakening the production deadline, account boundary or UI state
checks. No real model/semantic mission, external send, WebPush or whole-workspace
regression result is claimed by these focused checks.

Timezone review: exact chrono0.4.45 (MSRV1.62) and chrono-tz0.10.4 (MSRV1.65),
std-only chrono, no host timezone mutation/clock feature. Lock additions are these
plus num-traits0.2.19 (1.60), phf/phf_shared0.12.1 (1.61), autocfg1.5.1 (1.0).
All are compatible with project Rust1.89; public crate metadata/source were fetched
for this necessary dependency. No provider/private request was made. The compiled
IANA version is recorded rather than advertised as latest.

The implementation is a partial F29/F30/F32/AUTO01/AUTO02 slice. Natural-language
capture, weekly rules, long model tasks, changes-only observations/F31/AUTO03,
external-send/AUTO04, Push, topic mute, history GC and full restore/latestness remain
outside it. Restore quarantine is an integration hook, never ordinary-startup
disablement. No private traces, credentials, old code/tests, commits, delegation,
deployment or shared build target were used; builds use run/schedules-target.

## Integrated acceptance

Independent source review accepted the frozen reminder implementation. A separate
oracle checked Lord Howe's30-minute DST gap and Apia's skipped date, and140
coalescing comparisons across5zones and downtime windows up to730days.

Merged schema17 preserves personal artifacts16 and automatic-memory/destructive
journal startup hooks. On macOS and Linux x86_64: Store9 reminder cases,7 migration
cases,59 agent cases (one explicit live-model opt-in ignored), and one real HTTP
five-second worker/reopen case pass. Linux's embedded-installation case also
passes. Linux source was hash-verified and mounted read-only; the container had
no network. This is77 Linux checks, not a new full-workspace total.

React32 reminder/workspace/public-shell cases, typecheck/production build,
generated DTO equality and affected all-target warnings-denied clippy pass.
Account suspension and historical restore integration are subsequent units. The
quarantine helper is reserved for restoration and never ordinary startup.
