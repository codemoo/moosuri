# Change: bounded Calendar conversations

## Why
Connected Calendar reads and explicit solo-event actions should work from current chat without
an extra permission dialog, while preserving the form ledger and uncertainty contract.

## What changes
- Add schema30 chat-only frozen profiles, inherited withdrawal dependencies, opaque observed-event
  handles, one isolated current-user proposal and one sealed action UUID per request.
- Extend the existing mission intent round with a Calendar domain alternative. Literal reminders
  remain first, no-cue chat gains no classifier call, and a candidate has one combined round.
- Expose real five-event primary paging, fresh observed-event reads and receipt-only lookup.
- Resolve literal create/update/delete now with explicit date/time/end or duration, complete
  unique-target coverage, fresh ETags and the existing single-dispatch CalendarService.
- Render trusted receipt state independently of model completion; restored capabilities are stripped.

## Scope
Primary owned timed default attendee-free nonrecurring events only. No mission Calendar, delayed
execution, recurrence/all-day/secondary writes, invitations, mail sends or source-derived values.
No live provider/device/model-quality acceptance is implied by synthetic tests.
