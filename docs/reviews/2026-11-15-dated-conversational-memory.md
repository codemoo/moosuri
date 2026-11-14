# Dated conversational memory: bounded implementation evidence

F19/P3 now supports one remember/correct assertion with a leading comma/newline-
separated absolute ISO or Korean date directive. Date-only timestamps are derived
server-side using the admitted message's original timezone, saved independently
of continuation/calendar profiles in existing run-choice JSON. Start is inclusive
local midnight; requested end includes that day through an exclusive next midnight.
No new dependency, migration, public classifier field or receipt field is added.

Store checks the complete current message independently of model fact quotes. A
versioned internal plan envelope seals the derived scope; retry/reopen validates
it against immutable text/timezone. Legacy undated plans still work. Corrupt seals
and legacy temporal plans fail closed. Recognized unsupported scopes and dated forget return
editor guidance before semantic target selection. Successful confirmation reads
actual saved revision bounds. Target CAS, effective/head selection, forget journal
replay, invalidation and run-wide budget paths retain their existing behavior.

Native Rust1.89 offline/locked checks on the isolated task worktree passed:

- Core temporal grammar:1 test.
- Store memory commands and semantic target/lifecycle regressions:37 tests.
- Agent memory-filtered parser, runtime, personal lifecycle and retry checks:
  25 passed,1 explicitly live-account test ignored.
- Core/Store/agent all-target Clippy with `-D warnings`.
- Workspace format check and `git diff --check`.

Focused outcomes include actual future/current/as-of and expired retrieval, dated
correction switching without reviving expired prior facts, model-stripped date
rejection, invalid/reversed/time-of-day/relative/missing-zone/skipped-midnight
clarification, dated-forget blocking, forged seal and legacy compatibility, and
reopen/retry retaining Seoul bounds despite a Chicago continuation profile.
The scripted runtime uses the existing Calendar/mission no-command classification
for civil dates, then isolated memory classification; there is no post-write model
continuation. No routing priority or model-call budget was changed.

Development failures were corrected before the final checks: initial test command
used two Cargo filters; initial Store compile referenced a private validator and
wrong search-hit field; an initial synthetic retry assertion incorrectly expected
zero total dispatches instead of preserving its one planning receipt; and the first
new runtime fixture omitted the existing civil-date domain-classification round.
These were fixture/compile corrections, not production provider calls.

These native checks use synthetic data and loopback scripted providers. They do
not establish live semantic quality or arbitrary temporal-language support. The
temporal recognizer remains bounded Korean/English syntax; recognized unsupported
scope expressions direct the user to the editor instead of inferring a date.

## R1: ordinary vocabulary and inclusive confirmation

Independent R0 review requested one correction: bare 기준/부터/까지/이전/이후/적용
and English through/until could reject an ordinary assertion and prevent legacy
prepared undated recovery. R1 requires explicit nearby date/relative-time context
or an explicit effective-boundary label. This remains a bounded recognizer, not a
universal semantic guarantee about every temporal expression. The full current
message is still checked for its recognized unsupported/conflicting forms.

Actual Store outcomes now save both `나는 원칙과 기준을 중요하게 생각해. 기억해 줘.`
and `Remember I commute through Seoul.` undated, with a supported leading date, and
through legacy prepared undated recovery. Existing malformed/relative/stripped-date,
missing-zone, skipped-midnight, forged-seal and dated-forget focused cases still pass.

Successful confirmation now derives the local start date and local date at saved
end minus one second from the actual saved revision. It displays the inclusive
application dates and original timezone, for example `2098-01-01부터 2098-01-02까지
적용할게요. (Asia/Seoul)`. Exact underlying timestamp assertions remain unchanged;
range, start-only and end-only confirmation outcomes are checked.

R1 matching-source native Rust1.89 offline/locked focused checks passed: core1,
Store dated6, agent intent8 and dated runtime1. Scoped core/Store/agent all-target
Clippy with `-D warnings`, workspace format and diff checks passed. Unaffected old
suites were not repeated. R0's frozen packet is retained separately; R1 is delivered
as a delta. The final independent re-review accepted R1 with no new material findings.

## Integrated Linux acceptance

The integration owner verified all ten R1 delta files against the frozen manifest
and reviewed the temporal-context change. Independent review accepted the original
timezone, sealed scope, legacy recovery and inclusive saved-date confirmation.

The exact integrated source passed the affected offline Rust1.89 Linux checks:
core temporal1, Store dated6, scripted dated Runtime1, core/Store/agent all-target
Clippy with `-D warnings`, and the default CLI with embedded React build. The
source archive was `97dc26f9d837b1a3100b7ea236203b6eeb24fc2876cc025ce098f30ff555db5e`;
its source hashes still matched after all five phases. Earlier unaffected memory
suites were retained without repetition. This changes no DB schema, dependency,
model/effort setting or public classifier field. Real-model temporal understanding
and broader date expressions remain outside this acceptance.
