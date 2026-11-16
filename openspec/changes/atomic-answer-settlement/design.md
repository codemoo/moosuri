# One answer disposition, one writer transaction

`agent_execution/completion.rs` owns run-level failure settlement, exact-step
terminal settlement and foreground answer disposition. Their guards remain
distinct: run-level failure cannot declare completion, step-level completion
requires a committed no-tool step, and foreground disposition also excludes
missions that require evidence verification.

`settle_agent_answer` guards the exact current step, validates completion, applies
only its assigned guidance and either completes the answer or demotes its output
for newer guidance. Receipt state, output phase and terminal state roll back
together. Shared instruction mutation helpers require the caller's transaction
and guards; the replaced standalone supersede Store method is removed.

The agent uses the existing local Busy-only retries. A failure cannot authorize
another model round. Unknown acknowledgments are never replayed; cancellation,
stale leases, tool slots and local memory decisions retain their original guards.
Mission tool-round acknowledgment uses the same local retry policy, while mission
candidate verification and continuation remain domain-specific.

Synthetic SQLite TEMP triggers fail completion or output demotion after guidance
acknowledgment. The original separate sequence reproduces both faults. Tests check
rollback of the complete visible turn, later successful completion/continuation,
one final answer after reopen, unchanged dispatch counts and tool/cancellation
guards. Existing held-stream and recovery scenarios exercise the real Runtime.
