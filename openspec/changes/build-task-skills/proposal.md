# Change: grounded delegated-task skills

## Why
The current agent can retrieve personal knowledge but has no reusable task procedures
or exact arithmetic. Users need grounded briefings, meeting preparation and comparisons
without learning tool names, while unavailable external actions must remain explicit.

## What changes
- Add three original bundled recipes with compact discovery metadata and bounded on-demand reading.
- Pin the actual tool/recipe profile when admitting a run; preserve legacy and retry behavior.
- Add checked decimal sum, difference and product through the existing durable tool ledger.
- Ground task outputs in scoped source/memory reads and preserve revocation and budgets.
- Record public personal-agent cases and separate runtime tests from live semantic evaluation.

## Impact
Rust core, Store and agent; bundled community recipes; no new external connector or
arbitrary code executor. Public model DTOs remain unchanged. General community install,
email/calendar writes, scheduling, browser/phone/payment and credential brokerage remain
follow-up work. No new approval screen or general-purpose intent-classifier call.
