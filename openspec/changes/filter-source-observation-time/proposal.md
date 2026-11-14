# Filter source searches by observation time

## Why
F22/P3 requires time-scoped source retrieval. The existing validity query cannot select when a source was recorded: sources with identical validity may have different observation times.

## What changes
Add optional inclusive `observed_from` and exclusive `observed_until` to source-only authenticated/tool/React search. Preserve omitted-field behavior, cursor identity and unfiltered learned proofs. Refuse memory/mixed semantics and unsupported fixed-date learning. No schema or dependency change.

## Impact
Core search DTO, Store predicates and learned eligibility, native read tool, knowledge UI and bounded synthetic acceptance checks. Existing owner/deletion/withdrawal and pagination fences remain.
