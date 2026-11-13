# Local reminder schedules and notification inbox

`/app/schedules` supports account-owned fixed-content local reminders. Create a
title/message and one future UTC instant, anchored UTC interval (at least60seconds),
or daily HH:MM in a chosen IANA timezone. The form can preview the interpreted next
time; save is the user action and does not require another approval screen. Edit,
pause/resume and delete use the current schedule revision. Up to64 nonrevoked
schedules are allowed per account; deleting releases a slot and preserves receipts.

This is the first F29/F30/F32 slice. A reminder delivers the saved text to the local
inbox; it does not execute the requested text as an agent task, send mail, run a
browser task or call a provider. Natural-language capture, weekly rules, actual
model tasks, changes-only result comparison (F31/AUTO03), topic mute and WebPush
remain next work. No browser notification permission is requested.

## Clock and occurrence policy

- Intervals keep the original UTC anchor+n×duration. Completing late does not move
  the anchor. Once inputs must be future UTC instants; late one-shots fire once
  only while their background authority and optional validity end remain valid.
- Daily rules retain their own IANA timezone, independent of later browser/default
  zone changes. A nonexistent local time produces a skipped local-slot receipt
  with no invented UTC execution. A repeated local time uses the first UTC offset
  once. Quiet hours use their separately chosen IANA timezone.
- A five-second supervised worker performs bounded DB polling. Idle model calls
  are zero. Each poll handles at most64 due schedules and32 local publications;
  persisted per-owner round-robin claim order prevents one owner taking all claims.
- Recurring downtime retains the latest valid missed occurrence. Older skipped
  local/UTC ranges and counts are summarized, rather than materializing years of
  backlog. The current gap has its own skip receipt; older coalesced ranges can
  include skipped gap dates. At most one claimed and one latest queued occurrence
  exist per schedule.
- Pause fences uncommitted queued/claimed occurrences. Resume and edit schedule
  only the next future slot, skipping paused history. A one-shot passed during
  pause ends as skipped/expired, never as delivered. Deletion revokes future work;
  already committed inbox entries remain. Old occurrence UTC identities remain
  unique across schedule revisions.

Supported calculation timestamps are1970 through2100-01-01 UTC. The maintained
`chrono0.4.45`/`chrono-tz0.10.4` libraries use a compiled IANA database; its explicit
version is persisted with each schedule. This is not a claim that the bundled
database is independently latest. A database-version change recalculates only
unexecuted future cursors; historical receipts and queued past instants stay exact.

## Authority, publication and restart

An explicit owner mutation persists a background grant pinned to tenant, account,
auth_epoch and schedule revision; it never fabricates a login session. Account
eligibility and current revision/state are checked before enqueue, claim and final
publication. Logout leaves this separate grant active. Password/recovery/account
epoch changes fence it and require an explicit current-owner resume/edit. Account
suspension is checked independently of the pinned auth epoch.

Claim alone creates no delivery authority. The same SQLite transaction rechecks
the grant, inserts the immutable notification, marks the occurrence delivered and
stores its notification receipt. Repeating a committed lease returns the same
receipt. Pause/edit first prevents publication; publication first remains delivered.
The local effect has no external send boundary. A dead process can requeue or
coalesce an uncommitted claim (30second lease), while committed notifications never
repeat after ordinary restart. A failure after inbox insertion but before receipt
commit rolls back both.

Restoration/cutover is separate from ordinary restart. Root's activation transaction
must invoke the quarantine hook to pause grants and skip uncommitted occurrences,
then require fresh owner action after restored-account enrollment. Delivered
receipts/inbox are preserved. This slice supplies the hook; it does not implement
or certify historical restore, independent latestness or external task recovery.

## Inbox and quiet hours

The local inbox is account-owned, with immutable title/message/occurrence identity,
actual publication time, delayed flag and quiet-hour state. Read and dismiss are
idempotent owner mutations; dismissal never resurrects through a concurrent read.
Quiet hours mark local entries without discarding them or claiming an external
Push was suppressed. Overnight windows are supported; enabled equal start/end is
rejected. Read/dismiss/quiet settings remain separate from schedule authority.

Lists are bounded:64 current schedules,100 inbox entries per keyset page, and50
occurrences/ranges each per history request. Old receipts and revoked tombstones
are retained; automatic history pruning/GC is not implemented. The React page uses
the existing account RequestScope plus synchronous request ownership/generations,
preserves unsaved edit/CAS state through refresh and fences old-account responses.
Navigating there preserves the existing chat draft in the parent workspace state.

## API

Authenticated current-account routes are `/api/v1/schedules` (GET/POST),
`/schedules/preview` (POST), `/schedules/{id}` (PUT),
`/schedules/{id}/control` (POST pause/resume/revoke), and
`/schedules/{id}/history` (GET). Inbox routes are `/api/v1/notifications` (GET with
optional `before` cursor/`dismissed=true`), `/notifications/{id}/read` and
`/notifications/{id}/dismiss` (POST), and `/notifications/settings` (GET/PUT).
No route accepts a caller-chosen account, clock, model or dispatch capability.

Schema0017 follows personal artifacts0016. Account suspension and restore use
their own subsequent migrations; neither is claimed complete by this unit. See
[synthetic implementation evidence](reviews/2026-11-13-local-reminders.md).
