# Bounded compound exact calculation

Checkout `/private/tmp/moosuri-compound-calculation-20261115`, branch
`feat/compound-calculation-skills`, base `f94db70`. No SQLite schema migration,
Cargo dependency/lock change, automatic CI, publication or legacy operation.

The new `decimal_workflow` performs2–4 local sum/difference/product operations with
explicit input/prior-result references. Total operand references are limited to32;
all fresh inputs and operations must reach the final result. A coincidentally equal
number never establishes an edge. Fresh external decimals keep existing size,
six-fractional-place and i128 overflow checks. Typed intermediate coefficient/scale
values preserve generated precision and checked alignment avoids exponent overflow.
The provider tool definition uses the existing closed schema subset; core validation
enforces the actual bounds independently of provider hints.

Every operation has a child receipt under its parent slot and costs one existing
tool unit. Replay reads the saved parent result. Private learned reuse reserves
four units of headroom and charges actual operations. Existing ownership,
revocation,6model/12tool/time fences remain. Foreground graph learning does not
expand mission/private-route execution: mission receipt extraction merely ignores
the new non-authorizing result variant.

Canonical typed graph JSON is stored in the existing private route field, without
operands or trace text. At most64 routes/tombstones are retained per account.
Two independent finalized runs must have independently recomputable graph results
and adopt the final exact number. Input-expression hashes normalize decimal notation
and commutative arguments; separate primitive calculations are never inferred into
an implicit graph. Existing proof withdrawal/disable/delete behavior revokes actual
derived use. Current graph metadata supplies structure only and reuse accepts fresh
input slots only.

Existing cards and explicit sharing are reused. Export reconstruction reads only
the supported graph, generates repository-owned synthetic inputs and exact expected
results, and uses ordinary package/license checks. No training operands, messages,
identities, run links or successful-use proofs are exported. Imported packages
inherit no native learned proof or extra authority. Native toolset generation2
invalidates prior generation1 native learned proofs under the established schema
drift contract; two fresh proofs are needed. General external procedure learning,
semantic task success, arbitrary code and complete P4 remain out of scope.

## Actual selected native evidence

- Store false-edge/malformed-reference/canonical-replay/budget/replay gate:
  `cargo test --locked -p moosuri-store --features test-fixtures
  compound_graph_rejects_false_edges_replay_proof_and_budget_bypass -- --nocapture`,
  **1 passed /0.77 s**, `/private/tmp/compound-store-third.log`.
  It also checks twelve child receipts and exactly twelve tool charges after three
  four-operation executions, with same-slot replay charging nothing extra.
- Intermediate precision and input/reference bounds:
  `cargo test --locked -p moosuri-store --features test-fixtures
  intermediate_precision_is_exact_and_external_limits_and_reference_bounds_remain
  -- --nocapture`, **1 passed**, `/private/tmp/compound-precision.log`.
  Includes a12-place intermediate and a48-place generated value plus zero, while
  rejecting seven-place external input and self/forward/unused references.
- Actual supervisor/HTTP fixture A/B→unseen C, structural sharing and proof withdrawal:
  `cargo test --locked -p moosuri-agent --features test-fixtures
  runtime_learns_explicit_compound_graph_from_two_adopted_inputs_reuses_fresh_c_and_shares_no_trace
  -- --nocapture`, **1 passed /0.82 s**, `/private/tmp/compound-runtime-second.log`.
  Two line-item products→sum→factor yield249.21,28.8, then fresh C23.1; C uses one
  learned tool round and one final model round. A/B operands are absent from C
  requests and share files. Deleting B demotes to candidate and removes C output.
- Existing primitive exact-arithmetic compatibility: compiled Store test executable
  `agent_execution::arithmetic::tests::exact_decimals_reject_lossy_or_unbounded_inputs
  --exact --nocapture`, **1 passed**, `/private/tmp/compound-primitive-compatibility.log`.
- Existing two-proof primitive learning/fresh reuse/revocation compatibility:
  same compiled Store executable with `TMPDIR=/private/tmp` and exact test
  `agent_execution::tests::skills::learned_private_routes_require_independent_adopted_final_receipts_and_bind_fresh_c`,
  **1 passed /1.52 s**, `/private/tmp/compound-private-compatibility-final.log`.
- Edited Rust rustfmt and `git diff --check` passed.

These are synthetic native macOS arm64 fixture checks with no real model/auth
calls. Independent review, Clippy and root's joined Linux checks remain pending.
They are not a live-model quality evaluation, release-platform acceptance or a
claim that arbitrary multi-step user work has been learned.

## Root integration acceptance

The root reviewer independently inspected the fixed graph/evaluation/proof/receipt,
withdrawal and reconstruction-only sharing paths and verified the21-file freeze.
No material defect remained in that bounded scope. Integration preserves schema49
forward inference budgets and the accepted live-journal receiver.

Linux x86_64 passed compound Store1, intermediate precision1, actual Runtime1 and
8 existing learned lifecycle cases. The remaining schema37 fixture initially failed
because it left the later schema49 allowance tables present while pretending to be
schema37. Only that test setup was repaired; the same real upgrade/thread-deletion
case then passed. Existing knowledge-profile cases, strict Clippy and embedded-web
build passed. Passed unrelated suites were not repeated, and no web behavior or
public DTO changed. These checks do not establish live-model learning quality.
