# Design

`observation_usable` binds the observation to its child and rejects resolution,
quarantine/deletion/withdrawal and execution invalidation. `publication_current`
combines this with the existing owner, source, account, epoch, parent and expiry
policy checks. Recurring publication and final inbox/push paths already call that
predicate, so they share the corrected result lifetime without another delivery
policy. A valid partial or failed outcome is not rejected solely for its state.

`retain_current_policy` centralizes the existing expiry-vs-withdrawal retirement
choice. Admission and reuse selection call it. Reuse lookup continues after retiring
an invalid active match, so another pending proposal can be linked or a new proposal
can be recorded for the current completed task. Expired/revoked permission is never
reused or silently renewed. Valid reusable matches keep their cadence/expiry and
existing coalescing behavior.

No schema or generic mission/input-wait/recurring implementation changes are needed.
Synthetic outcomes use actual Store source ingestion, child read/verification,
source invalidation, deletion and notification/push authorization. The old source
review remains withdrawn while a later changed-source review can run once under
its still-valid original monitor permission.
