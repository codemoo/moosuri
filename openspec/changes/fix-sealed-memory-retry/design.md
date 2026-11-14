## Execution order

Explicit retry clears provider checkpoints and tool slots while preserving a
separately sealed local memory intent. An awaiting unrelated classifier route can
reject a run with previous remote calls before the memory planner reaches that
intent. The runner now asks the Store for the prepared memory intent before model
client construction or classifier dispatch, regardless of checkpoint presence.
It applies the original intent with the existing delivery lock and commit checks.

The Store owns intent identity, current lease rebinding, duplicate prevention and
target revisions. No intent means ordinary existing control flow continues;
opaque checkpoint bytes do not grant mutation authority. Pending memory-target
selection keeps its existing distinct planning path and is not broadened here.

## Validation

The unchanged sealed-command retry outcome test failed on the baseline. A first
conditional repair also failed: retry had already cleared the checkpoint it gated
on. Its new checkpoint-only negative case tested a different classifier guard and
was removed. Both failures remain retained. Validate the corrected repair with the
unchanged outcome test, full affected agent suite, Store memory-command lifecycle,
unknown shutdown/cancellation behavior, and independent review.
