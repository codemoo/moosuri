# A durable assistant that reads private knowledge

A model verified for function calls can search the current account's saved sources
and memories, read an exact revision, and continue its answer with those results.
Ordinary chat still works with text-only models. No search is required for ordinary
conversation. This unit supplies three built-in read-only functions; it does not
install arbitrary code, write memories or perform external actions.

## Enable and use

In `/admin/providers`, verify the model's text response, then select **도구 연결 확인**.
The tool check makes two synthetic model calls: the model must call a fixed empty
function and then repeat a fresh server challenge supplied only as its result.
Only a complete, correctly identified round trip records capability for the exact
provider revision, model and effort. It may incur usage. It sends no private sources.
An existing stored run keeps its original capability setting; verification never
upgrades old work silently. New messages use the current verified choice.

Save text in `/app/knowledge` or manage explicit memories in `/app/memory`, then ask
about it in chat. **확인한 자료** opens the exact source or memory revision returned
to the model. These buttons are server-derived input references, not a claim that
every sentence is proven by that reference. A model can still misunderstand evidence.
No live-model answer-quality benchmark has been completed.

## Execution and recovery

One Store actor owns admission, dispatch receipts, private continuations, pending
calls and local results. A run uses one opaque run lease and one new opaque step
lease per model dispatch. `begin_agent_step` is the dispatch linearization point:
a revocation committed first blocks dispatch; a dispatch admitted first is already
in flight, even if socket upload happens later. Already-sent provider data cannot
be recalled. A network-long lock is never held over inference.

The model transport must validate complete calls and clean bounded HTTP EOF. Only
then can the runtime commit the private continuation and typed pending slots. Local
reads execute from those committed slots, under current session/provider/run and
knowledge fences. Repeating a completed local slot returns its persisted result.
The next model request reads its continuation back from committed state. Stream
previews cannot authorize tools, and tools cannot accept caller-supplied results.

Every run, including all explicit retries, has at most **6 model dispatches,
12 local tool executions and 120 seconds from its first dispatch**. Clock rollback
also stops work. A limit produces `incomplete / budget_exhausted`; it is not an
authorization failure or a spending estimate. Each model step has its own receipt.
Observed token totals replace snapshots and sum across steps/attempts. A metric
stays unknown when any dispatch did not report it; admin totals are lower bounds.
Late valid usage can be saved after cancellation/deletion without restoring text.

A restart or ambiguous dispatch does not automatically replay remote work. Explicit
retry clears old continuation/input/result payloads while retaining dispatch history,
usage, the deadline, consumed budgets and dependency union. Successful completion
requires a validated completed step with no pending calls. Tool-free models use the
same ledger and existing text transport.

## Retrieval and deletion

`knowledge_search` uses literal words and at most five account-scoped hits. The
model must narrow truncated searches. `knowledge_read_source` takes an actual ID,
revision and UTF-8 byte offset; each excerpt is at most4096 bytes and returns exact
start/end/next offsets. `knowledge_read_memory` retains assertion/evidence/status
metadata. Results are capped at16 KiB, typed input slots at4 KiB. Malformed or
oversized semantic arguments become persisted error results, allowing repair within
the same run budget. No automatic memory saving is enabled here.

Only source/memory revisions actually delivered in a tool result enter the run's
dependency set. Successful prior turns selected as context propagate their dependency
union to later runs, including text-only successors. Source update/delete, memory
correction/forget or collection deletion purges affected derived assistant text,
private continuations and slots. User messages and content-free usage survive.
Destructive journal replay performs this invalidation before serving reads.
Any knowledge mutation conservatively fences an in-flight knowledge-dependent run;
completed unrelated answers remain. This conservative rule can interrupt a run even
when the changed item was unrelated; narrower in-flight tracking is future work.

An invalidated thread advances its reset floor and removes old text-bearing events.
The HTTP mutation and SSE delivery boundary share a short gate. The UI immediately
clears cached answer/evidence on reset. Every full snapshot also replaces old pages,
so reconnecting after a missed reset cannot retain an old deleted answer. Epoch
checks discard late history/action responses. Drafts remain editable and preserved.
Already delivered browser/network bytes cannot be remotely erased.

## Deliberate limits

This is lexical retrieval over manually saved text and explicit memories. It does
not establish semantic-search recall, prompt-injection immunity or mission success.
Retrieved content is instructed to be data, while the actual tool vocabulary and
Store enforce read-only account scope. Automatic extraction, conversational memory
writes, external tools, schedules, skills, attachments and the user-key vault remain
separate changes. Ordinary knowledge is not an encrypted user-key vault.

See [model protocol and official references](model-tools.md), [private knowledge](knowledge.md),
[chat](chat.md), [current status](status.md) and the
[acceptance record](reviews/2026-11-12-knowledge-agent.md).
