# Design

Core owns Calendar operation/reason vocabulary, shared memory/artifact classifier
tags, typed memory action/clarification values, and artifact action/kind vocabulary.
Schemas and typed parsing/new Store admission consume those values. Small agent
helpers construct nullable enum schemas and check exact object keys; they never
infer, trim, default or rewrite proposal values. Original JSON is still decoded
with serde so duplicate fields remain invalid.

Current-model Calendar/artifact proposals require every advertised top-level key,
including explicit nulls. Memory retains its existing complete-key, literal-quote,
temporal and cross-field checks. Calendar's decision-aware Store checks remain the
source of authority and civil-value validation. New Calendar clarify requires a
finite non-null reason: time/date/duration/target/account/ambiguous/unsupported.
Only the existing supported missing-field subset creates a continuation; ambiguous
and unsupported never become task completion authority. Unsupported proposals
retain their nullable reason compatibility.

Compatibility is intentionally asymmetric: new model parse and Calendar record
admission use complete keys and finite reasons; persisted DTO deserialization and
Calendar replay validation retain their prior defaults/reason fallback. Old stored
plans are not rewritten or reinterpreted. Artifact/memory no-command and clarification
require null command fields; malformed decisions cannot acquire a plan.

Synthetic regressions cross the real parser-to-Store checkpoint/continuation boundary
and the actual loopback artifact runtime. No live model/provider calls or operational
data are needed. Calendar continues only the original partial action and supplied
literal duration, then uses counted typed IO for exactly one effect. Mail final
preview approval remains unchanged.
