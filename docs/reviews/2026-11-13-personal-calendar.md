# Personal Google Calendar — 2026-11-13

The authenticated owner can connect a separate Google Calendar grant, read the
verified primary calendar, and create, update or delete a solo timed event from
the form. Google app setup is shared with Gmail, while consent, credentials and
operation receipts remain separate. Calendar is not yet an agent or mission tool.

Only a current owner-primary observation admits a mutation. Existing events also
need an exact provider ETag, self organizer, no attendees and no recurrence or
special event type. The connector requests no email notifications, disables event
reminders on create/update, and refuses writes when calendar email notifications
are configured. This does not control unknown third-party or device behavior.

Every admitted UUID has an immutable payload and a committed dispatch before one
provider request. Retries return the saved receipt; ambiguous outcomes only offer
lookup. A missing event after DELETE proves current absence, not causal deletion.

## Independent findings and fixes

- A claimed OAuth exchange could finish after disconnect and reactivate the
  connection. The durable disconnect now retires pending and claimed flows for
  the exact owner. Interrupted journal replay, another owner's flow, and a fresh
  explicit reconnect are covered. The original independent reproducer passes.
- Edits made while an update was pending survived, but reused the superseded
  ETag on their next save. A matching confirmed receipt advances the editing
  metadata while preserving the newer draft. Direct and lookup receipts are
  covered; the same event ID on another connection cannot rebase the editor.

Both findings passed independent re-review. Root integration preserved mission25,
vault24 and Push23 migration, recovery and session boundaries in schema26.

## Verification

- Linux x86_64: nine focused phases passed134 tests; two explicit opt-in checks
  were ignored. They cover Calendar Store15, connector5, Gmail19, Google setup8,
  restore9, migration8, destructive journal7, server44 and CLI19.
- After local-time formatting and edit focus improvements, the final web was
  embedded and the Linux CLI19 checks passed again. Unchanged backend suites
  were not repeated for these display changes.
- React256 passed as an integrated suite. The final Calendar12 and Artifacts6
  checks passed after the narrow fixes. Type checking, production build, generated
  DTO equality, formatting, all-target clippy, plan links and strict OpenSpec passed.
- The first broad native Store run found an old-schema fixture that retained
  Calendar tables while declaring a predecessor schema. Its teardown was fixed
  and the real encrypted predecessor restore passed. An existing UI test also
  clicked Save before navigation had enabled it; it now waits for that state.

Chrome at390×844 exercised the actual Rust login, direct Calendar route, session
after restart and unconfigured Google setup guidance. Connected UI checks used
synthetic Calendar API responses: edit and delete immediately updated the list;
an uncertain create was confirmed with one lookup and no additional write; the
chat draft survived Calendar navigation. Local dates, edit focus,16px inputs,
independent panel scroll and no horizontal overflow were observed. The final
loaded page had no console errors. Rust connector tests separately exercise
actual loopback HTTP.

No live Google consent, event or email action, real phone test, production
deployment or complete F37 claim is made.
