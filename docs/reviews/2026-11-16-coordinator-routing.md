# Coordinator and tool handoffs — 2026-11-16

Baseline `9d72356ab0081c4440964e4d25284e76cfaeafbc`. This is a bounded defect
repair of the existing coordinator → isolated domain planner → typed tool/runtime
handoff, using synthetic data and existing public interfaces. Rust backend, React
web, SQLite, model defaults and disabled automatic CI remain unchanged.

## Shared planner mechanics

Artifact, memory and mission/Calendar planners each carried the same connection
readiness, request preflight, admission, streaming and local checkpoint skeleton.
`runner/planning.rs` now owns these mechanics. The closed `Admission` enum retains
`begin_agent_step` for artifact/memory and `begin_mission_chat_step` for the shared
mission/Calendar classifier. Domain parsers, current-message isolation and Store
write admission remain with their existing owners. Planning never receives saved
source/tool evidence or new mutation authority from a previous answer.

Dispatch and ambiguous acknowledgments are never retried. Only the identical
local checkpoint uses the existing bounded transient retry; artifact now shares
that existing behavior with the other planners. Each domain retains its previous
parse/commit ordering and sealed result format. Independent frozen source review
found no material regression. This consolidates duplicated planner mechanics without adding a framework,
dependency or model call.

## Personal mission transport reuse

The mission loop called `client` at every inference, rebuilding HTTP transport,
although foreground continuations already used `client_for_round`. A real
loopback official-protocol fixture tracked the peer of all three requests in a
source-read/calculation/completion mission. Before the change the new unchanged
outcome assertion failed: the three requests did not share their connection.
After switching the loop to `client_for_round(..., &client)`, the exact workflow
and socket assertion passed, including original proof, refresh, evidence, recurring
idle and owner-boundary checks in that fixture.

The reused client is renewed only after the same durable
`subscription_access(authority, binding)` and token-readiness/rotation checks.
Initial construction stays fresh for each mission. This prevents avoidable
connection setup; it is not a measured production latency or token-saving claim.
Independent review of the final one-line production change and peer assertions
was clean.

## Routing and lifecycle repairs

- [Native read routing](2026-11-16-native-tool-routing.md): name-based discovery
  now agrees with typed `NativeMailRead` capability. Existing signed current-chat
  and delegated mission flows verify initial/resumed schemas and receipts.
  Independent frozen review found no material issue; flag, scope, selected grant
  and background checks remain in force.
- Existing-document edits and explicit composer/name conflict handling are
  documented in [the routing repair](2026-11-16-existing-edit-routing.md). The
  baseline routed a named edit to read-only and allowed selected A to win over
  explicit B; corrected outcomes preserve identity or clarify without mutation.
- [Original-chat task eligibility](2026-11-16-task-handoff-events.md) now follows
  lifecycle writes. The durable event projection drops finished/deleted targets
  and updates wait/resume/capacity without repeated events on unchanged polls.
  All three new regressions failed on the unchanged baseline and passed after
  repair. Independent review of ownership, retention and lifecycle coverage was
  clean. No SSE-read mutation, schema change or extra model call was introduced.

The document review also found a repeated-quote edge case: selecting A while
naming a document literally titled “그 문서” twice could still edit A. The final
selection check treats any quoted occurrence as a literal name. The unchanged
runtime case failed before the follow-up and passed after it, preserving both
documents and returning no write receipt. Matching names, null and plain deictic
selections still work; separate frozen review found no remaining material issue
within this scope.

## Integrated native acceptance

The integrated source passed the following Rust1.89 locked/offline checks with
synthetic temporary SQLite stores and loopback protocol fixtures:

| Scope | Result |
| --- | --- |
| `domain-routing` | 4 passed |
| `artifact-targets` | 12 passed |
| `mission-classifier` | 8 passed |
| `memory-classifier` | 10 passed |
| `personal-missions` | 4 passed |
| `personal-round-authority` | 1 passed |
| `guidance` | 1 passed |
| `task-handoff` | 3 passed |
| `native-mail` | 2 passed |
| `evaluator` | 10 passed |

The standalone evaluator passed all15 flows and62
required independent validation controls. Its full-flow library test was skipped
in the native library invocation because the standalone driver ran the same path
once and retained its report. Scoped core/store/agent/evaluator all-target Clippy
with `--no-deps`, the native-mail feature, and CLI/evaluator builds also passed.
Independent frozen reviews covered shared planning, connection reuse, native
capability routing, document authority and lifecycle event propagation.

These checks do not establish live model understanding, real Google/device
acceptance or production deployment. The connection fixture proves reuse, not a
production latency or token-saving percentage.

The broader Store mission scan had123 passing and10 failing tests. All10 failures
also occurred on unchanged baseline production: seven old-schema downgrade/reopen
fixtures and three unsupported-effect admission fixtures. They remain documented
baseline failures; this repair does not claim a passing complete repository suite.

## Public-source Linux acceptance

Implementation commit `90b370036f2867b51e52acf21326f0872f961b79` is public and the canonical
checkout was fast-forwarded cleanly. The exact published source and scanned web
assets ran in the existing pinned Linux container with read-only source/root,
network disabled and no production mounts. All12 phases passed: domain routing,
artifact targets, mission and memory classifiers, personal missions, per-round
subscription authority, live instructions, original-chat task handoff, native
mail, evaluator library, scoped all-target Clippy and CLI/evaluator builds.
Source hashes stayed unchanged. The evaluator library includes the complete
15-flow runner and62 independent validation controls.

Source archive SHA-256: `ce24fe6490989458856cda13a07e1d2cc4453176e8261aec72664560678bf8e6`.
Format, planning metadata, strict OpenSpec and changed-source/new-commit secret
scans passed before publication. Automatic CI remains disabled. This acceptance
adds no production deployment or broad real-model quality claim; the separately
reproduced baseline fixture failures above remain open.
