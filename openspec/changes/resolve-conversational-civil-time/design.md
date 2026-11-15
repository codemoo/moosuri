# Design

`schedules::civil_time` owns deterministic normalization, exact clock prefixes,
local date expressions and local-to-UTC conversion. It takes an explicit request
reference and IANA zone, never host timezone or a model-computed instant. Current
and next ISO week differ from upcoming bare weekdays. One-shot conversion uses a
single local result, rejecting gaps and folds without changing recurring rules.

Reminder prefix scanning preserves raw UTF8 offsets so saved content stays exact.
Invalid numeric/unit tails cannot be consumed as content after a shorter clock.
Mission and Calendar acknowledgement check complete overlapping source spans;
quoted typed title/task spans are inert and unrelated nonoverlapping data does
not supply or revoke authority. Explicit recurrence is retained in quote scope
and checked independently against the proposed mission frequency.

Existing request plans/drafts preserve original supplied temporal components and
zone/reference through clarification and reopen. Missing-time answers retain the
existing answer-anchor rule. Routing remains a cheap candidate cue; the Store
still independently validates civil components and owner/effect boundaries.
