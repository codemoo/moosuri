# Design

A cheap Korean/English cue on the current authenticated user message chooses whether
an isolated intent step is useful. It is a latency hint, not authorization. The model
request contains system instructions and only that current user text: no retrieved
source, earlier assistant output or previous provider continuation. The completed
closed function decision is no-command, fixed clarification, or a typed memory plan.
Exact literal quotes establish origin, not mathematical proof of semantic intent.
Classifier quality must be evaluated separately from synthetic transport correctness.

Normal chat after no-command uses its own fresh request and continuation. The intent
step remains part of the cumulative run budget and usage ledger; no planning text or
metadata is streamed into the conversation. Clear undated remember/correct/forget plans
are supported first. Effective-time ambiguity asks for clarification; the manual
memory editor retains explicit temporal support.

Each run has one stable sealed slot. Literal target queries resolve to exactly one
current active account-scoped memory and pin its revision. Zero/multiple/truncated
matches cause zero mutation. The typed plan cannot supply tenant/authority/object IDs.
Retry reuses the sealed plan, not another model-generated interpretation. Applied
receipts are queried under current tenant authority independently of active run guards.

Local writes are terminal. The actor removes obsolete assistant text, event replay,
private capsules and context dependencies, then publishes only a fixed truthful
confirmation and optional new-memory reference. No old provider continuation is
exempted from knowledge invalidation. Runtime mutations and HTTP mutations use the
same short delivery gate; no model/network wait holds that lock.

Forget adds an opaque journal action targeting a prepared command ID. Final guard,
append/fsync and application execute without actor interleaving. Fsync adopts the
operation; replay completes it even after later session expiry. Pre-append preparation
is not an applied effect and never autoexecutes on startup. Journal-resolution metadata
survives deleted conversations, but replay never recreates their text. Post-append
failure poisons the Store until recovery. Ordinary assistant memory is not the user-key vault.
