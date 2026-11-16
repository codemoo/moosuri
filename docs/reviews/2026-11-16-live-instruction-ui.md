# Live instruction composer UI

Synthetic frontend implementation from attachment freeze `7732508` in a separate checkout. No screenshot text, private instructions, production data, real provider calls, Cargo, deployment or push. The parent integrates schema 72, regenerates shared API types, independently reviews boundaries and runs browser/live synthetic evidence.

## Behavior and ownership

One main textarea has explicit instruction/next-message modes. Supported active execution defaults to instruction mode; typing in a normal next-message draft pins that mode when support appears later. Empty completed instruction mode returns to the next-message composer, and an empty state targets a later supported run. A nonempty old draft or uncertain request never silently retargets another run or attempt.

Instructions capture an immutable UUID, exact text and backend-supported run/thread/attempt. POST is bounded to 30 seconds, never dispatches /messages or mission creation/control, and displays received immediately from its actual response or authoritative SSE. Received is visibly distinct from applied and closed. Matching canonical receipt cancels a held redundant POST; its response/finally is fenced from a subsequent operation. Receipt display lives with its original user/assistant turn or Mission card, not a permanent stack in the composer footer.

Only text is admitted. Any selected pending/failed/ready attachment requires an explicit choice to retain it for the next message. No instruction path calls media.sent or discards/replaces uploads. Voice/shared insertion selects the normal next-message mode. The instruction/next drafts and immutable uncertain carriers remain independent. A terminal rejection or closed unapplied receipt restores only the instruction draft.

Unknown acknowledgments read the original receipt by GET. A 404 exposes explicit same-ID resubmission; no effect automatically POSTs. Outstanding received receipts use bounded GET observation while mounted; SSE/snapshot terminal states supersede prior received metadata. Account/thread request scopes and captured callback/transport ownership fence delayed responses. Account/session tab recovery preserves instruction drafts and exact pending carriers only as uncertain, using the existing 8-hour/16-slot/128KiB envelope and exact-key parsing; Mission entries share those bounds and are rendered only with the current owned Mission list.

RunView supports_instructions gates foreground actions. For completed mission admission and active Mission cards, the backend-provided instruction_target chooses the actual carrier; no latest/sole/global task guess or per-token version CAS occurs. Paused Mission controls, original authorizations, model/effort and final mail approval remain separate.

## Files

New RunInstructions.tsx owns receipt transport, recovery and compact receipt/card rendering; run-instructions.ts owns the small memory/validation helpers. ChatPanel.tsx owns the single composer mode/IME/media integration. App.tsx and chat-drafts.ts provide minimal lifetime-owned update/persistence plumbing. Missions.tsx has a small MissionInstructions mount. Media.tsx exposes only the selected-instance fingerprint for explicit retention. chat-state.ts validates the optional bounded backend DTOs. Shared generated api-types.ts has only confirmed request/receipt/target and optional RunView/MissionView additions; parent regeneration is controlling.

## Observations and results

- Nine affected suites passed 139 tests: RunInstructions, Chat, ChatPanel, ChatDraftContinuity, chat-drafts, Missions, MissionInput, MissionChatContinuity and ScheduleChatContinuity.
- New observations cover one textarea/no competing turn, immediate received versus SSE applied, explicit media retention, exact uncertain payload/UUID replay after GET404, next-draft continuity, terminal 409/closed recovery, UTF-8 limits, IME, old-thread/account late-response disposal, held POST settled by canonical receipt, actual Mission carrier routing, automatic terminal return to next-message input, and asynchronously enabled capability preserving the typed next draft.
- Draft observations recover exact instruction/Mission target/request data only as uncertain beside the normal draft, reject extra stored authority fields and retire another public session scope.
- `npm run build` passed app/worker TypeScript and Vite production output.
- `git diff --check` passed.

The backend owner maintains the live-run-instructions OpenSpec. These frontend tests use controlled fetch/SSE, not a live provider, and establish no timing or quality score. Real browser acknowledgement, held-model continuation, server transaction/permission proof and integrated native/Linux evidence are parent-owned. In unsupported execution paths, no additional-input authority is inferred.

Combined integration and later repairs are recorded in [live guidance and monitors](2026-11-16-live-guidance-monitors.md); that record supersedes the parent-pending notes above.
