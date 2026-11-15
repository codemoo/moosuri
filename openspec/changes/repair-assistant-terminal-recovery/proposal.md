# Repair assistant completion and native continuation

## Why

A provider could finish after only a progress message and the run was saved as a
completed answer. A completed native Gmail read could also exhaust tool slot 12
and be denied its otherwise permitted final model answer when resuming.

## What changes

- Classify completion from the final message metadata and actual answer body.
- Permit a completed native read to resume using remaining model capacity while
  preserving all original execution budgets and authority fences.
- Verify both failures through actual persisted runner/continuation paths.

## Impact

Agent terminal classification and store continuation admission. No new external
effects, automatic inference replay, model setting change or approval flow.
