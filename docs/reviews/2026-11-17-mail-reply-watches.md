# Exact Gmail next-reply watch

This change implements the [bounded product contract](../mail-reply-watches.md)
with synthetic accounts, messages and provider endpoints. There are no live Gmail
reads/sends, private credentials or production changes in this acceptance run.
Automatic semantic summaries are not claimed: the delivered result is a literal
verified preview in the original chat plus the existing inbox/opt-in Push event.

## Native acceptance

Native provider scenarios exercise actual metadata HTTP, body reads, Store leases,
conversational registration and notifications. The counter assertions distinguish
one metadata check from zero body/model calls on unchanged, label-only, old-mail
and outbound-only responses. A reply already present before the first poll is read
once; replay cannot create a second notification. Error, malformed and mismatched
responses cannot claim unchanged success. Stop/disconnect during a held body read
withdraw the late result.

Store scenarios cover owner separation, payload-bound request replay, logout and
restart, stale leases, account epochs, expiry, deletion, credential refresh CAS/app
pins and atomic rollback when notification persistence fails. An initial regression
found that result withdrawal tried to mutate the deliberately immutable notification
source-suppression flag. It now uses the existing dismissal/push-suppression and
live-authority guard. A read-test fixture was also corrected to use the real
completed no-call agent step rather than bypassing agent settlement.

The web tests exercise lost registration responses, literal untrusted previews,
result close, owned-conversation links and late responses after a conversation
switch, together with existing chat/task-monitor/schedule behavior. A final
ChatPanel SSE regression confirms that same-state streamed revisions do not
re-fetch the watch inventory, while task completion refreshes it immediately.
The regular five-second UI refresh remains in place.

The frozen read-only independent review found two further P2 issues: re-arming a
completed watch could repeat its prior reply, and a later read of an older message
could discard a newer observed baseline. The implementation now carries only the
same owner/epoch/connection/app's successful observation cursor into a new explicit
grant and merges bounded qualifying read/send observations. It preserves the new
grant's own cadence/expiry. A follow-up frozen review found these issues resolved
and no new material defect in the changed scope. Before the first publication,
only a test import relocation followed review. The subsequent UI-only follow-up
changes the watch refresh key from run version to run state and adds the SSE
regression above; the reviewed backend remains unchanged.

Native acceptance passed:

- 21 distinct Store contracts across watch lifecycle, out-of-order confirmed sends,
  shared notification mute, existing task monitors and schema71/73 migration/registry.
- Seven agent/provider scenarios, including explicit re-arm without duplicate body
  reads/notices and newer-message→historical-message read ordering.
- One metadata cutoff contract, 45 React chat/monitor/schedule/instruction tests, TypeScript
  checking, production web build, generated DTO equality and product CLI build.
- Scoped all-target Clippy for connectors/Store/agent/server with credential-broker
  chat enabled, and the existing 15-world/62-control synthetic evaluator.

The evaluation is a regression suite, not a new live-model quality sample. New
mail-watch behavior is covered by the separate synthetic provider/Store/web
contracts above. Initial failed checks and fixture compile corrections are retained
locally; final successful evidence is distinct. No live Gmail account, real mobile
Push delivery or browser interaction acceptance is claimed.

CI remains disabled. No production deployment.

## Public-source Linux acceptance

Implementation `3009ed677b3695de117179a00132cc7b8639149e` was published and the canonical checkout
fast-forwarded cleanly. All six Linux phases passed: 21 Store contracts, seven
agent/provider contracts, one metadata contract, scoped all-target Clippy, product
CLI/evaluator builds and the 15-world/62-control evaluator. Its source digest
matches the native evaluator. These are platform regressions, not independent
live-model quality or latency samples.

The pinned Rust1.89 container used read-only source/root, disabled external network,
synthetic temporary storage and no production mounts. The built web assets were
included. All source hashes remained unchanged. Source archive SHA-256:
`c470821d6ac6bfc2fd7598f14a5126c06a7ff8404596a3110c16b9a3b2a59cbb`.

Formatting, plan/link validation, strict OpenSpec, source/new-commit secret checks
and generated DTO equality also passed. Actual Gmail eligibility and mobile Push
delivery remain outside this synthetic acceptance.

## Final UI acceptance

The follow-up stream-refresh change passed the five focused React files (45 tests),
TypeScript checking and a fresh production web build. The updated assets passed
secret scanning and were embedded in a separate successful Linux product CLI build
using the same isolated profile. This build captured the working tree before the
follow-up publication; it did not start from an already-public commit. Its source
and asset hashes match the final patch, with only this acceptance record and the
status document updated afterward. Backend and migration hashes are unchanged
from the six-phase public-source acceptance above.

Final UI build source archive SHA-256:
`9e67afb14d1d990ad6937595732f9dcecb3de8e935a6e9183a621824b4cc82b0`.
No production deployment was performed.
