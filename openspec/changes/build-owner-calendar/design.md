# Calendar authority, commitment and lookup

The existing operator Google web app and callback are reused, with independent exact Calendar
scopes, token table and encrypted owner AAD. A cal.-prefixed state routes only; hashed one-use
state, PKCE, original session/account/epoch and app pin are the authority. Neither shared model
settings nor Gmail tokens are Calendar grants. The callback captures encrypted code and redirects
an opaque handle for real-session same-origin completion. Google app replacement accounts for
Calendar pending/active bindings. Gmail consent and existing route behavior stay separate.

The connector extends the established fixed Google transport with Calendar's fixed origin,
If-Match and204 support; it retains DNS/public-address, proxy, redirect, no-retry and timeout
bounds. Typed APIs never accept a raw provider URL. Calendar metadata is bounded; event reads
are primary only with explicit max31-day range and partial-page disclosure. Per-read result
publication rechecks the owner/session/app/connection revision. No read omission causes deletion.

Direct authenticated form actions create an immutable UUID+digest request. The writer admits
session/account epoch, connection epoch/credential revision, app pin and fresh verified primary
owner metadata before recording dispatch. Update/delete additionally require the exact ETag and
a fresh owner-organizer/self, attendee-free, nonrecurring/default/timed event observation. Create
uses a deterministic provider ID; create/update stamp private operation/digest markers. Writes
use sendUpdates=none/sendNotifications=false and disable event reminders; primary calendars with
email notification settings are read-only. No model, source, general chat or recipe can grant writes.

Dispatch is the linearization point. Earlier withdrawal blocks dispatch. Later withdrawal does
not claim to retract a committed request; it fences private result publication while preserving
minimal observed outcome accounting. There is one connector write invocation, without retry or
post-commit refresh/resend. Exact duplicate request returns the stored record before network IO.
ETag412 is a definite conflict; transport/timeout/ambiguous5xx/parser interruption is unknown.

Restart turns dispatched operations unknown. Owner history reads also classify a dispatch without a callback after90seconds as unknown, beyond the bounded provider timeout; this permits lookup without restart or resend. Authenticated explicit lookup reads the same event
ID only. Exact operation/digest and requested current fields prove a create/update saved; changed
or missing evidence stays unknown. Delete404/410 proves current absence without asserting causal
success. Late exact receipts cannot overwrite terminal outcomes. Lookup may use current credential
revision under the same connection epoch while leaving the original request/digest unchanged.
Disconnect purges credentials and private action payload/result using the existing destructive
journal. Restore removes OAuth/credentials, fences old sessions and quarantines unfinished actions.

UI keeps chat drafts, shows actual connection/partial/failure/unknown states, and provides the
useful next action. It uses account request lifetimes and discards held responses after switching.
There is no second approval UI and no claim of general conversational Calendar completion.
