# Change: clear conversational memory instructions

## Why
Reading personal memories is insufficient for a usable assistant. A direct request to
remember, correct or forget a fact needs one durable local result, including when a
model response, browser acknowledgement or process is interrupted.

## What changes
- Use a bounded, isolated current-user intent step for likely memory instructions.
- Seal one typed plan per run before any retrieved tool result can enter that branch.
- Apply one local memory mutation and fixed receipt-derived confirmation atomically.
- Reuse sealed intents across retries and reconcile adopted forget operations by journal.
- Share a short runtime/HTTP/SSE delivery gate and hide all planner previews.
- Support bounded effective-memory browsing when broad recall lacks literal terms.
- Add an opt-in, credential-free-reporting live classification evaluator.

## Impact
Advances explicit P3 memory commands, not automatic extraction or full mission capability.
Rust core/store/runtime and SQLite migration6 change. Existing memory editing and
read-only retrieval remain. Clear commands execute directly; ambiguous targets request
clarification. No repeated approval stage, bulk deletion or external effect is introduced.
