# Design

Store admission pins WebToolProfile and increments existing profile version only
when web is available. A current in-memory installation profile fences dispatch
and completion. The external-tool seam persists typed input/nonce/result using the
existing12-tool budget; no SQLite transaction spans network IO. Retry of an
unfinished started slot records unknown instead of repeating HTTP automatically.

Observed references are projected from successful completed web slots, scoped to
the same tenant/run. Purge deletes the existing slots and therefore references.
No cache, body duplication table or model-authored citation parser is introduced.
The HTTP reader performs no subresource loads or JavaScript; the later browser
worker has a separate containment contract and is not integrated in this change.
