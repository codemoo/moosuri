# Bounded browser profile recovery — 2026-11-15

P6 adds operator-opted-in browser login-profile recovery after an application
restart. The original app session, account, conversation, deployment and remaining
idle/absolute lifetime still apply. Recovery opens a blank page under fresh human
control. It never replays a tab, navigation, form, POST, send or chat turn. Completing
the handoff from a blank page is rejected without creating a turn.

## Acceptance

Independent review accepted the recovery authority, cleanup and no-replay boundaries
and the subsequent blank-page guard. Findings repaired before acceptance included
schema53 forward-restore registration, inventory overflow, cleanup starvation and
persisted capacity. Root checked the integrated source and retained the following
Linux evidence rather than rerunning already-passing suites:

- Store browser scenarios passed, followed by the affected blank-completion scenario.
  They cover original deadlines/session/pin/control fencing, no chat replay and
  cleanup authority after close, expiry, session revocation and account/thread deletion.
- The actual schema29/30 sealed-restore integration passed through schema53 and
  restored-state quarantine. Its old-schema fixture removes the new columns before
  migration; the original restore assertions remain unchanged.
- Connector browser tests and the agent workspace lifecycle passed. Final affected
  all-target Clippy passed with `-D warnings`. Application and worker builds passed.
- React component checks and the final TypeScript/production build passed. Existing
  takeover behavior and the recovery button/blank completion state remain covered.
- Real Linux Chromium smoke passed: live owned network canaries, scoped handles,
  screenshot and human-login flow, persistent cookie across worker replacement,
  blank recovery, invalid old frame tokens, no authenticated GET/POST replay,
  DNS rebind rejection and direct-egress denial.
- The actual application binary passed HTTP acceptance with two synthetic accounts:
  login then normal server restart, retained app session and profile, explicit recovery,
  rejected foreign account and old control, cookie continuity after a newly requested
  navigation, forced application-process termination and recovery, member isolation,
  profile removal on close and rejection of later recovery. No model calls were made.

A test-image issue initially blocked the second Chromium launch: `certutil -N` waits
on an already initialized NSS database. A bounded diagnostic observed first init
succeed and second init time out. The synthetic CA wrapper now initializes only a
missing database; the complete Chromium smoke and application acceptance then passed.
This changes only the test image and does not disable TLS checks or modify production
trust. Final equivalent Rust `if`-chain/borrow cleanup passed Clippy; behavioral
acceptance used the same functional code before the last agent syntax cleanup.

Acceptance executable SHA256:
`c88b8f5b30b9236696b6ecd1ac7f359d2e3722914ed0b398c09afbbe2609f120`.
Synthetic Chromium image:
`sha256:96f0aed2258a3eed43f311b8474f26faf92bef2128104ecbee9a8b5cfba90c55`.
The image combines the source-built workers with the repaired synthetic CA wrapper;
it is development evidence, not an official release artifact.

## Remaining boundaries

Persistent cookies/localStorage survive only as committed by Chromium. Session cookies
may expire, recent writes may be lost in a crash, and sites may require a new login.
A killed application process is not a host power-loss durability test. Real websites,
accounts, physical mobile devices and production deployment remain separate gates.

Profiles are server-managed, not a zero-knowledge vault. Local Docker volumes have no
hard filesystem quota; operators must provision capacity/quota/monitoring. Cleanup
requires the app/daemon to resume; resources under retired owner/project labels need
scoped operator cleanup. Automatic authenticated forms and indefinite login retention
remain unsupported. CI/Actions stay disabled.
