# Reuse existing execution and evidence

The compiled world catalog remains the source of expected case IDs. Each world
belongs to one feature. No model, endpoint, credential, database or fixture path
can be supplied to the regression runner. Default mode keeps all existing worlds
and controls; feature mode selects worlds before setup and retains all applicable
controls plus the setup-failure denominator control. Missing rows, failed controls
and unselected features cannot become passing feature results. Selection is part
of profile hashing and the format-4 report.

Recurring polling excludes any same-schedule child in queued, planning, executing,
verifying, paused or waiting state before ranking candidates per owner. Waiting
releases owner capacity, but retains this schedule's unfinished child. Existing
latest-only coalescing, transactions, due-based deadlines, terminal reconciliation
and cancellation stay authoritative. No second scheduler or queue is introduced.

The stability matrix links existing feature contracts and acceptance gaps. It is
not a new test framework or a claim of live-model/device/provider reliability.
