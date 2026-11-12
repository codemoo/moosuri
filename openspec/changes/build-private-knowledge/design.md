## Context

Sources and memories live in the existing tenant-scoped SQLite writer. This is a
manual knowledge management slice; retrieval tools, automatic extraction, documents,
obligations and real collectors remain later work.

## Decisions

Collections have immutable scopes. Batches use a request UUID, payload digest and
cursor CAS; changes and cursor commit together. Duplicate accepted batches return
the original receipt, changed replays conflict, partial batches do not retire absent
records. Complete snapshots are explicitly unsupported in this slice.

Exact evidence binds a source ID, revision and UTF-8 byte span/quote. Empty evidence
means an explicit user assertion. No facts, speaker or timestamps are inferred.
Corrections preserve stable memory IDs and use revision CAS. A temporal search picks
the highest revision that has begun by the query time, then checks status and expiry;
it never revives a superseded fact after a later temporary correction expires.

Source changes invalidate dependent memory revisions. Delete removes source text,
search entries and dependent historical memory text. A later independent user assertion
survives; forgetting that memory leaves source originals unchanged. Keep-disconnect
stops ingestion while retaining existing private content; delete-disconnect removes it.
Opaque tombstones prevent stale item/request replay. Evidence suppression hashes are
retained for a later automatic extraction path; explicit manual saving is not treated
as that automatic path. No semantic paraphrase blocking is claimed.

Destructive operations append and fsync an opaque independent journal before SQLite
application. Startup validates journal integrity/watermarks and replays crash gaps
before readiness. This is not supported backup restoration or a promise to erase
old filesystem snapshots. No plaintext belongs in that journal.

Search limits candidate generation by tenant/speaker/time/collection before results.
FTS5 trigram and bounded whitespace-insensitive lowercase substring matching cover
lexical Korean lookup; short queries report truncated when their candidate cap is hit.
No embedding model, external search or global private ranking is used. Pagination is
bound to tenant, query and knowledge generation; stale pages conflict.

React keeps editor drafts and idempotency tokens within the account lifetime, shows
source evidence and destructive scope, and discards stale search/detail/editor replies.
Source paste and memory saving need no additional permission prompt. Chat does not yet
consume this knowledge; the management screen says so once.

## Limits

32 collections and4,096 source/memory identities each per tenant,128 revisions/item.
Sources64KiB, batches32 entries/256KiB, memory text4KiB and evidence draft16KiB.
Pages20; short-query candidate cap1,000. HTTP envelopes allow JSON escaping without
relaxing decoded storage limits. Default SQLite only; no arbitrary path or URL import.

## Validation

File-backed tenant/provenance/temporal/replay/deletion/journal failure checks, actual
HTTP ownership and mutation journeys, React conflict/lostACK/account fencing tests,
real Chromium flow and isolated Linux acceptance precede publication.
