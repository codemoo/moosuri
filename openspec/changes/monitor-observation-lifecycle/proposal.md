# Monitor observation and reuse lifecycle

A changed source can quarantine a completed child while the separately approved
monitor remains active. The notice gate checked that monitor policy and resolution,
but not the child's withdrawn result, so an old achieved notice could remain visible
and deliverable. An invalid reusable match also stopped proposal handling before
the current completed task received its own monitoring proposal.

Tighten the shared observation gate used before publication and by final inbox/push
checks. Retire invalid reusable matches using the same policy classification as
ordinary admission, then continue normal proposal selection. Keep valid partial
reviews, original finite permissions, no-change gating and no automatic retry of
failed/unknown observations. This is a P4/P5 lifecycle repair, not new authority.
