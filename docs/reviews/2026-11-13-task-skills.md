# Task skills acceptance record

This development unit adds three original instruction recipes and bounded exact
decimal arithmetic to the existing durable knowledge agent. The public case
research uses Muse as the primary reference and distinguishes product documentation,
marketing, demonstrations, anecdotes and our derived use cases. It does not claim
that external services have been benchmarked or that moosuri can already perform
their transactions.

## Implemented and inspected

- Admission pins the private tool-profile version and each recipe's revision/digest.
  Legacy runs retain the original three knowledge tools. Unavailable pinned content
  is not replaced by a newer recipe during retry.
- `skill_read` provides the procedure on demand. `decimal_calculate` performs checked
  decimal-string sums, differences and two-factor products without floats, arbitrary
  expressions, currency conversion or an additional dependency.
- Both tools share the original execution authority, durable slots, cumulative
  6-model/12-tool/120-second limits and dependency deletion rules. A recipe read
  prevents retry from re-entering memory-command planning.
- Scripted end-to-end tests verify a grounded quotation comparison and exact totals,
  source deletion during the final stream, private payload purge and retry behavior.
  These tests validate implementation behavior, not a live model's understanding.

An independent reviewer inspected all 25 implementation files at the frozen manifest
and reported no material findings. The integration also updates the existing HTTP
fixture to expect the exact five admitted tool names instead of the previous three;
the actual HTTP journey passed after that correction.

## Verification

- Rust 1.89: **198 passing tests** on Linux x86_64 in a bounded, nonroot, offline Docker
  environment: Store77, agent41, connector59, HTTP20 and CLI1. One opt-in live-memory
  test is ignored by default and is not included in the passing count.
- macOS verification composes the author Store/agent checks, the integrated workspace
  run and the corrected HTTP regression. The workspace run initially stopped at the
  obsolete three-tool fixture; that fixture was corrected and its real HTTP journey
  rerun successfully. Rustfmt and workspace/all-target clippy with warnings denied pass.
- The Linux clippy attempt lacked the image's clippy component. No Linux lint success
  is claimed; the Linux runtime suite and macOS lint are separate evidence.
- React source, generated DTOs and dependency locks are unchanged. The previous
  70 React tests/typecheck/build remain applicable; no new UI behavior is claimed.
- Strict OpenSpec validation passes for16items. The planning verifier checks238 local links; the243-file public source
  allowlist and secret scan pass. No workflow is enabled.

The CLI fixture initially used generated assets with old directory timestamps,
preventing Cargo's embedding build script from running again. Refreshing those
generated asset timestamps resolved the apparatus issue; the source did not change.

## Live evaluation status

Five synthetic Korean tasks were prepared before execution: project briefing with a
hostile instruction in the source, meeting preparation with a superseded proposal,
two shipping-inclusive quotations, an unknown shipping fee and a request to send
mail while the capability is unavailable. None is a blind holdout or a full-assistant
benchmark.

The existing ChatGPT login was reused through official Codex app-server0.167.1 with
`gpt-6.1-terra` and actual effort `high`; no login token was copied. A private temporary
compatible bridge disables inherited tools/MCP and preserves dynamic call/result
identities. The compatible product profile's effort is null. Its bounded150ms
parallel-call grouping and process startup make timing and round counts adapter-specific.

The text capability proof passed. Two explicit attempts at the subsequent tool
proof failed: the dynamic-tool result was acknowledged with matching text, but the
model did not return the exact challenge. An isolated short-string control passed;
a long challenge returned an empty final answer and a UUID-shaped control failed.
This does not establish a length limit or a provider defect. The application correctly
refused to enable the capability; no database flag or relaxed verifier bypass was used.

At the first publication the five tasks were **not run**. The later follow-up below
preserves that initial proof failure; the earlier memory-workflow scores remain separate.

### Follow-up after publication

The public capability challenge now uses `proof-` followed by sixteen random hex
characters (64 fresh random bits). It is not an authentication token. Exact challenge,
function/arguments, model/connection/effort identity and continuation checks remain
unchanged. Independent review and the Store/HTTP proof regressions passed. The actual
two-round model proof then passed through the normal API; no capability flag was forced.
This observation does not establish the cause of the earlier failures.

The private bridge also fixed a prebuffered parallel-call queue bug and passed a
two-call offline regression. That development-only bridge is not product code.

| Repeated synthetic evaluation | User outcome | Observed tool behavior |
| --- | --- | --- |
| First five tasks | 4/5; unknown-cost comparison exhausted the cumulative budget without an answer | Successful complete comparison used exact arithmetic |
| After general budget guidance, same five tasks | 5/5; median completion 12.755 seconds, 14 model dispatches total | Both monetary tasks skipped the calculator despite correct totals: 0/2 tool-use checks |
| After mandatory monetary-calculation guidance, two focused tasks | 2/2; 13.232 and 16.281 seconds, 4 and 3 model dispatches | 2/2 actually used `decimal_calculate` for totals/recommendations |

Grading inspected final answers and actual persisted tool arguments, not model claims.
The briefing ignored a hostile source instruction; the meeting used the superseding
decision; the incomplete quotation left shipping/tax uncertain; unavailable Gmail was
not falsely reported as sent. The mail case proves capability honesty only, not sending.
These are small repeated development cases, not unseen holdout or overall quality.

## Limits and next work

Only saved text sources and memories are available in this published unit. HTTP web
research, a real isolated Chromium worker, personal Gmail connection and final-approved
email dispatch are separate implementation units under active development. The user
requires explicit approval of the final recipients/content/attachments before every
email send. No live email was sent in development.

No public service, collector, cron, operational database or deployed account was
changed. The remaining semantic memory-target resolution defect, automatic extraction,
calendar/scheduler/push, media, vault and community installation are not completed by
these recipes. See [current status](../status.md) and [external work](../external-work.md).
