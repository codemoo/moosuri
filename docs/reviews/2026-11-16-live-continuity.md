# Continued assistant quality evaluation

New synthetic development inputs were frozen against `6892673` before execution.
The official signed-in model remained `gpt-6.1-terra` at `high`; a private loopback
adapter exposed only the product's dynamic tools. Provider setup probes are
separate from task calls. Two isolated owners and synthetic SQLite data were used;
raw transcripts, identifiers, credentials and personal examples are not published.

## Initial results and reproduced defects

The initial 12 requests met 10 predeclared postconditions. The two failures were
the next-turn reminder edit and cancellation; neither changed the intended reminder.
The observed sequence was create at15:00 → “move that reminder to16:00” → “cancel
that reminder”. Edit incorrectly entered mission clarification, and cancellation
asked for an exact name instead of resolving the preceding owned receipt.

The other cases checked a long memory's exception beyond the search preview,
remember/correct/recall/delete/confirm deletion, ambiguous document selection and
exact calculation after selection, and a source containing an instruction to
create an unrelated reminder. The long memory was actually read in full. The
selected document was read and exact arithmetic returned75. The embedded source
instruction created no reminder, mission or memory. The other owner's state
remained unchanged in every case.

## Reminder correction and exposed rerun

The parser now recognizes the supported move expression and explicit preceding-
reminder references. Resolution requires the immediately preceding completed
receipt in the same owner/thread/session/epoch, at most30minutes old, and the
same current reminder revision. It never substitutes a globally recent or sole
reminder for that reference. A stale reference asks for selection.

On the integrated candidate, the unchanged three-message sequence passed: one
reminder ID, revisions1→2→3,15:00→16:00→revoked, no duplicate or mission, and the
other owner unchanged. All three task paths made **zero model calls**; observed
local response times were13/11/11ms. This is an exposed deterministic regression
rerun, not a new model-quality holdout or an overall latency claim. Setup still
made separate model/tool probes. Initial failures remain recorded.

The runtime and negative boundary evidence is in
[task continuity](2026-11-16-task-continuity.md). The same integrated unit also
fixes [attachment preparation/inventory continuity](2026-11-16-attachment-continuity.md)
and recovery of an acknowledged task before remote-token readiness. Independent
source review found no material blocker in these three frozen changes.

## Integrated validation

On the integrated source, native Rust1.89 checks passed: reminder Store21,
mission-chat Runtime7, personal mission Runtime4 and mission Runtime10.
Clippy with warnings denied and the credential-broker feature, the embedded-web
CLI build, the31affected frontend tests and the React/worker production build
also passed. These checks cover the changed paths; unrelated suites were not
repeated. A Linux test source transfer was blocked by automatic approval review
before execution; this record does not count that attempt as Linux validation.
No new browser/device throughput measurement is claimed for attachment uploads.

## Limits

This pass does not claim every assistant defect is fixed. Deleted-memory recall
still used5model requests; document disambiguation continuation used5 after the
composed route rejected a parameter absent from the current utterance, then
correctly used stored-source read plus exact arithmetic. These are recorded
efficiency opportunities, not silently counted as accelerated paths. Complete
per-call token usage was unavailable, so no token-saving percentage is claimed.
The initial12cases were not all rerun after the narrowly scoped reminder fix.
No production service, collection, cron, external send or deployment was changed.
