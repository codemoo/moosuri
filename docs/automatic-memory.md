# Memories from opted-in source collections

In `/app/knowledge`, select a connected collection and open **자료에서 기억 만들기**.
Automatic extraction starts disabled. Choose either the author label already
reported by each source, or explicitly assign one person label to this collection.
Neither choice verifies identity or assumes the author is you. Missing or composite
source labels are skipped; a specified label cannot replace a different existing
source label. Quoted, forwarded, ambiguous and instruction-only paragraphs are
excluded by a separate model decision, whose semantic accuracy remains fallible.

Turning the setting on applies to newly committed source revisions. It does not
process old history. Open a particular source and press **이 자료에서 기억 찾기**
to request that exact current revision. An enabled setting persists logout;
password/recovery, collection disconnection and provider changes fence old work.
Re-enabling creates a new policy epoch instead of reviving old jobs. Native tools
and conversational memory commands do not gain any new authority.

The worker reads complete literal paragraphs with their UTF-8 byte spans, preserving
LF and CRLF separators without newline normalization. Each
paragraph is at most4KiB; one call receives at most8KiB and16 complete paragraphs,
selecting at most4. Oversized or uncovered portions are skipped, never shortened.
The model only returns paragraph indexes: tenant, source revision, speaker, subject,
fact text, dates and evidence are constructed and checked by Store. Subject and
speaker retain the reported label exactly. Supported explicit source validity is
copied without interpreting dates in prose. This first path creates independent
source-backed assertions; it never silently merges or corrects another memory.

Saved memories appear in `/app/memory` with actual source evidence and extraction
origin. Source changes invalidate dependent memories; later manual corrections
retain the original memory ID and take over its later revisions. Repeated source
quote/author identities are skipped rather than appended again, even after a new
source revision or setting epoch. An invalidated duplicate is not silently revived.
The UI reports a completed call with zero new facts distinctly from saved facts;
partial input does not imply the entire document was inspected. Reopen the control
to refresh a pending result.

## Authority, cost and interruption

An explicit authenticated setting creates a Store-issued background grant tied to
account epoch, collection identity, policy revision and frozen verified model
binding. Normal ingestion revisions do not change that policy epoch. Background
work does not fabricate a session or act under an administrator's user context.
It rechecks current authority/source/provider before dispatch and commit. Changes
to unrelated account knowledge also conservatively fence an acquired job to keep
its selected-index namespace stable.

Defaults are at most10 dispatches,24,000 serialized inference-body **bytes**, and
4,000 reserved output tokens per account per UTC day. API settings may lower these
limits. The body count includes instructions, data/schema and public provider
routing fields, excluding HTTP/TLS framing. Each dispatch reserves1024 output
tokens and sets the transport's corresponding output cap. Actual observed token
usage is separate; absent usage never refunds the reservation. These are not input
token or dollar guarantees. A120-second job, one call per job,32 outstanding jobs
per account and one worker at a time bound this first implementation. Queue overflow
records a skip; newer revisions supersede queued work for the same source.

A consumed/unknown remote dispatch is never automatically repeated. Startup marks
acquired or dispatched jobs outcome-unknown; final receipts are idempotent. Current
source revisions and suppression are checked transactionally with all selected
memory writes. A malformed selection or failed atomic write cannot report partial
success. Daily budget reservations survive restart and unknown acknowledgements.
No remote wait holds the Store actor or HTTP/SSE delivery gate.

Jobs persist public model pins, opaque identities, fixed outcomes and accounting;
source prompts, selections and private continuations are not stored in the queue.
Source withdrawal cancels pending leases and prevents retry retransmission. Already
admitted requests retain the existing local cancellation boundary; a provider may
already have received their data, and late results cannot commit.

## Forgetting and evidence

Forgetting an automatically extracted memory stores its source/quote suppression
before deleting memory text/history. The same original paragraph is filtered before
future prompts and checked again at commit, including new job IDs and setting
epochs. Forgetting also cancels pending work for that source. Suppression is exact:
changed whitespace, a paraphrase, another source identity or a smaller manually
selected evidence span is not a universal semantic denylist. Original source text
remains within the existing forget scope. A later explicit manual assertion remains
allowed. Source/collection deletion keeps the independent destructive-journal
replay and derived-answer revocation contracts.

Synthetic Store, HTTP, worker and React checks verify these boundaries. They do not
measure real-model extraction precision/recall or establish perfect attribution and
prompt-injection resistance. No live extraction evaluation is claimed. See the
[OpenSpec change](../openspec/changes/extract-source-memories/) for its exact scope.

[Integrated verification](reviews/2026-11-13-automatic-memory.md) records actual
platform checks, the request-ordering repair and remaining semantic-evaluation limits.
