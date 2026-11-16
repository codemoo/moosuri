# Design

Schema74 stores an owner grant for one Gmail thread, immutable observed message
IDs, a mutable seen-ID cursor, auth/connection/app pins, expiry and a reclaimable
90-second nonce lease. The initial cutoff is the latest provider delivery time
among the actual read/sent baseline IDs. It is not the first poll time or array
order. A reply arriving after the observed message but before registration remains
eligible. A missing baseline, malformed/duplicate metadata or oversized coverage
cannot become an unchanged result.

The MailService supervisor polls due watch rows every five seconds; this is a
local queue check. Gmail is queried only when the individual watch is due (default
30 minutes). Header-only metadata includes IDs, labels and internalDate; no new
inbound ID means no body request and no model run. SENT/DRAFT, older messages,
unread or label-only changes do not notify. At most four new bodies are read and
projected into plaintext previews. Success and the existing local notification
commit together. A reply completes the watch, so no follow-up send is implied.

Credentials use a real watch lease and the existing per-connection refresh lock,
AAD and app pin. There is no invented login session, source-collector grant or
model-tool receipt. Authority is checked before each read, on commit, in the inbox
and at Push dispatch/display. Stop/disconnect/account changes, deleted original
conversation and restore withdraw the result. Ordinary logout leaves the explicit
durable watch active. Provider reads happen outside the SQLite writer.

The chat admission path handles bounded direct current-user phrases locally;
quotes, indirect statements and arbitrary model output cannot register a watch.
Ambiguous or absent evidence selects a target in the existing conversation UI.
Casual waiting text proposes a watch unless one is actually registered. HTTP create
requests have durable payload-bound receipts; retries coalesce the same active
scope without silently changing its cadence or original conversation.

The shared notification current-authority check now dispatches to the appropriate
mission-input, saved-source-monitor or mail-watch guard. A shared direct-event
writer preserves the existing mute/quiet/Push behavior and avoids another outbox.
The web displays escaped plaintext, bounded scrollable excerpts, state/retry/stop,
and an owner-validated conversation link. Excerpts are not copied into model
history, shared skills or automatic memory.
