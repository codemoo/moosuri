# Account-backed ChatGPT evaluation — 2026-11-13

The owner explicitly requested use of an already signed-in ChatGPT account for
quality testing. Evaluation uses the official local Codex app-server with
`gpt-6.1-terra` and `high`, synthetic text and a dedicated local SQLite installation.
No ChatGPT credential is extracted into the repository, copied to Linux or returned
by the application. This is a development evaluation adapter, not a shipped
subscription-login provider or a production deployment.

## What was measured

The first completed classification run used Codex CLI0.167.1 and the unchanged
41-case public development corpus. **36 cases reached the real model and matched
both raw and production-parser decision/action/clarification labels. Five ordinary
inputs bypassed the model and matched the cue-routing expectation.** There were no
label mismatches in that completed run. It is not 41 live-model successes and it
does not measure the correctness of every extracted fact span, exactly-one-call
transport termination or overall assistant quality.

Observed per-model-case wall time was median5,548ms and maximum7,717ms; the whole
loop took202,972ms. Those values include local app-server startup and interruption.
They are not product time-to-first-token, an API latency benchmark or a measured
improvement. Usage can be absent when the evaluator interrupts after receiving a
classification call. No spending estimate is derived from incomplete usage.

This corpus is public and reused during development, not a blind holdout. The
initial run reports one inherited instruction source. It disabled selected native
features but had not yet explicitly excluded inherited MCP servers; no unexpected
native-tool execution was observed. The reviewed reusable evaluator now discovers
only server-enable metadata, restarts with each inherited server disabled, checks
effective feature flags and verifies an empty MCP surface before model dispatch.
Metadata-only verification observed7 configured servers,0 enabled and0 tools.
The initial score is preserved with its actual configuration, not relabeled as a
run of the revised evaluator or later classifier instructions.

## Intermediate classification run

After the reviewed changes, the same frozen corpus completed with **35/36 real
model label matches (97.2%) and5/5 cue-routing matches**. All41 cases were evaluated;
none was blocked or counted as a success without evaluation. The failing case was
`en_correct`: `Correct the remembered tea preference to I prefer coffee.` returned
`clarify/ambiguous` instead of the expected correction command. No mutation was
attempted by this classification-only evaluation. The opt-in test correctly exits
failed when a label differs; the failure is retained rather than retried away.

The revised evaluator verified0 enabled inherited MCP servers and0 MCP tools for
every model case, while still reporting1 inherited instruction source. Median
case time was4,999.5ms, maximum8,696ms and total195,618ms. With only one repeated
development run per configuration, changed scaffolding and no blind holdout, neither
the score difference nor the timing difference establishes improvement or regression.

That intermediate classifier digest was
`e913da3022c8c00b3a6173fe6a64ca7fde9628acd501e285e33bf2250f2be4dd`.

## Latest classification run

The classifier instructions now distinguish recognizing a requested operation from
checking whether a matching database row exists. The unchanged public41-case corpus
then completed with **36/36 real-model label matches and5/5 cue-routing matches**.
Both raw labels and production-parser labels matched. No cases were unevaluated,
and there were no model-format, parser, transport or apparatus failures in this run.
The earlier35/36 result remains an intermediate result, not removed from the record.

Each model case verified0 enabled inherited MCP servers and0 MCP tools;1 inherited
instruction source was still reported. Median case time was4,834ms, maximum6,695ms
and total178,644ms. This is a repeated development run, not a blind evaluation or
evidence that extracted facts/subjects/targets are semantically correct. The remaining
target-resolution failure below demonstrates why label accuracy is insufficient.

Latest compiled SHA-256 provenance:

| Input | SHA-256 |
| --- | --- |
| Classifier | `47a52a58b6d4340d61a45a22b0ee033266e6458a6668243fe5838599020a9263` |
| Corpus | `0f5852ff7baccdd1208b9ef438a6e42aed826aa667bdb3e255501de75f5c86b6` |
| Rust evaluator | `3a4a53331cedddb4f80fb600f014e7bc79d302393c8c6e909ac58c726528838a` |

