# Store retry consolidation — 2026-11-16

Baseline: `f1e58e9ead3e747e73408234e9adf0c0b2115aab`.

Feature stability includes removing duplicated responsibilities as well as
verifying outcomes. This unit consolidates seven retry loops: runner authority,
usage, output, terminal settlement and generic local operations, plus mail send
receipt and reconciliation persistence. Their common mechanics now live in
`moosuri-agent/src/store_retry.rs`; replaced loops are removed.

Local operations still retry only Busy, at most five attempts with 20 ms between
attempts. Unavailable returns immediately because a write acknowledgment could
have been lost. The separate existing run-lease check retains Busy/Unavailable
retries; Store may settle an expired/revoked run while checking but never dispatches
work. Revocation, conflict, budget and all other errors stop retries immediately.

The same caller retains its payload, lease, usage/output order and change signals.
Network send, Sent lookup, model dispatch and tool IO remain outside retry closures.
Mail paths omit only their former extra sleep after the last failed attempt.
The collector's distinct three-attempt budget remains separate. No API, schema,
permission, model default or production service changes are made.

The earlier authority-pressure test moves with its implementation. One local
receipt test covers successful Busy recovery, a lost acknowledgment that must not
repeat a committed write, conflict and bounded exhaustion. Existing actual stream,
cancel/delete, shutdown, mail-response-loss and source withdrawal scenarios verify
the caller behavior. They use synthetic data and loopback services only.

## Native acceptance and independent review

All 12 focused native agent checks passed, including actual stream continuity,
cancel/delete fencing, shutdown, and four synthetic mail reconciliation flows.
The approved-send reply-loss fixture observes exactly one provider POST across
automatic ticks, manual status checks and restart. The full evaluator passed
15 worlds and 62 controls. Scoped all-target agent Clippy with the credential
broker feature enabled passed. Evaluated source SHA-256:
`a4573a022c052d7e46ae685e15f0d969c1492e8355af10cba26f9c481cde7345`.

Independent frozen review of all five changed source files found no material
issue: error policies, bounds, original payload/lease reuse, external IO exclusion
and signal ordering remain intact. This is behavior-preserving consolidation,
not a claim that seven new product defects were found or fixed.

Native execution, independent frozen review and exact-public-source Linux results
are recorded below when complete. Automatic CI remains disabled. No live model,
real mail send or production deployment is part of this unit.
