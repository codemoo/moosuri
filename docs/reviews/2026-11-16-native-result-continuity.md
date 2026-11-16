# Native mixed-result continuity — 2026-11-16

Baseline source: `dd443d03a93f09a8205b9e338f80e14713f2eace`.

A foreground round can mix native Gmail and saved-source tools. Native settlement
returns the original run with its saved continuation and typed local results.
The ordinary result path adds actual Search/Source receipt addresses under the
evidence-calculation profile, but the reclaimed native path serialized only the
typed value. That lost the address needed by `evidence_calculate`; source evidence
was present but could not be cited using a returned receipt.

One shared serializer now projects both paths. It uses the original completed
StepLease and call ID, retains the existing evidence-calculation gate and null
direct-receipt ordinal, and changes no caller error flag. Native grant/scope,
selector, budget, cancellation, withdrawal and dispatch admission are unchanged.

## Bounded synthetic evidence

A new real Runtime test uses a loopback model, temporary Store and authenticated
synthetic signed broker settlement. It exercises both call orders: local-first
results already saved before yielding, and native-first local reads deferred
until reclaim. It mixes search/native reads in one round and source/native reads
in the next, then consumes the actual returned addresses in source-bound
arithmetic. It checks exact total `10` from saved value `2.35`, current quantity
`4` and fee `0.60`; the original run and native observations survive. The final
continuation contains exactly one saved-source search and one source read, and
each variant uses four model dispatches with none while waiting.

With unchanged baseline production code, the test failed on the resumed search:
`receipt.call_id` was null instead of the actual `saved-search` ID. With the
shared serializer, both order variants passed. Raw synthetic diagnostic logs
remain outside Git.

Executed locally with Rust 1.89 and locked offline dependencies; all commands use
`-p moosuri-agent --features credential-broker-chat`:

| Filter | Result |
| --- | --- |
| `native_mixed_source_results_keep_receipts_for_bound_calculation_without_rereads` | baseline failed; fixed passed, both order variants |
| `native_mail_runtime_current_chat_and_background_mission_continue_from_signed_real_slots` | passed; existing signed chat/mission continuity |
| `runtime_evidence_calculation_learns_two_sources_reuses_fresh_c_and_withdraws_exported_structure_only` | passed; existing evidence arithmetic, learned reuse and withdrawal |
| `cargo fmt --all --check` and `git diff --check` | passed |

Existing ts-rs serde-attribute notices remain. Independent frozen review found
no material issue in saved-step/call identity, profile gating or Store trust.
Pinned OpenSpec and combined native/public-source acceptance are recorded in
the [continuation review](2026-11-16-continuation-integrity.md). These are scoped synthetic outcomes, not real account/device
acceptance or broad model-quality evidence. No live provider, production service,
CI configuration, model setting or credential was changed.