## Failures discovered before acceptance

The initial live adapter could not initialize its parser under high effort. This
was an evaluation-apparatus defect, not a model semantic failure. It now adapts the
captured dynamic call to the existing Responses decoder with the exact capability,
and regression checks prove that valid calls reach the production parser while an
invented literal fact is rejected. Reports are checkpointed before dispatch and
after each case.

The temporary end-to-end bridge also discarded asynchronous notifications while
waiting for RPC responses and ignored streamed message deltas. Both were corrected
with a synthetic lifecycle replay, preserving actual call/result IDs and text.
Its first implementation split pending parallel tool calls into separate model
rounds, exhausting the product's existing budget. The evaluation bridge now groups
observed pending calls within150ms. This is disclosed adapter framing, not proof
that native provider round accounting or streaming latency is equivalent.

The tool-capability probe sometimes completed without returning its actual result.
The application's exact-output verification instruction now occupies the system
role, followed by a neutral verification request. The challenge is still unknown
until the real tool result arrives; the exact call, empty arguments, response model,
completion, EOF and text checks are unchanged. Direct controls and a real HTTP
round trip succeeded after the change; earlier failures remain part of the record.
No capability flag was manually inserted to bypass the probe.

A real memory was then saved with a genuine reference, but a new conversation
asking for a favorite drink failed to retrieve the saved tea preference. Literal
queries such as a category name were absent from the stored fact and subject.
The bounded memory-browsing change addresses that gap without introducing fuzzy
mutation targeting or claiming semantic-search quality. The narrow target planner
also receives the actual substring-resolution contract; the Store still rejects
missing, ambiguous or changed targets.

Historical probe failures are connection/setup evidence, not additional scored
semantic cases. The successful probe was used through the actual HTTP setup path.

## End-to-end behavior:7/10

The integrated Rust HTTP server, actual agent runtime and dedicated SQLite store
used a temporary loopback bridge to the same official ChatGPT session. Native model
effort was `high`; the compatible Moosuri profile's effort was null. The bridge
performed protocol adaptation, so these results do not certify a native subscription
connector or native provider round/stream behavior.

| Scenario | Observed result |
| --- | --- |
| Explicitly remember tea preference | Pass: one literal memory and actual reference |
| Recall favorite drink in a new conversation | Pass: tea and an actual memory reference |
| Do not remember a coffee preference | Pass: existing memory unchanged |
| Explain a quoted remember instruction | Pass: no quoted fact saved |
| Correct “홍차 취향” to green tea | **Fail:** no target resolved; no revision/change/reference |
| Forget a nonexistent spacecraft qualification | Pass: existing memory preserved, no false reference |
| Forget “녹차 취향” | **Fail:** intended corrected memory did not exist |
| Recall after that attempted forget | **Fail:** original tea memory still present/retrievable |
| Answer17×19 with only an integer | Pass:323 |
| Add7 to that result in the same conversation | Pass:330 |

Both the earlier browsing integration and the latest target-contract prompt completed
this10-case scenario with7 passes. In the latest run, correction was recognized but
the literal target resolver returned `NoTarget`. The following two failures are
dependent workflow failures: correction never committed and no successful forget
occurred. This is **not resurrection after successful deletion**. `completed` run
status includes truthful clarification and is not counted as task success by itself.

Latest scenario completion time was median5,845.5ms and maximum9,177ms, including
bridge overhead and0.5-second observation polling. These are not TTFT or production
streaming measurements. The pass checks are narrow assertions on synthetic state,
references and text; they do not assess all prose quality or arbitrary missions.

## Separate exact-target diagnostic:3/3

After preserving the failed scenario results, the same stored tea memory received:

1. `"나는 홍차를 좋아해"라는 기억을 "나는 녹차를 좋아해"로 수정해 줘.`
2. `"나는 녹차를 좋아해"라는 기억을 잊어 줘.`
3. A new-conversation question asking for the favorite drink.

