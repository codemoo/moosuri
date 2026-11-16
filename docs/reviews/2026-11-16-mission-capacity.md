# Mission capacity consolidation — 2026-11-16

Baseline: `7019d79e613862764b862757452e36ac45b04e67`.

This unit continues feature stability through code consolidation. Static audit
found duplicated capacity predicates, not a reproduced production defect. New
admission, user-input waits, provider/device connection waits and explicit resume
must agree on the owner slots without sharing unrelated authority decisions.

The mission-owned policy keeps 16 outstanding missions globally, one runnable
slot and two waiting slots per tenant/account. Paused work occupies a runnable
slot. Only new missions consume the global limit; an admitted mission can change
its slot at global capacity. Foreground native chat retains its existing exemption
from mission waiting limits. Admission-order polling, leases, source/credential
checks, budgets and receipt semantics remain with their original callers.

Stable mission and native continuation keep their distinct authority and pending
result checks. The latter supports foreground chat and already retires the old
lease before yielding; its similar attempt-update SQL is not sufficient reason
to merge the two paths.

A new synthetic public-Store workflow fills all 16 places, rejects a seventeenth
owner's request, then waits, answers/replays, pauses and resumes the original task
with unchanged accounting/profile. Cancelling another owner's task admits the
previously blocked request. Existing mixed input/connection waits, paused-owner
ordering, recurring overlap, guidance and native continuation cases remain the
other behavioral evidence.

## Native acceptance and independent review

The full-capacity workflow passed on unchanged baseline production, then passed
unchanged after consolidation. All 18 focused Store checks passed with the
credential-broker feature enabled, including mixed ready waits, owner pause/order,
connection guidance, native foreground/background results, attempt-32 recovery and
recurring overlap. The full synthetic evaluator passed 15 worlds and 62 controls.
Scoped all-target Store Clippy passed. Evaluated source SHA-256:
`4be06f357611a46b72967efa82937f5cb37d7dfdcf520fa00eeb419ce072c1eb`.

Independent frozen review of all seven source/test files found no material issue.
Every old capacity caller was migrated with the same transaction placement, tenant/
account scope and limits. The new full-capacity scenario directly covers input
waits and explicit resume; connection/device promotion retains separate existing
coverage and the same shared runnable predicate. This is consolidation with
behavior preserved, not a newly reproduced production defect.

Baseline, native execution, independent review and exact-public-source Linux
acceptance are recorded below when complete. No live model, real provider/device
operation or production deployment is included. CI remains off.
