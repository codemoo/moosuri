# Source identity and ordering

A source is the immutable account-owned reminder_schedules ID, including recurring
mission schedules. Titles and message keywords never determine identity. Editing,
pausing or revoking a schedule does not change its source or mute preference.
Preferences have their own monotonic CAS revision, independent of execution grants.
Current-session ownership is checked inside the Store actor transaction.

Each actual notification publication snapshots an immutable source_suppressed flag.
Muted publications retain their original occurrence/mission receipt and history but
never enter inbox pages, even with dismissed=true or after unmute. They are not
marked read/dismissed and never become pending deliveries. Already visible entries
stay visible with their existing read/dismiss state. History distinguishes withheld
notifications from delivery to the inbox.

Each notification also has a monotonic push_suppressed latch. Mute atomically sets
that latch for every existing notification from this source and terminalizes only
queued/claimed Push jobs. New muted publications set both flags. Queue scanning,
claim/dispatch and passive service-worker authorization check the latch AND current
source preference. Unmute clears only the source preference; it never clears either
notification flag or requeues work. This covers an unscanned old notification and
backoff without relying on queue retention. A committed dispatch cannot be recalled:
its real acceptance/unknown receipt is retained, and mute never authorizes a resend.
Mute-first rejects held dispatch; dispatch-first may reach the service and display
already committed before mute cannot be undone. The current passive authorization
still rejects after mute (and after unmute because the latch remains set).

No new device/provider/model authority, keyword classifier or navigation is added.
A maximum of 128 sources may be muted per account; the complete muted list is bounded
by that admission limit and includes revoked/paused/restored sources for unmute.
Preferences and monotonic flags survive reopen and sanitized export/restore; existing
restore quarantine still pauses work and retires Push authority. Unmute never resumes
work. Current quiet-hour, account, epoch, session and display-window fences remain.