All three passed. Correction retained the same memory ID, incremented its revision
and returned its actual reference. Forget removed the memory and revoked the
correction answer/reference. The final recall found no memory, returned no false
reference and did not assert the removed fact. Completion times were5,592ms,5,594ms
and11,193ms respectively. These diagnostic cases are not substitutes for the failed
phrasing and are not pooled into an improved success percentage.

Broad correction/forget target resolution remains an open product-quality defect.
Before enabling semantic targeting, test negation, meaningful qualifiers, similar
facts and ambiguous people/topics against independent unseen cases; matching a single
reduced token must not silently authorize changing the wrong memory. Automatic
extraction and general assistant acceptance remain outside this work unit.

## Subsequent offline target-resolution work

A later implementation adds an isolated candidate-index selection round only when
literal resolution finds no target. The Store seals the already accepted current
intent and complete bounded current candidate snapshot; neither candidate text nor
the selector can change action/fact or provide account/memory IDs. Generation,
full-snapshot and effective/head revision checks fence selection and commit.
Candidate memory/evidence dependencies purge private snapshots and checkpoints on
actual forget/source withdrawal; snapshot checks also run before resume and remote
dispatch admission. Literal ambiguity still clarifies without a selector. Limits and retry behavior are
in [conversational memory](../conversational-memory.md).

Synthetic Store and scripted-runtime checks exercise preference, residence, work
and dietary assertions, person/negation/qualifier ambiguity, index/parser authority,
candidate drift, effective-time changes and retry boundaries. These checks are
protocol/storage evidence, not a new semantic score. No account-backed model was
called for that implementation; the historical7/10 and exact-target3/3 results above
remain unchanged. Cross-topic selection accuracy and candidate prompt-injection
behavior require a separately authorized independent model evaluation.

## Reusable opt-in evaluation

Ordinary `cargo test` never starts an authenticated model. After explicit authorization
and a working official Codex ChatGPT login, use a new private absolute report path
outside the repository:

```sh
MOOSURI_LIVE_CHATGPT=1 \
MOOSURI_LIVE_MODEL=gpt-6.1-terra \
MOOSURI_LIVE_EFFORT=high \
MOOSURI_LIVE_REPORT=/absolute/private/new-memory-report.json \
cargo test --locked -p moosuri-agent live_chatgpt_memory_semantics -- --ignored --nocapture
```

The report contains source digests, fixed case identifiers, labels, timing,
configuration counts and available token counts; it omits raw prompts, facts,
credentials and arbitrary RPC errors. Source/corpus digests describe the compiled
inputs. Setup attempts, dispatched model calls, captured decisions and evaluated
cases are counted separately. Completed responses without a decision and oversized
arguments are model-format failures; failed/interrupted transport and decoder
apparatus failures are distinct. Blocked rows remain unevaluated. `report_finished`
means the loop finalized, while `complete` requires all41 cases evaluated, including
explicit cue routing. Three offline regressions cover these boundaries.

No quality claim here covers automatic extraction, schedules, external work, skills,
attachments, a user-key vault, real mobile devices or the full accepted roadmap.
The temporary Python transport is private test apparatus and is not shipped in the
Rust/React product. General native subscription setup remains separate work.

## References

- [Official Codex app-server protocol](https://developers.openai.com/codex/app-server/)
- [Official configuration reference](https://developers.openai.com/codex/config-reference/)
- [OpenAI: harness-aware evaluation](https://developers.openai.com/cookbook/examples/partners/harness_aware_plugin_evals/harness_aware_plugin_evals#overview)

These sources explain supported local account integration and why tool tests,
classification and a product harness must be reported as different evidence.


The later semantic-selector implementation and failed-before-case holdout setup are documented in [target-resolution acceptance](2026-11-13-memory-target-resolution.md). Its two fresh connection probes did not establish tool capability, so no new holdout score replaces the historical observations above.
