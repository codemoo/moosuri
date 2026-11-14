# Notification source mute acceptance

2026-11-14. P5/F32 stable-source delivery control for reminders and recurring missions.

The existing inbox cards let their current owner mute an exact schedule source. Mute
does not change execution authority, schedule revision, model budget or task receipts.
New publications record immutable suppression and remain in occurrence/history rather
than inbox pages. Existing visible/read/dismissed entries remain. A complete bounded
same-page muted-source list allows unmute even after dismissal or schedule revocation.

The preference has its own CAS revision and owner/session validation. Mute atomically
latches suppression on old notices, including unscanned ones, and retires queued/claimed
Push. Scan, dispatch, retry and passive display authorization check current/permanent
suppression. Unmute only affects future publications. A previously committed dispatch
may still be delivered; its real acceptance/unknown record remains and no recall or
resend is claimed. Reopen and encrypted sanitized restore preserve preferences/flags;
unmute does not restart quarantined schedules or Push authority.

Independent read-only review found no material defect in these boundaries or the UI's
stale-account/request fencing. Native focused verification passed schedules13, Push15,
recurring16, actual restore11, predecessor2, migration6, schema20 and upgrade6 selections,
HTTP schedules2/Push1 and Store/server all-target Clippy. These selections overlap and
are not summed. React main typecheck,27 relevant Schedules/Push tests and production
build passed; the existing Vite chunk-size advisory remains.

The first migration batch exposed four synthetic predecessor fixtures retaining new
schema39 objects while rewinding their version. Their setup now removes those objects
before constructing older fixtures; migration/schema/upgrade reruns passed. A separate
shared native build-cache collision resolved definitions from another worktree. Only
the stale core package artifacts were invalidated, after which HTTP and Clippy passed.
Neither failure was relabeled as a pass or repaired by weakening product checks.

Linux integration passed the affected schedules/Push/recurring, migration, actual restore,
schedule/Push HTTP selections, all-target Store/server Clippy, embedded-web binary and
plan checks. The frozen source archive and file hashes were verified. Strict OpenSpec68,
source/history secret scans and current-checkout plan validation accompany publication.
No real Push provider/device, production DB, collector, cron or private
service was used or changed. CI remains disabled. This slice does not implement semantic
topic grouping, the full proactive suggestion policy or automatic historical pruning.
