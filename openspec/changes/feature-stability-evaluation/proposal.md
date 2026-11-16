# Stabilize existing feature flows

## Why

A single synthetic score hides which feature contracts were exercised. A recurring
task waiting for user input could also admit its next occurrence, violating the
existing one-unfinished-child contract and duplicating work or questions.

## What Changes

- Partition the existing 15 synthetic worlds into six selectable feature suites;
  retain applicable controls, failure denominators and explicit exclusions.
- Prevent a recurring schedule with an unfinished child from admitting another,
  while allowing unrelated schedules and retaining the latest queued occurrence.
- Track model, provider and device acceptance separately from contract checks.

## Impact

F03–08, F19–22, F24–32, F54. Evaluator CLI/report format 4 and a Store query fix.
No migration, new tools, permission prompts, CI activation or production deployment.
