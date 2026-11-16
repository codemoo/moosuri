# Answer settlement stability — 2026-11-16

Baseline: `c207c952be40c34e9ce27c4e33296e8a1a7862bb`.

Foreground completion acknowledged guidance, checked completion and demoted an
outdated answer in separate Store transactions. Reproduced faults were an applied
receipt surviving a failed completion write, and an ignored demotion failure that
still authorized the next round. The original sequence was extracted unchanged
into a wrapper; both new SQLite-failure scenarios failed before the fix.

`agent_execution/completion.rs` now owns the existing run/step settlement methods
and the new atomic foreground answer disposition. The same lease, no-tool/local-
memory checks, terminal validation and learned-skill finalization are preserved.
Foreground disposition additionally excludes missions so it cannot replace their
evidence verification. Instruction mutation is shared inside caller-owned guarded
transactions, and the old standalone supersede writer is removed.

The agent acknowledges tool-round guidance and settles final answers with the
existing bounded Busy-only retry policy. Permanent errors settle through existing
failure outcomes. An uncertain acknowledgment cannot replay the write or authorize
another inference. No model or tool dispatch is inside a retry closure.

## Reproduction and verification scope

Connection-local synthetic SQLite triggers fail the final state or output-phase
write after guidance acknowledgment. Rollback must preserve the entire visible
turn, including receipt state and version. Clearing the trigger permits the same
local disposition, then only the needed continuation. Reopen preserves one final
answer and original dispatch accounting. A separate case rejects unfinished tools
and cancellation. Existing stream/guidance and mission recovery tests cover Runtime
integration. The new injected failures occur at the actual Store boundary, not
during a full first provider stream or a physical power loss.

No live model or production deployment is included. CI remains off.

## Native acceptance and independent review

All 42 focused contracts passed with the credential-broker feature enabled:
12 Store contracts and 30 agent contracts covering the new rollback/authority
cases, guidance handoff, cancellation, partial responses, mission recovery,
planners, Claude settlement and local retry policy. Both original rollback cases
passed unchanged after the atomic fix. Agent/Store scoped all-target Clippy and
the full synthetic evaluator passed (15 worlds, 62 controls). Source SHA-256:
`341d2e823df9cf978d3f405947eaf731b39dfa5252796f35eab2016c1aed16a5`.

Independent frozen review found no material issue; all seven source/test hashes
match the reviewed version. Existing run/step failure semantics, explicit terminal
error forwarding, lease guards, atomic event writes, mission verification and
unknown-acknowledgment behavior were checked. Tests compare complete conversation
projections rather than directly counting event rows; event rollback additionally
follows from the reviewed shared transaction ownership.

## Public-source Linux acceptance — 2026-11-17

Implementation `403bcb3e4b6953ae846ac1796ce25353f81e09f2` was published and the canonical checkout
fast-forwarded cleanly. All six Linux phases passed: 12 Store contracts, 26 agent
contracts, four Claude contracts, scoped agent/Store all-target Clippy, product
CLI/evaluator builds and the full 15-world/62-control synthetic evaluator. The
evaluator source digest matches the native run; these repeated checks are not
independent model quality or latency samples.

The pinned Rust 1.89 container used read-only source/root, disabled network and no
production mounts. Only the synthetic Claude phase allowed its fixture launcher
to execute from temporary storage. Source hashes remained unchanged.
Source archive SHA-256:
`21c41a148d8b1f61b583ce4000d3e70affeae941a92fd00e102cc9610257b105`.
Format, plan/link, strict OpenSpec and source/new-commit secret checks passed.
