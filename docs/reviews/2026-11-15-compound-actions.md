# Compound actions — synthetic implementation evidence

Scope: a current one-off composed mission can prepare one exact existing local MailAction,
save its explicitly requested new Gmail provider draft through the existing journal, and
create or move one primary solo Calendar event through the existing ledger. No tool
approves or sends mail. Original cards read actual linked action state and receipts.

The local draft and its stable link commit atomically. Repeated proposals cannot change
the original payload. The provider journal commits before IO and records the exact save
intent; readback uses an operation-specific MIME hash and exact PUT target. The original
send approval bytes stay unchanged. Calendar uses current owner/connection/auth/app and
credential pins, the actual observed target/ETag, and its existing UUID/effect receipt.
A mission capability is not converted into a fabricated session. Later explicit final
mail approval and late Calendar settlement do not restart inference or reset budgets.
Connection withdrawal removes private derived projections while minimal receipts remain.

Passed locally with the configured native toolchain and offline dependencies:

- `cargo check --locked --offline -p moosuri-agent -p moosuri-store`.
- `cargo test --locked --offline -p moosuri-store missions::connected::tests -- --nocapture`:
  8/8. New cases cover durable draft preparation after logout, stable identity, later
  approval after mission expiry without new inference; no action rights from read-only
  tasks; duplicate Calendar titles and stale ETags; exact date-move duration; missing ACK,
  late settlement after expiry, and connection withdrawal of the actual private event.
- `cargo test --locked --offline -p moosuri-agent mail::drafts::tests -- --nocapture`:
  8/8. Includes earlier PUT A versus unknown PUT B/new POST identity, and mission-driven
  provider save lost ACK with one provider write, zero sends and unchanged approval.
- `cargo test --locked --offline -p moosuri-agent connected_missions -- --nocapture`:
  3/3. The new complete Runtime case reads actual supported full Gmail payloads and
  Calendar data, prepares a local draft, performs the requested Calendar creation, saves
  the Gmail provider draft, then returns both original-card links using six model calls.
  It records one provider draft POST, one Calendar POST and zero mail sends.
- The new Runtime vertical also passed five consecutive executions after fixing its
  fixture ambiguity. Initially the fixture left both a read-only and a compose Gmail
  connection active with the same email; UUID ordering sometimes selected the read-only
  account and correctly hit the compose guard. The final fixture admits one compose
  account. A prior legacy raw-message fixture response was replaced with the supported
  full MIME payload. Neither failure was treated as product success.
- `cargo clippy --locked --offline -p moosuri-agent -p moosuri-store --all-targets -- -D warnings`.
- `npm run typecheck`; targeted `Missions.test.tsx` and `GmailDrafts.test.tsx`: 20/20.

Integration remains root-owned. This isolated branch includes schema59/62/65; the root
must retain its other reserved migrations in order. B's materialized mail pins are used
for preparation. E owns the bounded learned-evidence and recurring draft-admission
integration hooks; those are not established by the standalone D tests above. Regenerate
and compare the complete merged API types when integrating all contracts. A fresh
independent review is required before publication. These checks do not establish real
Gmail header retention/raw-MIME normalization, real Google account acceptance, native
Calendar support, general Calendar edits/invitations, automatic sending, or final release
readiness.

## Independent-review repairs

The fresh review identified an unrelated imperative granting Calendar authority (P1)
and a prepared action disappearing from revised acceptance requirements (P2). Calendar
admission now recognizes only an imperative clause whose object is a Calendar event;
report/draft/document creation verbs cannot authorize it. Proposed create/date-move must
match the exact operation in both immutable original and current owner request. Mixed
mail-draft creation plus Calendar date-move retains the move operation.

Acceptance editing after any prepared effect now returns an explicit conflict before
changing the task/checks. Original action identity, recipients, content and receipt remain
attached to the original card. A defensive verifier also rejects historical acceptance
snapshots whose prepared action belongs to another revision. No new effect ID is issued.

The focused native Store connected-mission suite passed10/10 after these changes. New
actual Store outcomes cover Calendar-read/report and Calendar-read/draft requests with
zero Calendar admissions, a legitimate mixed task with one update/PATCH commitment and
zero event-create/POST commitments, and rejected recipient/body/check edits preserving
the original paused task and draft. `cargo clippy --locked --offline -p moosuri-store --all-targets -- -D warnings` also passed.


Final integrated re-review narrowed English event authority to an event/meeting/
appointment object followed by a time clause/end or explicit `named/called/titled`
name. Table/note requests and meeting-summary/event-report noun phrases have zero
Calendar admission. Existing report-only requests remain usable. Final integrated
Store15/15 and connected Runtime3/3 passed. P1/P2 review findings are closed; broader
free-form Calendar interpretation remains outside this bounded grammar.
