# Evidence-bound exact calculation · 2026-11-15

Worktree `/private/tmp/moosuri-evidence-calculation-20261115`, branch
`feat/evidence-calculation`, base `ff28aab`. This bounded F26/P4 increment adds a
common receipt-bound arithmetic primitive and structural mixed-source learning.
It uses existing saved-source tools, exact arithmetic, learned lifecycle and
sharing. No new runner, database migration, provider/URL adapter, public API/UI,
dependencies, CI, live model, private source or production changes are included.

## Implemented contract

Current query/prefix/scalars are model-facing exact **quotes**. The server
requires unique current-user matches, derives disjoint UTF8 ranges and rejects
absent/overlapping/ambiguous parameters. Actual complete current search/read
receipts must identify one unique source and revision. The exact fresh field
prefix must occur once in the full pinned source; the server derives its adjacent
strict decimal. It never selects the first of multiple sources or trusts a model
amount, guessed byte offset or semantic field extraction.

A plan stores source/current origin kinds and a1–4-step typed exact graph only.
Actual arithmetic child receipts and independently re-resolved source/current
bindings must match terminal final-answer adoption. Two distinct finalized
parameter/source subjects are required. Fresh C reuses the existing source
primitive with uniqueness enabled, records read children0/1 and arithmetic
children2 onward, and uses current quotes and current source evidence. Existing
owner/run/step/dependency/withdrawal fences apply. Absent or ambiguous evidence is
not a learned arithmetic defect; unsupported current input does not quarantine.

Generation3 and a false-default new profile flag prevent older runs from gaining
the new tool or qualifying as current examples. Existing schema-drift eligibility
index retirement/relearning is retained; original runs, observations and tool
receipts remain private. Structural sharing reads only the plan and generates a
new synthetic source precondition, anchor and numeric inputs. Missing fixtures
are not replaced with example numbers; installation creates no source or IO.

## R0 focused native outcomes

All commands use `--locked` and synthetic private temporary Store databases; the
Runtime provider is a local scripted HTTP fixture.

- `cargo test -p moosuri-store --features test-fixtures --lib
  evidence_calculation_ -- --nocapture`: **2 passed**,2.30s, log
  `/private/tmp/moosuri-evidence-calculation-store-r1-20261115.log`.
  Real outcomes reject multiple source matches, repeated fields, incomplete
  bounded source reads, unsupported source decimals, forged/current-overlap/
  ambiguous quote origins, foreign-run/owner receipts and changed revisions.
  A real single-operation calculation and slot replay retain exactly six charged
  tools and one arithmetic child after the deliberate failed attempts.
  Old capability and generation-drift outcomes reject the new primitive and require
  two fresh adopted decimal examples to restore eligibility. The existing
  standalone parcel route was not rerun; generation2 proof invalidation applies
  to it under the same shared contract.
- `cargo test -p moosuri-agent --features test-fixtures --lib
  runtime_evidence_calculation_ -- --nocapture`: **1 passed**,1.04s, log
  `/private/tmp/moosuri-evidence-calculation-runtime-r2-20261115.log`.
  Actual supervisor A/B queries select distinct sources, bind different private
  prefixes and evaluate product→sum graphs; first becomes candidate and second
  usable. Fresh Korean/emoji C reads a new source and yields13.75 using one
  learned tool round and one final model round. Its model requests contain no
  A/B query, field prefix or source amount. Server-resolved query byte ranges
  select the exact current text after Unicode. UUID retry returns the same run.
  Three real missing-field attempts keep route readiness/revision unchanged.
  Four-file structural export passes the package checker and excludes private
  values/anchors/identity/run canaries. Deleting B demotes to candidate and
  removes C's derived answer.
- Existing source primitive compatibility:
  `learned_source_route_reads_fresh_ids_and_source_deletion_erases_proofs_and_derived_outputs`:
  **1 passed**,0.77s, log
  `/private/tmp/moosuri-evidence-calculation-source-compat-20261115.log`.
  This checks the shared source executor retained its existing behavior and
  withdrawal after being factored for unique mixed-source reuse.

The first Runtime attempt exposed a1050-byte tool description exceeding the
existing1024-byte protocol bound; only the description was shortened. The next
attempt exposed a current scalar followed by a sentence-final period being
mistaken for an attached numeric fragment; the current-scalar boundary now allows
one terminal period while rejecting decimal/version continuation. The same
Runtime outcome passed after these repairs. Both failed logs are retained.

