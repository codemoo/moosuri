# Design

Compound report routing ignores well-formed quoted literal spans only when looking
for research/read/compute cues. Other routing semantics and isolated interpretation
remain. Apostrophes inside words do not open/close a literal; unmatched quotes keep
existing routing. This is a cost-routing heuristic, not mutation authorization.

The isolated artifact classifier keeps a literal deictic target_quote instead of
inventing an old title or identifier. The Store accepts only a bounded reference
for its requested document/goal/follow-up kind and one actual saved receipt from
the immediately preceding completed run in the same tenant/account/thread/session/
auth epoch, within30minutes, with an uninvalidated execution. The current artifact
must still be active at exactly the receipt revision. An intervening turn, another
conversation, withdrawn/stale result or ambiguous named-versus-recent record
cannot silently select another record. A record literally named 그 문서 is eligible
without a recent receipt only when the current request explicitly quotes that
unique name; an unquoted reference never falls back to a coincidental title.

Named targets use an exact-title query capped at two matches, so unrelated records
cannot exhaust a whole-library cap. Explicit composer targets retain existing
behavior. Only after current intent and target validation, an edit plan seals its
ID/revision into the existing profile and exposes the verified target for its
required fresh read. This avoids another target search without granting a write.
The actual mutation still requires the pinned revision and unique read anchor;
plan, selection and request receipt remain in the original transaction. No schema
or DTO changes are introduced. Create never infers an edit target.
