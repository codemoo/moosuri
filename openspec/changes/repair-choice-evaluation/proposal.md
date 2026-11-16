## Why

The current synthetic pack covers valid free-context clarification and an early
in-flight cancellation separately. It does not prove that an invalid owner choice
preserves the original wait, a corrected real choice resumes completed work after
restart, and owner cancellation still defeats the resumed late final response.

## What Changes

- Add one reusable two-estimate household source-choice lifecycle to the Rust evaluator.
- Use actual HTTP input/control and Runtime reads/waits, with read-only persisted state grading.
- Reuse shared stage/call-budget and wait helpers; keep failed/not-run cases and usage missingness.
- Require candidate/authority/work/reply/cancellation/publication mutation controls.
- Observe actual worker retirement while Runtime stays alive before checking restart persistence.

## Impact

Evaluator source, scoped documentation and an authorized minimal read-only Runtime
activity observer change, including minimal wiring in three manual test constructors. No schema,
React UI, credentials, external effect, live model, service/cron or CI changes.
