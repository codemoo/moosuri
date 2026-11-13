# Gmail unknown-send reconciliation — implementation evidence

2026-11-14. Bounded schema31 implementation accepted after independent review, repairs and integrated native/Linux/browser checks. All provider data is synthetic; this is not live Google acceptance.

## Behavior

The existing frozen final-preview approval and one-shot send commitment remain unchanged. The commitment transaction now stores immutable v1 complete supported-MIME fingerprint evidence before raw bytes are removed. Unknown content is purged; restart cannot reconstruct or requeue a send. Schema29/30 unknown rows migrate with an explicit unavailable oracle, never a fingerprint derived from a candidate.

Reconciliation uses a private typed read lease, immutable snapshot getters and current original owner/account/auth epoch, connection epoch, readonly scope and application pin. It does not fabricate a browser session. Account suspension/recovery/password-change/temporary expiry, disconnect, source withdrawal, expired/stale leases and app changes fence credentials and final observation CAS. The service rechecks between the list and raw GET and after IO through the Store CAS; a late refresh cannot resurrect a disconnected credential. There is no IO in SQLite.

The persisted automatic budget is commitment +0/15/120/600 seconds, max4; a read crash cannot reset it. Authenticated same-origin owner POST `/mail/actions/{id}/check-sent` accepts only `{}` and rate limits manual attempts to once per minute per action. Provider work is GET-only fixed users/me Sent/RFC-ID search, max5 results/one page, followed by one raw GET only for a unique complete result. It never creates a send carrier. Model tools remain read/search/thread/local draft only; every new send still requires its own full recipient/body preview and explicit final approval.

`matching_sent_observed` is historical lookup evidence beside `outcome_unknown`, not a send response or delivery/reading receipt. A later empty/failed lookup records a separate latest result/count/time without erasing the earlier match. Conflicting identities are retained, actual send-response receipt wins, and the UI states uncertainty. Restore quarantines all reconciliation authority and clears leases/due times, alongside existing credential disconnection and pending-send retirement.

The v1 oracle binds the whole supported MIME envelope/headers (including Bcc and reply metadata), encoding and encoded body. It rejects duplicate/unsupported headers, extra parts, unsupported MIME or encoding problems. It normalizes only CRLF/LF, header folding and generated Date. Normal Gmail-added Received, DKIM/Authentication-Results and X-Google headers are outside the v1 allowlist, as are changed encoding or missing Bcc. Therefore legitimate Sent mail may frequently remain unresolved; this strict oracle is conservative and its useful matching coverage on real Gmail is unvalidated. RFC Message-ID correlation does not establish provider idempotency or cryptographic attestation.

## Focused verification

Commands use Rust1.89, shared offline vendor/target, `--offline --locked`, synthetic loopback addresses and private temporary SQLite roots. Filters below overlap; counts are not a total of distinct scenarios.

| Gate | Result / defect oracle |
| --- | --- |
| `cargo test -p moosuri-connectors gmail::` | 15 passed: existing fixed-origin/OAuth/MIME/transport bounds plus complete strict oracle counterexamples |
| `cargo test -p moosuri-store mail::tests` | 15 passed: original approval/nonce semantics, evidence immutability and pre-purge capture, persisted four-attempt budget/restart, same-user/account/scope checks, other-user denial, source withdrawal, late receipt conflict, legacy schema29/30 migration |
| `cargo test -p moosuri-agent mail::reconcile` | 4 tests: actual MailService/Store fixture accepts one frozen POST then drops response; worker/manual/restart leave POST count1. Negative fixtures cover no/duplicate/paged candidates, wrong ID/SENT, missing Bcc, added header/MIME part type, truncated body and changed recipient. Held list/raw GET and token refresh race disconnects suppress late observations/credentials |
| `cargo test -p moosuri-store cutover::restore::tests` | 9 passed: current and schema29/30/oldest supported restore, new lookup quarantine, existing failure/retry/enrollment/receipt preservation |
| `cargo test -p moosuri-server --test http_boundary mail::` | 5 passed: original preview/OAuth/disconnect boundaries plus authenticated POST-only unknown check, rejection of forged fields, manual rate limit and no new dispatch |
| `npm test --prefix web -- Mail.test.tsx` | 8 passed: existing stale-editor/purge/approval fences plus honest unknown lookup/manual owner POST and no approve/send request |
| Type export / TypeScript / web build | Generated shared DTO and checked React/worker contracts; production web build |
| Clippy / rustfmt / plan / OpenSpec | Focused crates all-targets Clippy `-D warnings`, workspace format, `xtask verify-plan`, strict named delta validation |

