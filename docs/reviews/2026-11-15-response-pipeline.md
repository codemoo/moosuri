# Response reliability and connection reuse

This unit follows the earlier [source-repair latency comparison](2026-11-15-source-repair-latency.md).
It fixes local failures and redundant connection work. It does not change model
selection or reasoning effort and does not claim a live-provider speedup percentage.

## Confirmed defects

- The foreground tool validator allowed32 definitions. The actual complete profile
  has32 by default and40 with credential-broker chat enabled. The latter failed
  before model dispatch. The ceiling is now48; schema byte ceilings, admitted
  names, owner grants and execution budgets remain enforced. The old-limit failure
  and actual profile composition are recorded in [complete tool profiles](2026-11-15-complete-tool-profiles.md).
- Every model round built a new HTTP client with idle pooling disabled. One
  configured ModelClient now retains at most one idle connection for30seconds.
  Each request still resolves and validates every DNS answer. A different origin,
  full address set or transport limit retires the pool. Authorization remains on
  each request; there is no process-global pool, redirect, proxy or retry.
- Chat streams could remain silent forever, exhaust six lifetime reconnects after
  otherwise healthy connections, or clear/reload history after an accepted send.
  Stream connect/read deadlines, consecutive failure counting and matching SSE
  receipts address those paths. Snapshot GET requests also have one30second
  headers-and-body deadline. See [chat recovery evidence](2026-11-15-responsive-chat-stream.md).

## Verification

- Linux:28 model transport,15 tool protocol and3 complete foreground-profile tests
  passed. Strict no-dependency Clippy for agent/connectors with broker chat enabled
  passed. Native transport tests also passed. Existing unrelated broker dependency
  Clippy findings are outside this unit; no workspace-wide lint success is claimed.
- The TCP fixture observes three completed sequential tool requests on one accepted
  connection. Separate clients and changed limits use new connections; newly
  denied and changed DNS sets cannot bypass validation through an old pool. Clean
  EOF, cancellation, partial-error and no-retry regression checks remain passing.
- Frontend:71 focused tests passed, including deadline/account closure, healthy
  recovery beyond six connections, bounded immediate failure, POST/SSE races,
  history retention, explicit resend and delayed snapshot disposal. TypeScript
  and the production web build passed. All three code scopes received independent
  review with no material outstanding findings.
- A real temporary Chrome profile used a fresh local account, SQLite store and
  synthetic streaming provider. Two chat messages completed. For the second send,
  observation recorded one message POST, zero message-history GETs, zero new SSE
  requests, no disappearance of the first answer and six partial answer updates.
  Conversation-list metadata still refreshed normally. This browser check covers
  the streamed-send change; the subsequent GET deadline is covered by controlled
  transport/component tests and the final build.
- The final production web assets were embedded in a successful Linux CLI build.
  The source and asset manifest hashes were verified in the isolated build.

## Limits

The browser check uses synthetic output and establishes UI/network behavior,
not model quality or real-provider latency. Connection reuse benefits callers
that retain a ModelClient; current personal-subscription readiness can reconstruct
one between rounds and does not yet receive this pooling benefit. Browser
suspension can defer timers. Mutation request timing and uncertain-send recovery
remain unchanged. Production deployment and real-device/mobile network acceptance
were not performed. CI remains disabled.
