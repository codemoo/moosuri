# Personal ChatGPT read-only missions

This bounded P2/P5 extension lets an explicitly delegated existing one-off or
recurring read-only mission use its owner's saved exact personal ChatGPT model,
effort and binding revision. It adds no shared/family subscription pool, token
extraction, schema migration, dependency or public DTO change. The original owner
mission grant is background authority; text/tool proof is transport capability.

## Implementation boundary

Owner-aware personal binding/proof checks replace shared-only checks at mission
creation, control and recurring occurrence admission. Unsupported Claude background
remains denied. The mission driver obtains current token readiness through its
original RunLease and uses the existing official plan dialect, namespace parser,
durable begin-step allowance reservation, signed usage/checkpoint lifecycle and
stored typed receipts. It does not resume using provider continuation IDs.

New personal tool conversations carry a private, default-false command admission
marker. Only newly admitted exact-proof runs receive it; existing Calendar-only,
text-only, corrupted or retried old runs are not backfilled. Command interpretation
retains current request quotes, original session/account epoch, exact selected
scope/target and stored decision before creating a background grant. Provider/model
proof does not replace that explicit request. Calendar's existing current solo-event
write boundary is unchanged; mission execution exposes reads only.

## Actual focused evidence

- R0 Linux compiled and passed the connected Gmail/Calendar Runtime case and the
  cancel/revoke/restart/current-session Runtime case. The natural repeating case
  initially failed; that failed run is retained rather than counted as a pass.
- R1 native macOS arm64 passed the original `Every minute` direct/source/recurring
  creation, due execution and explicit natural-control Runtime case (1.38s).
- R1 `implicit_english_intervals_preserve_exact_unit_and_numeric_control` passed.
- R1 `cargo clippy --locked -p moosuri-agent -p moosuri-store --all-targets -- -D warnings`
  passed (21.97s). Direct formatting and `git diff --check` passed.

The repaired natural Runtime case and preexisting corrupted-admission guard passed
on Linux. R2 later passed both affected Calendar cases and strict scoped Clippy.
The other eight Calendar cases had passed R1 and were not repeated. Independent
R0/R1 review accepted the authority/parser boundaries; root independently accepted
the R2 verb+noun guard repair. No real account, credential or inference was used.

The first natural fixture exposed that existing civil validation required a numeric
duration and rejected the clear `Every minute` quote. R1 adds only exact normalized
`every minute/hour` interval quotes for60/3600 seconds; mismatched durations,
relative-delay reinterpretation and ambiguous phrases remain denied. Numeric two-
minute control remains covered. The fixture also now drives the due worker's actual
DB enqueue→mission admission order and reports stored failure receipts promptly.

The new fixture checks direct source evidence plus exact arithmetic, new natural
repeating delegation and explicit control, idle zero-model polling, durable logout
versus current-session authority, actual delegated Gmail/primary Calendar reads,
connection withdrawal, token readiness refresh, cancellation/revocation/restart
with no unknown reentry, and original usage receipts. Its capability metadata setup
uses the existing synthetic exact tool-proof fixture; it is not real-account proof.
Existing restore quarantine and forward-allowance paths are reused without change;
this fixture does not independently rerun full sealed restore or live historical
allowance acceptance.

Supported mission operations stay saved reads, exact calculation, admitted
instruction skills/configured public HTTP and explicitly delegated owner Gmail/
primary Calendar reads. Mail/Calendar writes, browser/vault/media, automatic memory,
Claude background and real-account acceptance remain outside this change. Explicit
owner delegation is sufficient; no repeated per-step permission UX is added.

[Use](../missions.md#개인-chatgpt로-맡기는-읽기-업무) ·
[OpenSpec](../../openspec/changes/personal-chatgpt-read-missions/proposal.md).

## R2: Calendar misrouting and existing regression alignment

Integrated R1 Linux passed the original natural repeating Runtime flow and the old
corrupted personal command-admission gate. Eight of ten existing Calendar cases
passed. A new admitted personal command can correctly finish with a fixed clarify
receipt when the proposed task target does not exist, so Completed alone is not
proof of unauthorized execution. Inspection of the paired create variant also
found that the existing mission write guard required the literal Calendar domain
even though its own verb+noun predicate already recognized create/update/delete
meeting/event requests. R2 removes that redundant domain precondition, preventing
an event-write request from becoming an unsupported queued read mission merely
because the original owner said meeting instead of Calendar.

The actual test now verifies clarify/unsupported receipts with no target and no
mission/recurring task or Calendar action/write, plus a forged intent quote absent
from the original request still fails without a receipt. Cancellation still proves
one write and purged private event; its classifier count now checks definition
membership instead of order. R2 touches only the existing conservative write guard,
that test fixture and this evidence record. The two repaired native regressions passed (4.52s and1.51s respectively); no other
previously passed Calendar/Runtime cases were repeated. Root owns final joined
Linux/build acceptance and independent review rechecks this production guard. The
native lane was released after these focused tests. Old Calendar-only/corrupted
admission remains unchanged.

## Integrated acceptance

The final public change also updates the React provider guidance and current support
contracts to expose personal read missions. The web typecheck/build passed. R2
Linux tests and Clippy used the frozen author source; the joined default Linux
binary passed separately with the current embedded web assets (27.108s). There is no new
schema, dependency or generated DTO. The prior loopback evidence does not establish
real ChatGPT eligibility, provider quality or live-account reliability.
