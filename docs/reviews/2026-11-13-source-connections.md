# Source connections acceptance — 2026-11-13

Scope: R29/F70, tenant-owned official Gmail/Slack collection and custom HTTP ingestion.
Kakao implementation, private collector/cron/DB, production deployment and actual accounts are excluded.

## Implementation and evidence

- SQLite migration9: connection scopes/epochs, narrow token digests, encrypted Slack tokens, Gmail binding,
  runs, page receipts/checkpoints and revision provenance. The account session is never fabricated for workers.
- Batches reuse the knowledge commit path. Managed collections reject manual mutation bypass. Explicit source
  tombstones withdraw dependent evidence; partial omission never deletes. Empty pages differ from heartbeat.
- Source deletion suppresses subsequent imported copies; unknown external-ID deletes are no-ops and create no
  permanent suppression. Full snapshots remain rejected. This is not backup/restore product certification.
- Five-second scheduler, bounded pages, lease/heartbeat, current authorization on commit, finite error backoff;
  normal page continuation is partial, Slack bounded completion is polled, custom completion is a producer report.
- React connections, account-fenced requests/token cleanup, reconnection/reset, provenance/receipt detail.

Independent review found and resolved scheduler starvation behind ineligible credentials, deletion of receipts
still needed by source provenance, and Gmail backfill's missing includeSpamTrash flag. Rechecks found no
remaining P1/P2 in the reviewed storage/HTTP/runtime and provider protocol scopes. Review is not proof of
all possible defects. Main integration also fixed non-Send URL serializers held across await.
Actual Chromium navigation found a missing embedded-server `/settings/sources` route;
the fix is covered by the real CLI installation journey on macOS and Linux.

## Executed checks

macOS and isolated Linux x86_64 passed318 Rust tests with one opt-in live-model test ignored.
This includes CLI unit2/installation1, agent52, connectors124, server unit2/HTTP26, store109
and xtask2; a nested subprocess result is not counted twice. Strict all-target Clippy, formatting,
React113 behavior tests, TypeScript, production build and generated DTO equality passed.

Linux used a nonroot, read-only source container with no network and a pinned image. The original
376 source files and4 web assets were hash-verified before and after testing; the final two-file CLI
route delta was applied to a fresh snapshot, rebuilt and covered by the installation test.
The actual loopback journey passed12/12 checks with40 HTTP requests: initialization/login,
settings route200, custom connection/token, the Rust sender twice against the same private state,
sequence1 and the same receipt, search/provenance, producer-report labelling, no credential/checkpoint
in history, read-authority refusal, second-account/token isolation, pause and revocation fencing.
The server stopped and owned containers/networks were zero; temporary password/token files were removed.

A separate macOS process test killed the synthetic server after a committed page and before run finish.
All6 checks passed after restart: the page/cursor/receipt/provenance survived, the unfinished run became
unknown, and an exact retry returned the same receipt without a second commit. This complements the
store's simulated journal-before-receipt recovery gap; it is not a backup product certification.

Actual Chromium against the Rust server completed login, direct settings navigation, custom connection
creation, Rust-sender ingestion, history refresh, mobile navigation to its managed collection, source
detail and receipt lookup. Desktop1280×900 and mobile390×844 viewports were inspected; both settings
and source detail stayed within390px with an independently scrollable main region. This is viewport
validation, not an iOS/Android device test. The sole console error was the expected unauthenticated
session401 before login. The one-time token was kept out of browser screenshots and source artifacts.

New cases cover token/tenant/expiry/revoke, exact replay/conflict, invalid mixed-page rollback, opaque checkpoint
and no-change semantics, run generations, account epoch, credential encryption/refresh/rebind, source/memory
withdrawal, durable deletion intents restored across the receipt gap, eligibility fairness, provenance retention,
actual HTTP bearer/session separation, source views and account switches. Existing mail approval tests remain.

The DATA01/03/06/07/08 checks cover this source-connection contract. DATA02 covers Gmail text mapping,
scope, history/pagination and error handling here; its broader Calendar/PDF/attachment and live-account
evaluation remains pending. The overall evaluation-scenarios registry is not relabelled as fully passed.

## Provider and operational limits

Gmail uses the operator's OAuth client and each user's consent, with explicit selected label IDs/start date.
Slack uses a personally imported user token verified by auth.test; OAuth app provisioning is not implemented.
A Slack poll cannot attest all deletions or late replies outside the configured parent lookback. Large or
incomplete MIME/oversized pages fail without checkpoint advancement; attachments are not silently imported.
The development implementation does not claim live Google/Slack acceptance, real mobile-device tests or
production deployment. No real email was sent, no real mailbox collected, and CI remains disabled.
