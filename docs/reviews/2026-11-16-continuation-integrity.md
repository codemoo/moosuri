# Continuation integrity — 2026-11-16

Baseline `dd443d03a93f09a8205b9e338f80e14713f2eace`. This follow-up checks
coordinator, tool-result and instruction continuity with synthetic data.

## Unsent instruction drafts

An unsent draft stayed pinned to its old attempt after the same mission resumed.
The shared composer then disabled Send indefinitely. The same problem affected
a separate new draft typed while an older submitted request was being reconciled.

The composer now follows a newer attempt for an unsent draft only when run and
thread still match. It persists that forward target; a stale view cannot rewind
it. No POST occurs until the user sends. An unresolved submitted request keeps
its exact original UUID, attempt and text for receipt reads or explicit replay.
Another task never inherits the draft.

Two interactive regressions failed on the unchanged production hook: Send stayed
disabled in both workflows. With the repair,14 instruction tests and45 tests
across instruction, mission/chat draft continuity and chat-event state passed.
The controls cover stale views, another carrier, preserved future text and exact
uncertain-request replay. These are React/DOM tests with a synthetic HTTP fixture,
not a live browser or provider acceptance claim.

Independent frozen review found no material issue in draft ownership, monotonic
target persistence, effect closures or immutable pending-request handling. The
TypeScript/worker checks and Vite production web build passed.

## Stored guidance and evidence

- [Provider connection guidance](2026-11-16-connection-guidance-continuity.md):
  the connection-wait promotion duplicated attempt SQL and omitted the shared
  instruction migration. Stable requeue now retains both received and already
  applied guidance with immutable original receipts, model/tool accounting and
  deadlines. Both new baseline regressions failed and fixed outcomes passed,
  including the capped attempt boundary. This repairs a documented Store
  foundation; there is no new ordinary UI/provider-wait workflow.
- [Mixed native/source results](2026-11-16-native-result-continuity.md): one
  serializer preserves actual saved-source receipt addresses in fresh and
  resumed foreground rounds. Signed synthetic native waits now continue to
  source-bound arithmetic without guessing addresses or repeating reads, in
  either tool order. The unchanged baseline failed on the missing address.
  Separate frozen review found no material identity, scope or trust regression.

## Feature-enabled discovery defect

The first integrated native run passed142 Store checks but failed the held-stream
guidance test when `credential-broker-chat` was enabled. The same test had passed
without that feature. Fresh foreground profiles unconditionally admitted both
native model-mail and all terminal credential tools, so ordinary conversations
correctly failed the live-guidance exclusion for native stages.

Fresh chat discovery now starts from the original request. Native mail uses the
same mail-domain predicate required by actual native dispatch. Terminal Google
routes reuse their existing whole-request predicates. Repository discovery keeps
a conservative slash bound, preserving every supported owner/repository command,
canonical GitHub URL and Korean suffix. These are catalog restrictions, never
execution authority; exact resource, owner, session, binding and receipt checks
remain. Requested native routes remain discoverable before a device is connected
or unlocked. Retry/resume keep their frozen profiles. Existing profiles are not
upgraded or silently rewritten.

The extended held HTTP-stream test passed with the feature enabled: final/tool
continuations retained guidance, malformed/refused responses closed it, and a
later instruction plus fabricated native call created no broker job, native read
or commitment. Native schemas stayed absent from those ordinary model requests.
Mail mentions or slash-containing text may conservatively retain native discovery
and remain outside live guidance. Connected mail/browser and native operation
support are not expanded by this repair. Integrated checks and independent frozen
boundary review are recorded below.

## Historical fixture maintenance

The previous10 baseline failures are resolved as test maintenance, not weakened
product checks. Seven incomplete reverse-downgrades now construct an actual
registered migration prefix and project only columns present at that boundary.
Three old unsupported-effect tests now verify the existing earlier `Forbidden`
rejection, empty tool slots and dispatch ledgers, and no repair round. The older
rollback, identity, ownership and no-answer-replay outcomes remain. See the
[maintenance review](2026-11-16-mission-fixture-maintenance.md).

## Integrated native acceptance

Rust1.89 locked/offline checks passed. The142 default-feature Store checks
were retained from before the request-scoped discovery refinement, which changes
only broker-enabled behavior; affected broker/guidance checks were rerun:

| Scope | Result |
| --- | --- |
| `mission-continuity` | 142 passed |
| `broker-discovery` | 21 passed |
| `native-source-guidance` | 11 passed |
| `guidance-default` | 1 passed |
| `evaluator` | 10 passed |

The standalone evaluator passed all15 synthetic flows and62 required independent
validation controls. Its duplicate full-flow library test was skipped in the
native library invocation; the standalone report retains that complete result.
Scoped Store/agent/evaluator all-target Clippy with the native-mail feature and
`--no-deps`, and CLI/evaluator builds passed. The45 React continuity tests and
TypeScript/worker/Vite production build passed. Frozen independent reviews
covered the UI draft lifecycle, native receipt identity and connection requeue;
separate frozen review and root inspection also checked that test maintenance
retained the former meaningful outcomes. The fresh-chat admission boundary and
final negative-model test delta also received independent frozen review.
The existing real Gitea/browser test remains ignored in these synthetic runs.

The intermediate broker-enabled group passed9 checks and failed2 old negative
Google tests: they waited only for Completed, but an unadvertised tool now returns
Incomplete/ContinuationUnsupported. Updated tests retain all negative inputs and
the affirmative wait control, and verify exactly one model call, zero tool slots,
zero broker jobs/commitments and empty authenticated polling. No production gate
was relaxed to pass them; the original failure logs are preserved privately.

Public commit and exact-public-source Linux acceptance follow this checkpoint.
No production deployment, real provider/device acceptance or broad model-quality
result is claimed. The new mixed round proves preserved receipts and no repeated
search/read in those workflows, not a general latency or token-saving percentage.
