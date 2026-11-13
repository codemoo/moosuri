# Design

Parse only current authenticated user text at Store chat admission with a bounded
deterministic grammar. No classifier/provider call is added. Reuse schedule create,
edit and control bodies inside the chat IMMEDIATE transaction. The current session,
tenant/account, auth epoch and selected target revision are revalidated there.
Persist the fixed confirmation and structured receipt with the schedule effect and
complete the chat before it can be claimed. UUID/payload digests replay the same
receipt after lost ACK/restart. Changed timezones/targets conflict on same UUID.
No model tools or deferred write lease are introduced. A committed command cannot
be retroactively cancelled; SQLite actor ordering controls revocation races.

Browser pending sends retain the original IANA timezone and receipt-selected
ID/revision. New browser context never supplies clock, tenant, epoch or authority.
Without selection, use exact owned titles or a bounded unique generic candidate;
ambiguity clarifies. Translation/quote/negative ordinary text never mutates. Mission
requests get a fixed unsupported answer. Existing verified model-choice admission
is intentionally retained; language/clarification grammar remains bounded.

Weekly days are sorted unique ISO1..7. Reuse compiled IANA wall-clock conversion,
gap skip/first-fold handling and bounded coalescing with constant whole-week count.
Keep existing Schedules request ownership, refresh and draft/CAS guards intact.
Migration0022 is reserved; isolated schema18 uses19 pending root22 integration.
