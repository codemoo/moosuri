# Conversational memory target resolution — 2026-11-13

A correction or forgetting request can describe a remembered subject without
copying its stored words. Exact unambiguous matches keep the existing direct path;
a literal miss may use one isolated bounded semantic selection. A literal
ambiguity still asks for clarification.

The Store seals the current user's original action/fact, the complete eligible
candidate snapshot, generation and revisions before dispatch. The model sees only
candidate data and opaque indexes. It cannot choose another action, replacement
fact, database ID or account. Up to64 effective memories/32KiB are admitted;
overflow asks for a narrower request rather than selecting from an incomplete
list. Selection and commit recheck the entire snapshot and selected head.

## Failure and invalidation checks

An independent review found that a candidate later forgotten could remain in a
private target plan and be sent again on retry. The final implementation registers
all candidate/evidence dependencies when sealing, purges plans/checkpoints during
withdrawal, and checks the snapshot again before resume or dispatch. A durable
started fence prevents a purged plan from being classified anew. Source withdrawal,
unselected candidate deletion, clock-dependent validity, retry, cancellation,
account/provider changes and stable same-ID correction/forget receipts are tested.

The second independent review closed that defect. Integration review checked
migration0011→0012, the shared schema bound, community pin/removal logic and memory
purge/dispatch fences together; it found no concrete remaining defect.

## Evidence and limits

- macOS focused Store31 and agent21 tests passed, with the separate live-model test
  ignored. Schema migration and HTTP memory-command boundary tests also passed.
- Full Linux x86_64 workspace:362 passed,1 live opt-in ignored. The complete
  frozen source manifest matched before and after execution.
- Formatting, all-target clippy, plan links and strict OpenSpec validation pass.
- A separate author froze12 Korean/English holdout cases before model calls. The
  apparatus used a fresh synthetic DB and isolated local official Codex app-server,
  with `gpt-6.1-terra/high` and no native tools/MCP surface. No account token was
  copied or exported.
- Both real connection attempts passed text exchange and invoked the protocol
  challenge tool, then returned an empty final assistant message. The application
  correctly refused to record tool capability. The six HTTP model rounds and both
  failed connection attempts remain recorded; **zero holdout requests were sent,
  all12 are not attempted**. No test capability was injected or proof bypassed.
- Consequently this slice establishes storage/protocol behavior, not a newly
  measured semantic success rate. Historical7/10 and focused3/3 observations remain
  historical. The held-out semantic and injection evaluation still needs a working
  verified model connection. No new whole-assistant quality claim is made.

The private harness is an evaluation transport, not a released ChatGPT subscription
integration. No production account data, email, collector, cron or existing service
was changed. GitHub Actions remains disabled.
