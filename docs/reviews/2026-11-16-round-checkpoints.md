# Model round checkpoint stability — 2026-11-16

Baseline: `5cd6098fe8b6f4bbfdf7f2c0d5c0962adf79f9bd`.

The mission runner saved received model rounds once, while foreground chat,
isolated planners and Claude already used the bounded local retry policy. A full
Store actor queue therefore returned Busy and stopped the mission even though
the model response was available. One agent-owned `commit_round` now serves all
four paths, using the existing five-attempt/20-ms Busy-only policy.

The helper never surrounds inference, tool execution or dispatch admission.
Response bytes, lease and proposed calls stay identical. Store rechecks authority
on every admitted write and persists the checkpoint and tool slots atomically.
Unknown acknowledgments and permanent errors are not replayed. Each caller keeps
its terminal outcomes, private checkpoint format, notifications and call order.

## Reproduction and verification scope

The old mission write was extracted into the shared helper without changing its
one-attempt behavior. Two new tests then failed at the actual full-queue Store
boundary because the received round was abandoned immediately. The first test
compilation had a fixture-only field assumption, corrected before these two
runtime failures; it is not counted as a product regression.

The tests hold the existing synthetic Store actor fixture and enqueue reads by
polling once, so the pressure is deterministic. After the fix, the unchanged
scenarios must preserve the exact checkpoint and one pending tool, reopen the DB,
execute that tool once and complete with one necessary follow-up model response.
When cancellation is queued ahead of retry, no tool slot is saved and restart
cannot revive the task. The first admitted response is supplied at the checkpoint
boundary; the follow-up uses the actual Runtime and loopback HTTP model adapter.
This does not simulate an entire provider stream under sustained DB load.

Native results, frozen review and exact-public-source Linux acceptance are
recorded below when complete. No live model, real provider/device operation or
production deployment is included. CI remains off.

## Native acceptance and independent review

All 32 focused agent contracts passed with the credential-broker feature enabled:
29 execution/planner/mission/guidance/pressure contracts plus three other Claude
completion, truncation and revocation contracts. Both new pressure cases passed
unchanged after the retry fix. Scoped all-target agent Clippy and the full synthetic
evaluator passed (15 worlds, 62 controls). Evaluated source SHA-256:
`f0407f0eae5f85a8ce108c15ad9dbbe5d3762d32ef7ec97514777dc5319c35aa`.

Independent read-only review found no material issue. All five source/test hashes
match the frozen reviewed version. The review confirmed Store transactions,
completed-step digest guards, lease/acceptance checks and the Busy/Unavailable
distinction. SQLite-level lock/COMMIT Busy and real lost acknowledgments are not
newly fault-injected; existing Store semantics and retry-policy contracts remain
the evidence for those boundaries.
