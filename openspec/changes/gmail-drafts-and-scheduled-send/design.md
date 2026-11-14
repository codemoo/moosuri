# Boundaries

Gmail draft IO uses typed fixed users/me endpoints through the Gmail adapter.
Default OAuth remains readonly/send. An explicit compose OAuth start persists its
choice with the original one-use session, state, app and PKCE exchange. Absent or
partial compose consent fails; an unsolicited compose scope on default consent
or refresh does not upgrade draft authority. Current owner/session, app, connection
epoch and compose scope are checked before IO and before private output publication.
Draft payloads and tokens are never logged or exposed to model send tools.

Provider drafts are separate from local immutable approval snapshots. Create and
update copy the stored bytes of an owner-selected local revision. Read/import of
supported single-part plain text creates a new unapproved local snapshot; no
provider ID is ever the send payload or send authority. Unsupported HTML, multipart,
attachments, named addresses, aliases and reply metadata cannot be overwritten or
silently imported through a partial text projection. They remain readable with
limits and explicitly deletable. Lists are 20 per page. Provider read content can
be truncated and says so. All provider mutations have no network retry.

A provider digest detects changes since the user's last read. Gmail drafts do not
provide a conditional-write revision here: another editor can still race between
GET and PUT/DELETE. The UI states this limitation. A lost write ACK asks for a
fresh list/read; it does not claim a successful save or automatically repeat it.
Saving/deleting a Gmail draft does not approve, cancel or modify any local action.

Schema40 adds an optional mail_schedules sidecar, distinct from reminders. A future
UTC instant (maximum 366 days ahead) plus an IANA timezone is previewed from an
explicit local datetime; DST gaps reject and folds use the first offset. Approval
binds action revision/digest plus stored timing, and pins a separate background
auth epoch/revision grant. Timing mutation increments revision and resets approval.
A content/connection edit removes old timing and requires a new full preview.
Logout leaves this explicit grant active; account status/epoch/temporary validity,
connection epoch/status/send scope, source withdrawal and grant revision fence due
execution. Approval after the displayed due time is rejected. Due work expires
five minutes after its original instant; there is no recurring/coalesced mail policy.

The existing single-writer transaction selects only due approved mail, validates
current authority, freezes equality evidence, and commits dispatch before one exact
messages/send POST. Existing no-retry, startup unknown, bounded Sent reconciliation
and late receipt preservation apply unchanged. No network IO runs inside SQLite.
Approved scheduled mail can be edited/cancelled before commitment. A concurrent
commitment may win; cancellation cannot recall an already in-flight request.
Sealed restore clears scheduled background grants and cancels uncommitted actions.
