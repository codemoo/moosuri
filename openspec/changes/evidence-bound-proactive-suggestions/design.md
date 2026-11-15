## Policy and identity
The only fixed product topic is followup_review. An explicit save of an active,
unmuted followup with all due/review/snooze fields absent can admit it. The topic
anchor is stable per owner; the suggestion schedule is stable per artifact.
No text classification or title grouping determines either identity.

A published occurrence is the permanent dedup receipt, including suppressed
publication. A pre-publication save fences the old revision and may retarget the
same identity; post-publication edits redact and retire without replay. The shared
cap counts pending schedules and valid undismissed unsuppressed inbox entries.
Hiding/withdrawal releases capacity; no overflow queue or old-record scan exists.

## Existing mechanisms
Two content-free sidecars bind topic/record to internal fixed schedules. Current
artifact head/status/version joins gate publication and link delivery. General
reminder listing/control/chat selection excludes internal schedules. Existing
owner grants, account/epoch checks, source preference CAS, immutable notification
suppression, quiet hours, Push latch and restore quarantine remain authoritative.
A source mapping resolves the same topic anchor in inbox and all Push scan,
dispatch/retry and passive-display checks. Dated followup source mapping is separate.

## Boundaries
Normal and offline migration add only empty schema57 tables. Account deletion's
tenant-scoped cleanup includes both; FK edges follow existing schedules and artifact
versions. The existing artifact invalidation hook calls suggestion retirement for
normal edits, deletion and transitive withdrawal. A suggestion only opens the
existing editor. It cannot admit a chat run, mission, tool or external action.
