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
switch, together with existing chat/task-monitor/schedule behavior.

The frozen read-only independent review found two further P2 issues: re-arming a
completed watch could repeat its prior reply, and a later read of an older message
could discard a newer observed baseline. The implementation now carries only the
same owner/epoch/connection/app's successful observation cursor into a new explicit
grant and merges bounded qualifying read/send observations. It preserves the new
grant's own cadence/expiry. A follow-up frozen review found these issues resolved
and no new material defect in the changed scope. Only a test import relocation
followed review; production source remained unchanged.

Native acceptance passed:

- 21 distinct Store contracts across watch lifecycle, out-of-order confirmed sends,
  shared notification mute, existing task monitors and schema71/73 migration/registry.
- Seven agent/provider scenarios, including explicit re-arm without duplicate body
  reads/notices and newer-message→historical-message read ordering.
- One metadata cutoff contract, 30 React chat/monitor/schedule tests, TypeScript
  checking, production web build, generated DTO equality and product CLI build.
- Scoped all-target Clippy for connectors/Store/agent/server with credential-broker
  chat enabled, and the existing 15-world/62-control synthetic evaluator.

The evaluation is a regression suite, not a new live-model quality sample. New
mail-watch behavior is covered by the separate synthetic provider/Store/web
contracts above. Initial failed checks and fixture compile corrections are retained
locally; final successful evidence is distinct. No live Gmail account, real mobile
Push delivery or browser interaction acceptance is claimed.

Exact-public Linux acceptance will be recorded after publication. CI remains
disabled. No production deployment.
