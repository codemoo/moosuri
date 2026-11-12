# Design

The existing SSE parser optionally delegates validated events to a tool-round state
machine. HTTP uses one decoder abstraction; only tool rounds require clean EOF
after terminal. Previews contain no executable calls. Successful validation returns
private-constructed calls and a bounded protocol/model/effort-pinned continuation.

History preserves ordered reasoning, identities and phase needed by each dialect.
Results exactly match pending IDs. Later Store/registry work must validate original
intent, account scope, cumulative budgets and receipts. No checkpoint persistence,
automatic resume, knowledge tool or private reasoning browser export is added here.
