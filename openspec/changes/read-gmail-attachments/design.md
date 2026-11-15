# Design

## Official protocol

- [messages.get](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages/get)
  supports `format=full` and returns the parsed MIME payload.
- [MessagePart](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages#MessagePart)
  has immutable partId, MIME type, filename, body and nested parts.
- [MessagePartBody](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages.attachments#MessagePartBody)
  uses base64url data and decoded byte size; attachmentId identifies external data.
- [attachments.get](https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.messages.attachments/get)
  binds messageId and attachment ID and accepts gmail.readonly. No URL from an
  email, model or remote document selects the download origin.

## Projection and limits

Interactive message and thread reads use full MIME trees. The full-message HTTP
JSON remains capped at1MiB; at most64 unique parts, depth8,128 headers per part,
64KiB aggregate headers,256-byte filenames and128-byte MIME types are accepted.
One multipart/alternative representation is selected, preferring plain text.
Other body text is combined up to128KiB with explicit truncation. Selected external
text/plain or text/html body parts use the official attachment endpoint under a
shared128KiB decoded body budget and at most4 calls. Excess body parts remain partial;
source sync does not commit a truncated body as complete. No attachment is
downloaded while reading the body. Huge inline attachments can still exceed the
full JSON bound; the2MiB attachment capability applies to the separate endpoint,
not a promise that every inline2MiB payload fits the full-message cap.

Exact metadata includes message/thread plus part ID, optional attachment ID,
filename, MIME type, decoded size and readability. Model message results expose
up to4 attachments; mail_attachments continues bounded metadata pages. The user
message view shows the accepted metadata directly. Attachments.get is not paged;
bytes are fetched once per text read/page with a decoded2MiB cap and a dedicated
base64 JSON response cap. Each attachment read fetches one full-message metadata snapshot, validates the exact
message/part/attachment tuple, and then fetches only that part before exposing text.

mail_attachment accepts only exact part identifiers, byte start/max and an
optional expected page digest, never paths, programs or arbitrary URLs. PDF goes
through the existing deployment-owned executable's fixed internal-pdf-text child
with its2MiB/50page/24KiB text, CPU/memory/wall/output limits. A semaphore bounds
parallel extraction to2. UTF-8 text/plain is supported with24KiB whole-text bound;
other MIME types, encrypted/scanned/unsupported/over-limit PDFs and unavailable
extractors return explicit status rather than invented text. No OCR or binary
attachment download UI is added.

Text excerpts are at most4096 UTF-8 bytes and must progress at a character boundary.
The serialized external receipt, including escaping and metadata, is constrained
to12KiB below the existing16KiB Store cap. Model body reads also bound the whole
escaped mail_read/mail_thread result. Names mirror only exposed attachment
metadata; omitted metadata retains its exact next listing index (including0).
Display headers, recipient/label lists and body excerpts may shrink with explicit
partial/truncation flags; provider message/part/attachment IDs are never shortened.
Threads reserve the shared envelope across up to3 message projections. A page returns byte start, next start,
whole-text size, optional PDF pages, partial flag, binary SHA256, extracted-text
SHA256 and a combined digest binding message/part/attachment plus both digests.
Continuation requires the combined digest, so changed bytes or extractor output
requires restarting rather than splicing documents across pages.

## Authority and persistence

Foreground reads resolve the authenticated owner's current mail binding and
revalidate epoch after network and PDF IO. Connected durable missions use their
actual frozen owner read grant, never a fabricated user session; a message must
first have fresh successful search/read evidence in that occurrence. Existing
external-tool admission, call accounting, completion receipts and mail dependency
purge cover attachment text. Disconnect fences late results and revokes derived
output. User REST reads use the same service and revalidation. Browser scope
changes discard late attachment responses. No send or approve capability is added.

Gmail source sync shares full-message body projection but remains body-only under
its existing label/time/account consent. It never downloads file attachments, indexes
PDF text or unions interactive send scope. Large external PDFs no longer break
otherwise supported body indexing. Source history pagination, scope departures,
atomic page/checkpoint and revocation semantics stay unchanged. Interactive
attachment receipts are not new persisted source objects.