R0 scoped `cargo clippy --locked -p moosuri-core -p moosuri-store -p moosuri-agent
--features test-fixtures --all-targets -- -D warnings` passed (22.44s), log
`/private/tmp/moosuri-evidence-calculation-clippy-r0-20261115.log`. Final formatting,
diff whitespace and strict OpenSpec checks passed. Independent review and root
Linux/integration acceptance remain separate gates. No full suite was run. Existing `ts-rs` serde-attribute
notices remain unrelated compile output.

## Limits

Only one complete saved source, one uniquely prefixed plain decimal and current
literal scalar quotes are supported. Repeated scalar strings in the current
message require unambiguous new input; there is no model-assigned numeric range.
The source must fit a complete4096-byte read. Multiple source joins, URL adapters,
JSON/CSS extraction, transformations, unit conversion, semantic source correctness,
multimodal inputs, arbitrary code and external writes are not supported.

The model round count above is a synthetic call-structure observation, not a
real-model latency/quality measurement or full F26/P4 completion. Common root
README/status/features/testing documents are owned by the integrating root.

See [behavior](../evidence-calculation.md) and
[OpenSpec](../../openspec/changes/evidence-bound-calculation/proposal.md).


Root integration preserved the separately published Google preview capability and
the learned-proof display-only exclusion. The first integrated Linux run passed
the new Store2 and Runtime1 cases. Of15 affected existing learned cases,13 passed
and two revealed stale test constants after generation3. The schema-drift fixture
now uses the prior actual generation, and the parcel pin assertion uses the current
toolset generation. Those two cases then passed. The compound graph1, learned
parcel Runtime2, combined subscription/shared-model transport2 and existing
ten-world evaluator outcome/mutant scope also passed. The combined transport
fixture now includes both the already present decimal_workflow and new
evidence_calculate definitions, with23 actual definitions. No production fallback
or generation retirement rule was weakened to satisfy those fixtures.

Independent R0 review found two substantive defects before publication: grouping/
fraction numeric fragments could pass the current-scalar boundary, and exported
example requests repeated their query/prefix while larger scalar lists could
substring-collide. R1 repair and final acceptance are recorded below.
## R1 independent-review repairs

Independent review found two P2 defects. Current scalar boundaries admitted a
numeric fragment next to grouping/fraction punctuation, and the exported normal
request repeated query/prefix in its setup while values1–32 could substring-match
one another. R1 uses explicit supported scalar separators, keeping ordinary
whitespace/quotes/brackets/semicolon and bounded comma/colon/terminal-period
handling; adjacent grouping/expression symbols, including Unicode operators,
cannot authorize a numeric substring. Current-input rejection still produces no
successful arithmetic/proof and does not increase learned failure/quarantine.

The synthetic fixture prerequisite now appears only in `SKILL.md`, outside the
normal current request. The request quotes each query/prefix once. Repository-owned
synthetic inputs100–131 have equal width and are evaluated by the actual exact
primitive, without reading any private anchor/constant or adding a generator
library. A new actual Store outcome learns a12-input sum structure from a real
bound calculation, exports it, separately prepares the described synthetic source,
and executes the exact exported normal request and graph through real current
search/read/binding. It covers11 current scalar quotes, not only string checks.

R1 selected commands (subsequently passed on the integrated Linux source):

- `cargo test --locked -p moosuri-store --features test-fixtures --lib
  evidence_calculation_declines_ambiguous_incomplete_stale_forged_and_noncurrent_origins
  -- --nocapture` adds the direct `1,234` fragment outcome.
- `cargo test --locked -p moosuri-store --features test-fixtures --lib
  evidence_calculation_export_example_executes_with_many_unique_current_scalars
  -- --nocapture` executes the exported12-input example.
- `cargo test --locked -p moosuri-agent --features test-fixtures --lib
  runtime_evidence_calculation_ -- --nocapture` retains A/B→C/withdrawal/export and
  now checks missing-field, grouped and `1/3` learned rejections without quarantine.

The R0 evidence above does not certify repaired bytes. Independent R1 source review
accepted both repairs without further material findings. Root verified all frozen
file hashes and both full/delta hashes, then integrated the eight-file R1 delta
while preserving Google/profile guards and the existing generation fixture fixes.

The final network-disabled Linux capsule passed all three selected repaired
outcomes: direct current-number boundary1, actual12-input exported example1 and
actual Runtime A/B→fresh C/failure-classification/export/withdrawal1. Core/Store/Agent
all-target Clippy with both credential and test-fixture features passed with
warnings denied; the default assistant build passed. The exact capsule source
hashes were rechecked. No native duplicate of these R1 checks was needed.
Publication includes common support/learning/feature/testing docs. This remains
synthetic execution evidence, not live-model quality or full F26 completion.
