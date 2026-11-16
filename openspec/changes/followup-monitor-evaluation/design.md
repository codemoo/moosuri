## Reuse and ownership

The existing public evaluator owns denominator and token missingness. The new
case reuses shared stage accounting and mission terminal observation. A separate
owner-visible fixture, protocol script, read-only observer and grader keep input
and oracle roles explicit. No duplicate runner framework or product DB writes.

## Interfaces

The authenticated HTTP router handles source collection/ingestion, parent
mission, monitor proposal/list/accept/stop and inbox. Public Store scheduler
methods are the only non-HTTP driver boundary because no tick API exists.
Monotonic synthetic due times stay within the originally accepted expiry and
are recorded; actual Runtime still runs and verifies the parent and changed
child. No synthetic state claims that the real refund occurred.

## Outcome

A real revision-1 read proposes one exact-source monitor. Acceptance replay
creates one policy. The first unchanged tick records a skipped occurrence and
consumes no model/tool budget. One revision-2 ingestion admits a single actual
child and one notice; a later unchanged tick cannot duplicate either. Owner stop
and exact replay revoke policy/schedule and dismiss/suppress that same persisted
notice. Reopen preserves those tombstones and inbox filtering.

## Independent failure evidence

Observe candidate-to-source-read links and versions, policy/schedule identities,
child count, notification identity/provenance and visibility separately. Negative
controls alter duplicate child count, source scope, changed read, stopped inbox
visibility and policy state. Every control depends on a passing positive case.


## Review correction: exact quotes and actionable links

The task explicitly asks to quote the current saved status. Grading therefore
uses independently authored full expected quotes, with whitespace normalization
only, across source receipt, saved candidate and output. Keyword presence cannot
establish that approval is positive or that settlement remains pending. A correct
quote with contradictory commentary fails; this does not extend to arbitrary
summary or paraphrase grading.

Observe the child's persisted schedule/occurrence identity and the saved notice's
exact joined mission linkage. Check the HTTP LocalNotification mission, schedule
and occurrence against those observed values, including the same persisted notice
after stop/reopen. Required controls alter each visible link independently and
append contradictory paid or negated-approval claims to otherwise correct quotes.
Original passing reports predate this independent-review correction.
