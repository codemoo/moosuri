# Official read protocols (implementation evidence, not provider certification)

`GmailSyncClient` and `SlackClient` normalize one bounded provider page at a time.
They do not obtain collection consent, store tokens/checkpoints, run a scheduler,
commit source data, or authorize mail sending. Their endpoint origins are fixed;
only test builds can substitute loopback HTTP fixtures. Production requests pin
validated public DNS addresses, disable proxies/redirects/automatic retries, bound
response bodies, and support cancellation including DNS/body reads. Error values
contain static codes and bounded Retry-After seconds, never provider error text.

Scope and checkpoint structs implement serde for **private** configuration and
checkpoint storage, but intentionally have no Debug. A checkpoint must be bound
to its tenant, connection and immutable config generation. Persist a successful
page's changes and checkpoint in one transaction. An error advances nothing.
`SyncPage::is_no_change()` explicitly represents an empty success. An empty or
partial page never implies deletion.

Each page has at most 32 normalized changes and 256 KiB of serialized changes;
source bodies are at most 64 KiB, titles/speakers 200 bytes. An overflow returns
`LimitExceeded`, with no usable next checkpoint, rather than silently truncating.
Attachment bytes/files are outside this text adapter. A message with no text is
represented with an explicit metadata note. Large or multiple-inline-body MIME
is explicitly refused when the maintained Gmail parser reports incomplete text.
These refusal cases require an explicit user-visible recovery decision; the
adapter does not bypass a blocked message or silently call a run complete.

## Gmail

`GmailScope` requires an expected mailbox email, 1–16 fixed label IDs (all must
match), and an explicit Unix-seconds cutoff. Fresh backfill is limited to 366 days.
`next_page(token, scope, checkpoint, now, cancellation)` verifies `/users/me/profile`
on every page, builds the Gmail search query itself, and fetches raw MIME from
at most 15 listed messages. It captures history ID before listing begins, so
post-backfill history catch-up closes the listing race. An initial backfill
finishes into `MorePages`, not a false complete status.

`GmailCheckpoint.history_id` is a high-water mark, independent of `page_token`.
Across history pagination the original start ID is retained until the final page;
only then does `GmailHistoryComplete` permit committing the returned history ID.
All history event categories are inspected, including label removals. Deletes
come from explicit deletion records or a fetched message's positively observed
scope departure, never list omissions. HTTP 404 on history yields
`ResyncRequired`; callers must explicitly reset and backfill without retiring
existing records by omission. A 400 on a continued backfill page also requires
restart. HTTP 429 and Gmail's documented HTTP 403 quota reasons are typed rate
limits; no read operation retries internally. This does not change the existing
interactive OAuth grant or exact approved-send contract.

Official references consulted:

- https://developers.google.com/workspace/gmail/api/guides/sync
- https://developers.google.com/workspace/gmail/api/reference/rest/v1/users.history/list
- https://developers.google.com/workspace/gmail/api/guides/handle-errors

## Slack

`SlackClient::identity` POSTs to `auth.test`, derives the immutable workspace/user
identity, and rejects bot tokens. The trusted caller uses that identity to create
`SlackScope`; client-supplied identity is never sufficient. `next_page` verifies
that binding again on every page. Personal user tokens must already be granted
appropriate channel history scopes by Slack; `auth.test` alone cannot establish
permission for a chosen channel. The first real history request checks that
permission. Public/private channel replies need an appropriately scoped user
token; this library neither manufactures consent nor implements Slack OAuth UI.

`SlackScope` fixes one channel and initial oldest timestamp. Fresh initial
backfill is at most 90 days; configured parent-thread reconciliation lookback is
1 second to 90 days. Pages request a conservative 15 messages. Every cycle first
polls new messages with one second of overlap, then **mandatorily re-enumerates
parents in the bounded reconciliation window** and paginates their replies.
Thus a parent absent from new history can still produce a newly arrived reply.
The persisted phase contains at most the 15 roots of the current history page,
a root index and separate reply/history continuation cursors. The timestamp
watermark advances only after incremental polling and reconciliation both finish.
Edits observed in those windows produce changed versions. Invalid/repeated
cursors and incomplete pagination cannot be mistaken for completed coverage.