The hidden synthetic connector constructor is behind non-default `test-fixtures`, accepts only exact numeric IPv4 127.0.0.1 with nonzero port and is enabled only by the agent dev-dependency. The default `moosuri` normal/build feature graph contains no `test-fixtures`. Production origins stay fixed, with no runtime/env/CLI host override, proxy/redirect or send retry. No external dependency or Cargo.lock change.

First-failure artifacts are retained privately: initial restart test retained the Runtime handle after shutdown (fixed test lifetime), concurrent test fixtures exceeded the existing two-KDF budget (serialized with the existing test lock), a direct account fixture violated the existing password/temporary-expiry CHECK (fixed valid synthetic state), a truncated short fixture removed the MIME separator (changed fixture to truncate inside a longer body), and Clippy identified a nine-argument helper (replaced with same-transaction identity reload). These were corrected and relevant gates rerun; no failed gate is reported as accepted.

## Limits and remaining gates

No real provider/OAuth/account/token/recipient data, send, deployment, CI or old service/cron/DB was used. Synthetic tests do not validate Google MIME/Bcc preservation, indexing delays, rate limits, real consent/refresh behavior or recipient delivery. The conservative v1 oracle can leave legitimate sends unknown. Schema31 restore authority remains quarantined; restoring an old snapshot does not revive lookup/send grants. Provider drafts/compose consent, fixed scheduled sends and live provider/device acceptance remain pending separate slices.

The exact changed-path/baseline/new hash manifest and native command logs are private handoff artifacts. README/status integration is root-owned. Calendar30 and vault source bytes are unchanged against the supplied 773-file baseline.


## R1 independent-review repairs

Independent review found a queue fairness defect: sixteen oldest due rows with withdrawn/disconnected or otherwise invalid original read authority could be selected and skipped forever, preventing a later eligible owner from being checked. Actual Store regression on Linux reproduced the failure before the fix. The scheduler still inspects at most16 rows per tick; after a rejected claim is rolled back, Conflict/Forbidden now durably retire only that row's automatic `next_due` pointer. They do not consume the four-read budget, change the original unknown send/equality evidence, clear manual rate limits or restore any grant. Busy/transient leases are rolled back and preserved. Invalid automatic authority stays retired across restart; a manual request still needs every current authority guard.

The Store regression creates eight withdrawn and eight disconnected unknown sends before another owner's eligible row, with a concurrent held manual lease. It verifies bounded forward progress, preservation of the transient lease/manual limit, unchanged send state/budgets and no new send across repeated ticks/restart. The separate constructor regression also failed before the fix: the hidden test fixture accepted other127/8 addresses and IPv6 loopback. It now accepts exactly numeric IPv4 `127.0.0.1` with a nonzero port and rejects `127.0.0.2`, `::1`, zero port and non-loopback addresses. The nondefault feature boundary, production origins and strict MIME header rules are unchanged.

The earlier verification table describes the original freeze. Independent rereview accepted the exact production repairs. Final Linux checks passed: Store mail16, actual MailService4, restore9, connector Gmail16 and HTTP33 (one explicit public-network test ignored). The repaired migration fixture then passed all10 private-file tests; combined HTTP34 plus affected-crate all-target Clippy passed after draft integration. Production's normal/build feature graph contains no test-fixtures. Counts overlap and are not an assistant-quality score.

Two integration failures were retained and fixed in tests: the former account-guard test reused one automatic action after its due pointer was intentionally retired; it now creates fresh work for every invalid account state and checks retirement/no budget use, authenticated manual revalidation, another-owner denial and late scope CAS. A schema26 rollback fixture had left schema31 tables in its artificial old database; it now removes them before testing the late ALTER failure and successful recovery. These corrections change no production policy.

Real Chrome at390×844 with the Rust server and isolated SQLite showed a persisted synthetic historical match beside a later empty lookup, retained unknown state without a send button, and the explicit check rejected by the unconfigured/disconnected fixture (409) without an approval/send request. This validates UI and HTTP boundaries, not a live provider lookup. Shared generated DTO, focused React/typecheck/build, format, plan/OpenSpec and publication secret scans are checked separately. No actual Gmail OAuth, message sending, deployment or CI activation occurred.
