# Recurring read-only missions — integrated acceptance

2026-11-13. Schema28 is integrated after personal Calendar26 and private files27.
This is a bounded development slice, not an overall assistant quality score or a
production deployment. See [the behavior contract](../missions.md) and
[the active change](../../openspec/changes/recurring-read-only-missions/proposal.md).

## Accepted behavior

- An authenticated owner saves a read-only task, original checks and verified
  model/effort with a once, interval, daily or weekly rule. Time/name edits preserve
  the immutable task, model and scope. Existing IANA/DST/UTC scheduling is reused.
- Five-second idle polling queries SQLite without model calls. An occurrence
  atomically admits one real mission under fair account admission, original
  budgets and a deadline measured from its scheduled UTC time.
- Parent controls, account/provider changes, expiry, restart and restore preserve
  authority and outcome uncertainty. Generic fixed-reminder routes cannot control
  these grants. Restore pauses templates and quarantines unfinished work.
- Results and receipts stay in owned mission history. Inbox entries carry a
  content-free status and a currently valid typed link. Exact successful
  text/calculation/source-receipt equality may suppress a repeated notification;
  unsupported comparisons, review-needed, failure and unknown outcomes still notify.
- Inbox navigation preserves the current chat draft and focuses the owned result.
  Mobile controls have44px targets,16px inputs and independent vertical scrolling.

## Independent findings and fixes

RM-01: expiry previously classified dispatched unfinished work as a known partial
result. The bounded shared expiry classifier now preserves dispatched/external
uncertainty, retires grants and carrier leases, blocks late output, and retains
late usage accounting. The current-run guard also handles actual-clock expiry.

RM-02: four mission transaction callers rolled back those guard-only expiry
effects on error, leaving a stopped carrier running. They now commit the rejected
guard transition while keeping subsequent input/action/SQL failures atomic. An
independent acknowledged-final reproduction and five focused probes passed after
the repair. The combined Calendar/files/recurrence source review found no further
product defect. Final navigation changes were separately checked by root through
focused React tests and the actual embedded browser application.

## Verification actually run

- Native full Store initially passed341, failed one synthetic predecessor setup,
  and skipped two opt-in cases. The fixture relabeled schema28 as19 while retaining
  recurring tables. Only its setup was fixed; the exact predecessor case then passed
  on native and Linux. This is not reported as a fresh full342-test Store run.
- Final native and Linux agent63, server46 and CLI19 cases passed. Their respective
  opt-in cases remain excluded. Linux also passed mission56, scheduler24, restore8
  before the fixture correction, then the corrected predecessor and schema9 checks.
  Filters overlap and are not added into a product-wide unique test count.
- React full272 passed before the final navigation adjustment; the affected
  App/Missions/Schedules48 then passed. TypeScript, production build, generated DTO
  equality, workspace all-target clippy with warnings denied, fmt, plan metadata and
  strict OpenSpec validation passed on the accepted source.
- Actual Chrome390×844 used the built Rust server, SQLite and a synthetic loopback
  streaming provider. A deliberately lost create response followed by the same UUID
  retry made one template. Three interval occurrences each performed planning,
  exact decimal calculation and receipt verification, producing0.3. Only the first
  notified; both later occurrences recorded exact equality. Before due time there
  were only the three setup capability probes and no mission model calls.
- The browser checked pause, daily-time edit, resume, pause again, unchanged
  task/model, restart with the same login and three records, and revoke. Revocation
  removed the inbox link. Inbox→result→chat and server restart preserved the unsent
  draft. Screenshots were visually inspected; no horizontal overflow at390px.

Browser harness corrections were two pre-submit locator retries and a time-edit
waiter corrected from POST to the actual PUT. They were not product failures.
Observed console errors were the initial unauthenticated session, intentional lost
response, HTTPS-only Push setup on an HTTP development origin and the deliberate
server restart. The owned browser and synthetic services were stopped afterwards.

## Remaining scope

Conversational task admission, semantic change comparison, other receipt-type
normalization, mail/browser/vault execution and real-device/provider evaluation
remain separate work. The synthetic provider validates protocol and lifecycle,
not natural-language quality. The earlier live-provider holdout remains unattempted
after its exhausted probe retry budget. No production data, old service/cron,
external send or live credential was used. GitHub Actions remains disabled.
