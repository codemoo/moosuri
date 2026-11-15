# Conversational civil time · 2026-11-15

This P5/F29 increment extracts one deterministic date/clock oracle for reminders,
missions and Calendar. Request reference and IANA timezone remain supplied by the
existing owner-scoped admission/continuation profiles. No schema, dependency,
provider, operating database or ordinary-chat classification is added.

## Outcome evidence

Fresh synthetic outcomes cover actual reminder submission, schedule receipts,
Store reopen, exact request retry, original timezone retention and absence of
model admission. A fixed UTC calendar oracle crosses the December/January and
local-day boundaries: at 2026-12-31T15:30Z Seoul is Friday January1 while Los
Angeles is Thursday December31. Next ISO Friday09:15 in Seoul resolves to
2027-01-08T00:15Z. Gap/fold one-shot cases create no reminder.

Actual mission admission creates the requested next-ISO-week mission with its
saved once-rule and active receipt. The Runtime missing-clock continuation checks
that reopening and a later changed browser timezone retain the original week and
zone. Calendar civil-date/moment checks use the same external UTC expectations,
including native-hour/minute words, and reject mismatched proposed date/clock.

R0 integration reported passing the new Store reminder/mission/Calendar outcomes,
Runtime continuation outcome, existing Runtime mission outcomes and library
Clippy. Two old reminder regressions exposed ordinary weather capture and fixture
drift; R1 restores the parser decision and updates the owned pre53 reconstruction
fixtures without changing migrations. The integrating owner retains later schema
fixture drops while merging this earlier clone.

## Independent review repairs

Review found three authority defects: a model could drop a week qualifier from a
literal quote; a malformed Korean clock could fall back to its shorter prefix;
and weekly/one-shot mission frequency could be changed. R1 binds complete
overlapping current source spans, rejects malformed remaining numeric/unit/marker
tails, and requires literal weekly authority for weekly missions while rejecting
it for one-shot missions. Lowercase routing and bare-morning clarification are
included in the same repair.

The decisive R1 regressions exercise actual Store boundaries: a truncated mission
quote fails `commit_agent_step`, then the full original quote succeeds in that
same step; weekly/one-shot substitutions yield clarification and no new mission;
malformed reminder clocks create no schedule; and a truncated Calendar date fails
acknowledgement with no sealed plan/action before a valid whole quote succeeds.
The Calendar title includes unrelated quoted temporal words and start/end quotes
remain separate, checking that title data is not scheduling authority. Ordinary
weather remains `Decision::None`, while bare morning clarifies.

The integrated repair also preserves the full request's frozen timezone. An
unsupported timezone produces the existing clarification instead of a failed
commit; Calendar cannot recover rejected timezone authority from a narrower
model-selected date quote. Independent source review accepted that boundary.

## Integrated Linux acceptance

The integrating owner ran the actual Rust implementation on Linux with synthetic
accounts and no live provider calls. The five new Store civil-time outcomes,
one Runtime continuation, nineteen existing reminder outcomes, four Calendar
conversion cases and four existing mission Runtime outcomes passed. The mission
suite passed twenty-three cases before exposing the unsupported-zone regression;
its repaired case and the mission/Calendar full-quote regressions then passed.
The final Calendar case checks both the valid explicit IANA override and rejection
of a selected quote that omits an invalid full-request timezone. Its second
account fixture is independent of the first run's still-held lease.

Affected all-target Clippy and the assistant build passed after the final
production change. Old-schema fixtures retain the current schema55 drops; the
earlier DST-fold expectation now requires clarification rather than choosing an
occurrence silently. These fixture repairs do not change migrations. Formatting,
plan metadata, strict OpenSpec and source/history secret checks run at publication.
No live provider/model/account, device timezone, notification delivery or general
language accuracy acceptance is claimed.
