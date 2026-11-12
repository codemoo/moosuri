# Public HTTP research

The agent can fetch actual public HTTP(S) HTML/plain text using `web_read` and,
when the operator provides a Brave Search key, obtain actual search hits through
`web_search`. This is an HTTP reader, not browser automation: JavaScript, cookies,
personal browser sessions, forms, clicks, attachments and authenticated sites are
unsupported. Missing search configuration returns unavailable and does not invent
search results or silently select another provider.

The real `serve` command enables public HTTP reading. To enable search, add
`--web-search-key-file /absolute/private/key-file`; the file must be a small,
owner-only regular file with no symlink or additional hard links. Its contents are
never a CLI argument, DTO, model input or logged error. The only search destination
is the connector's fixed Brave endpoint. Operator configuration creates a client
before opening the store; its actual capabilities determine newly admitted run
profiles. Library `Store::open` and `Runtime::start` retain disabled defaults.

New web-enabled runs pin tool profile version 3, including existing exact recipe
pins and any Gmail account pins. Legacy profiles 1/2 keep their vocabulary. A
profile pin does not override the current installation: reopening without web or
search configuration prevents the corresponding old run from starting web IO.
No web tool can approve or send mail. Every mail send continues to require final
explicit human approval of its immutable preview, even if a user asks to send it.

External tools use persisted typed slots and a private per-slot dispatch nonce.
The SQLite actor validates current run/session/provider/budget/generation and
capabilities before admitting work, then releases the actor during network IO.
The runtime polls authority while IO is pending and validates again before storing
the result. Completed slots return their saved result. An unfinished previously
started slot returns `outcome_unknown`, without automatic duplicate HTTP. Explicit
run retries retain consumed budgets and deadline, as with other agent tools.

The existing run-wide ceiling remains six model dispatches, twelve tools and 120
seconds. The connector has a 20-second total request deadline, a 1 MiB response cap,
at most three redirects, 8 KiB extracted text and 12 KiB serialized result. URLs have
a 2048-byte cap; searches have a 512-byte query cap and at most five hits. Text
truncation means omitted content; there is no cursor, hidden refetch or claim to
complete page coverage. Nonidentity compressed responses are currently rejected.

All URL hops resolve and pin public destination addresses. Every resolved address
must pass the policy; localhost/private/link-local/reserved/metadata destinations,
including Azure 168.63.129.16 and mapped IPv6 equivalents, are denied. Requests use
HTTP(S) default ports only, no proxy inheritance, credentials, cookies, automatic
redirects or retries. Redirects are separately parsed and resolved. HTML extraction
does not execute scripts or load subresources; visible text marked only
`aria-hidden` is retained. These HTTP guarantees are not sufficient to sandbox a
future JavaScript browser, which needs separate Linux containment and egress tests.

Successful persisted web slots produce a separate optional `RunView.web_evidence`
list. It contains observed URL/title/retrieval time, page-versus-search-hit kind and
truncation, never raw bodies. React presents safe HTTP(S) links with opener/referrer
protection. The list covers the current run's observations; it does not assert
that every answer sentence or prior-turn context was verified. Model prose cannot
add entries. Existing source/memory evidence buttons retain their behavior.
Source withdrawal purges dependent slots and observed references; thread deletion
removes their owning run. Private results use the existing tenant-scoped storage.

Local validation uses synthetic data. Scripted model tests drive the real durable
runtime/store and a test-only web IO seam, including held IO with cancel/logout/
thread deletion and an unblocked actor. Connector tests independently exercise
actual loopback HTTP parsing, cancellation, limits and destination denial; they do
not introduce a production private-IP override. No live paid Brave request, Gmail
send, model quality claim or JavaScript-browser support is established by these
tests. Independent review and deployment validation remain separate release gates.
