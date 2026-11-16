# Household reminder replies

## Why
A frozen actual-model household scenario asked for a reminder time and then
answered its missing-content question with “우산 챙기기”. The narrow verb list
failed to save it and the ordinary model falsely said reminders were unavailable.
A natural cancellation worked only by falling through a model call.

## What Changes
- Keep immediate owned draft semantics, accept short Korean activity labels by
  structural nominal ending rather than a fixed activity vocabulary.
- Recognize complete spaced/polite cancellation replies and bounded dismissal
  prefixes for the current unsaved draft.
- Extract literal reply parsing from time/draft lifecycle code.
- Test actual saved state, original time, replay/reopen and preservation of existing reminders.

## Impact
Reminder local admission and tests only; no schema, model setting, external action
or user permission change. Missing/general prose still uses explicit content labels.
