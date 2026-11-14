# Fixed-reminder missing-field followups · 2026-11-15

This P5/F29 increment supports bounded next-answer completion for explicit
fixed-text reminder creation. It adds schema51's typed chat-run draft sidecar,
uses existing atomic schedule/chat receipt creation and introduces no provider,
model classifier, API/React/type/dependency, CI or production change. Generic
Calendar/mission continuations and selected-target control keep separate contracts.

## Verification scope

Four new tests execute the real Store on fresh synthetic private databases rather
than merely mirroring parser outputs:

1. A missing-time Korean request completes after Store reopen; a five-minute-later
   `in 10 minutes` answer uses the answer clock and original source timezone even
   when the next browser context differs. Identical UUID retry/reopen returns the
   exact run/receipt once; changed content conflicts. A supplied relative time is
   preserved while waiting for an explicitly labelled literal body.
2. A time-only Korean request accepts bare `물 마시기`. Partial numeric AM/PM/day
   questions preserve the earlier message/date/clock. English content's case and
   punctuation remain exact. In particular the Korean 오후 marker does not itself
   count as a supplied day; the day answer remains required.
3. Cancellation, questions, translation/meta and new unrelated requests cannot
   silently become old content; following answers do not revive them. Outer
   quoted/source Korean suffixes do not create missing-time drafts. Other
   threads/logins/owners, expiry and restore quarantine cannot reuse a draft; a
   new independent command uses its own fields. Conversation deletion leaves no
   dangling sidecars. Ordinary unrelated chat is deliberately cancelled by the
   fixture before a later turn, respecting the existing one-active-run index.
4. Reopen from synthetic schema50 performs the actual schema51 forward migration.
   Foreign keys remain valid. Injected failures at question state insertion and
   final receipt insertion roll back the chat/schedule/draft consumption together;
   the same UUID can then complete once. Conversation deletion cascades the draft.

The existing current/inherited-image integration was extended with missing-time
creation and its bare time answer, asserting no pending draft/schedule creation.
The existing complete-command atomic receipt/reopen regression checks that the
shared finishing path retains its direct behavior. These checks use no live
provider or operating database. Scoped Store all-target Clippy and formatting passed on the R0 source;
strict OpenSpec validation also passed for the R0 change contract.
Independent source review and Linux integration are the integrating owner's
separate acceptance gates.

## Deliberate limits

Only the immediately next completed question under the same authenticated
session/epoch/thread is eligible, with15minute TTL and at most two answers. This
is a fixed reminder creation grant, not an action, mission or send delegation.
Missing time supplied later uses the answer reference; already supplied rules or
civil constraints and the original IANA zone remain fixed. Unsupported partial
clocks, DST gaps or absent civil timezone do not receive guessed times.

The bare content language is deliberately narrow. Other literal bodies need an
explicit `내용: …`/`message: …` label. This does not claim semantic detection of
all unrelated prose. Recognized unsupported input retires the prior draft and
may continue through ordinary chat. State is private per-chat data, not a new
public instruction or execution lease. Restore disables pending state; ordinary
reopen retains valid questions. No real-device notification or general language
coverage is certified by these synthetic Store checks.

Schema51 introduces a new sidecar;17 synthetic historical downgrade fixture files
now drop it before their existing `source_run_starts` drop and migration replay.
The production migration remains strict and does not use `IF NOT EXISTS`. These
unrelated suites have not been broadly run. Normal supported forward migration
and deletion/FK behavior passed at R0 as described above.

## R1 repair freeze

Independent R0 review identified cancellation precedence with a saved reminder,
Korean meta text accepted by a bare-body suffix, ISO-date Korean AM/PM insertion,
and a final-answer prompt that could ask for an unsupported third answer. R1
repairs these paths and adds one actual Store outcome case:
`reminder_followup_repair_cancel_meta_iso_and_exhaustion_do_not_expand_authority`.
It verifies saved reminder identity/revision/state survives draft cancellation,
meta prose cannot supply a bare body while explicit labels preserve literal text,
ISO date/body survive AM/PM completion, final-answer completion remains allowed,
and exhausted incomplete drafts close without revival.

At the R1 freeze, the new Rust checks were unrun; the integrating owner then ran
all five reminder outcome cases on Linux Rust1.89. They passed, and independent
R1 source review accepted the repairs. The representative schema47 fixture first
failed because its old setup retained schema49/50 tables. Only that synthetic
setup was corrected using the existing downgrade recipe; production migrations
were not relaxed. Its targeted retry passed. Integrated Store all-target Clippy
and the default embedded assistant build passed, with source hashes verified.

The integrated cancellation sentence now says that **no new reminder was saved**,
so it does not imply that other existing reminders disappeared. This wording-only
refinement is included in the final Linux build/Clippy; no redundant behavioral
suite was added for the copy change. Prior R0 media/shared-atomic evidence and R1
outcomes remain separately recorded; no broad historical suite was rerun.

See [behavior and grammar](../reminder-followups.md),
[existing schedules](../schedules.md) and
[OpenSpec](../../openspec/changes/complete-reminder-missing-fields/proposal.md).
