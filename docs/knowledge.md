# Private sources and explicit memories

The knowledge screens let each account manage text sources and editable facts.
They are a manual management foundation. Tool-verified models can search and read
them in chat; see [knowledge agent](knowledge-agent.md). Automatic extraction,
official/custom external collection and document/goal editing remain upcoming features.
[Data connections](data-connections.md) defines the next ingestion/provenance contract;
Kakao collection remains an independently operated private producer.

## Sources

Open `/app/knowledge`, name a collection and paste text. The browser assigns its
stable scope identity within your account; it is not a field you need to manage. There is no
arbitrary path/URL fetch, binary upload or connection to production collectors.

Repeating an active collection creation with the same scope and name returns the
same collection. A changed name under that scope conflicts, and disconnected/deleted
collections are not revived by a delayed creation retry.

Each source records an external ID/version, internal revision, title, speaker,
observed time and optional validity interval. The API accepts bounded incremental
batches. A request UUID and payload digest make lost-acknowledgement replay a no-op;
source changes, its receipt and the cursor commit together. Different content under
the same request ID or a stale cursor conflicts. Omitted records are not deleted;
complete-snapshot retirement is explicitly rejected until supported.

Source detail shows plain text. To make an evidence-backed memory, select the exact
excerpt. The server checks its UTF-8 byte span, quote, source ID and current revision.
It does not infer a speaker, a date or the truth of a paraphrase. Source updates
invalidate every memory revision depending on the changed source.

## Memories and corrections

Open `/app/memory` to create a direct assertion, correct a saved memory, inspect
history or forget it. Direct assertions are separate from evidence-backed memories.
A correction retains the memory ID and requires the expected revision. A conflicting
edit preserves your draft instead of silently overwriting someone else's newer edit.

Search selects the highest revision whose start time has begun, then applies that
revision's status and expiry. A future-effective correction leaves the previous fact
current until its start. After a later temporary correction expires, an older fact
does not automatically return. Historical revisions require an explicit history read.

## Deletion scope

| Action | Result |
| --- | --- |
| Forget memory | Removes its text/history/search entries; original sources remain |
| Delete source | Removes source text/history/index and source-dependent memory text |
| Disconnect and keep | Stops ingestion; retained private copies stay usable |
| Disconnect and delete | Removes that collection's source content and dependent memory text |
| Delete conversation | Does not remove separately saved sources or memories |

An independent later assertion survives source deletion. Purged historical revisions
are no longer readable; content-free temporal barriers prevent them from exposing an
older superseded fact as current. Opaque item identities and request receipts reject
stale item replay. Evidence suppression hashes are retained for the upcoming automatic
extraction path; manual saving does not consume them as a semantic denylist. This does
not prevent a user from explicitly saving a new assertion.

Answers and private tool continuations that depend on withdrawn sources/memories are
also purged; user questions and content-free accounting remain. The same dependency
follows selected prior successful context into later answers.

Destructive actions append and fsync an opaque journal before applying SQLite changes.
A crash gap is replayed before private reads become ready. A damaged, truncated or
missing required journal fails closed. This mechanism is not yet a supported backup
restore workflow or forensic erasure of old WAL pages/filesystem snapshots.

## Search and limits

Search is lexical. Whitespace-insensitive Unicode lowercase normalization, FTS5
trigrams and a bounded substring fallback support Korean words and mixed Unicode.
The search engine runs locally without an embedding service. When used by the chat
agent, retrieved results are sent to the configured model for the answer. Tenant,
collection, speaker, subject and time filters constrain candidates before results.
Search pages are bound to the account, query and visibility generation; stale pages
must refresh after a mutation. Snippets are plain text, at most 512 characters.

- Up to 32 collections and 4,096 source/memory identities each per account; tombstoned
  identities still count, so repeated deletion cannot bypass those admission limits.
- Source text 64 KiB; up to 32 entries and 256 KiB text in one batch.
- Memory text 4 KiB, up to 8 evidence spans of 4 KiB each, serialized draft 16 KiB.
- Up to 128 revisions per item; list/history/search pages 20.
- One/two-character substring searches inspect at most 1,000 eligible items and expose
  `truncated` when the search space was bounded. Use a more specific query/filter.

These are implementation bounds, not memory quality scores. Local knowledge is
stored in the assistant database; it is not the separately designed user-key vault.
Administrator web APIs have no override to read another user's knowledge. An operating
system/database owner has a different trust boundary from that future encrypted vault.

## HTTP contract

All paths are relative to `/api/v1/knowledge` and require the current session plus
its expected-account header. Unknown input fields and invalid/stale revisions fail.

| Path | Method / payload |
| --- | --- |
| `/collections` | GET list; POST `{name,scope}` |
| `/collections/{id}/batches` | POST `IngestBatch` |
| `/collections/{id}/sources?after=…` | GET source page |
| `/collections/{id}/disconnect` | POST `{expected_revision,mode}` |
| `/sources/{id}?revision=…` | GET current/explicit historical text |
| `/sources/{id}` | DELETE `{expected_revision}` |
| `/memories?after=…` | GET memory management page |
| `/memories` | POST `{request_id,draft}` |
| `/memories/{id}?revision=…` | GET memory |
| `/memories/{id}` | PUT `{request_id,expected_revision,draft}`; DELETE `{expected_revision}` |
| `/memories/{id}/history?before=…` | GET revision history |
| `/search` | POST `KnowledgeQuery` |

The ingest HTTP envelope allows 2 MiB and memory envelopes 128 KiB to cover JSON escaping;
decoded Store limits remain stricter. See generated Rust/TypeScript DTOs for fields,
and [status](status.md) for the actual validation stage and remaining assistant work.