`SlackPollingWindow` means only that these bounded polling windows completed.
It cannot establish deletions or edits/replies on parents older than the window,
including late replies on arbitrarily old threads. Deleted objects omitted from
history remain stored; no deletion is inferred from absence. Explicit deletion
records, if provided, normalize to tombstones, but this is not an authenticated
Events API intake. Reliable immediate deletion coverage would require a separate
verified Events API subscription and durable event/reconciliation strategy.
The scheduler must honor typed rate limits; commercially distributed non-Marketplace
apps may have 1 request/minute and 15-item limits, so large reconciliation windows
can take substantial time. Interrupted reconciliation resumes without falsely
advancing the completed watermark.

Official references consulted:

- https://docs.slack.dev/reference/methods/auth.test/
- https://docs.slack.dev/reference/methods/conversations.history/
- https://docs.slack.dev/reference/methods/conversations.replies/
- https://docs.slack.dev/messaging/retrieving-messages/
- https://docs.slack.dev/apis/web-api/rate-limits/
- https://docs.slack.dev/reference/events/message/message_deleted
- https://docs.slack.dev/reference/events/message/message_changed/

Verification uses synthetic loopback HTTP only, including identity mismatch,
expired cursors, opaque token/high-water separation, explicit deletion,
no-change pages, mandatory late-reply reconciliation, paginated replies, edits,
auth failure, Retry-After, quota errors, body limits, cancellation, redirects and
repeated-cursor fencing. No real provider-account or Linux evidence is implied.

## Runnable synthetic custom sender

`examples/custom_source_sender.rs` is a pure Rust, Unix-only protocol example.
It posts one built-in note explicitly labelled **synthetic**; it does not read a
real service, user messages, or a source database. After creating a custom HTTP
connection and saving its one-time ingest token privately, invoke:

```sh
mkdir -m 700 /tmp/moosuri-synthetic-sender
# Save the ingest token privately as token.txt in that directory; chmod 600 it.
cargo run -p moosuri-connectors --example custom_source_sender -- \
  https://your-moosuri-server.example \
  /tmp/moosuri-synthetic-sender/token.txt \
  /tmp/moosuri-synthetic-sender/state.json
```

The server argument is an HTTPS origin only, without path/query/userinfo; explicit
HTTP localhost/loopback origins are allowed for local synthetic tests. Redirects,
proxies and automatic HTTP retries are disabled. Requests and streamed response
bodies have time/size bounds. Authentication uses Bearer only, with no Cookie or
Origin header. Token input must be a same-owner, owner-only regular file without
symlinks/hardlinks, at most 128 bytes, containing the 64-character ingest token
(and optional surrounding whitespace). Neither token, note body nor checkpoint
is printed. Success prints a fixed status and the request UUID; errors print
static codes only.

The state directory must already be owner-only (mode 700), and state/lock files
are created owner-only (mode 600). A nonblocking process lock excludes another
instance using the same state path. Atomic replacement and file/directory fsync
persist a run intent before `/runs`, then the exact batch envelope before
`/batches`. State includes the origin/connection/config binding, sequence,
checkpoints and immutable request UUID. On interruption, rerun **the exact same
command with the same state file**. The example checks the current connection
binding, looks up the saved request receipt, and sends the original saved bytes
only if no receipt exists and the original run/position still matches. It never
allocates a replacement batch UUID after an unknown acknowledgement. A committed
receipt can be recovered after the run has finished; finishing a run is retried
idempotently. The `completed` finish outcome is only the producer's report, not
server proof that any remote scope was completely collected.

Use this demonstration as the **only producer for its connection**. The server's
run-start endpoint has no client idempotency key: if its acknowledgement is lost,
a persisted run intent plus the same-connection active run permits recovery only
under that single-producer assumption. An active run found before a new local
intent is refused, and an uncertain start with no active run is left for manual
review. An unrelated sender using a different state path is outside the local
lock contract; do not run one on the same connection. An expired/stale run,
changed config/position, unavailable receipt or denied token stops with the state
retained. Inspect/reconcile through the connection UI rather than deleting state
to force a new request. A completed state is retained and rerunning it confirms
that same receipt; it does not create another synthetic note. Adapting this into
a real collector requires its own durable source cursor and scope verification.
