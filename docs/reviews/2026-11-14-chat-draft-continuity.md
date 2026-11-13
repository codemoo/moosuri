# Chat draft continuity acceptance

2026-11-14. Bounded tab-session recovery, with no automatic send queue.

The server exposes a non-authenticating, domain-separated draft scope for the exact
current unrestricted session. The browser restores a bounded next draft and original
uncertain request only after confirming that scope/account and the owned thread list.
Pending restoration uses GET reconciliation; an explicit resend keeps the original
UUID, text and immutable attachment/target/context carrier. Acknowledgment never
overwrites text entered afterward. Logout/auth transitions synchronously retire the
storage lease so delayed callbacks cannot repopulate it.

Independent review reproduced CHAT-DRAFT-R1: automatic online recovery after a failed
offline boot cleared saved bytes before authenticating the same session. Connectivity
retry now preserves inaccessible bytes and synchronously abandons the old boot probe.
Current-scope checks fence late system/session/401/error replies. Fresh current401,
changed account/scope, restricted sessions, password changes and logout still clear
recovery. Red9 failures and the original review remain retained privately. The repaired
23 lifecycle tests, focused70 frontend tests and26 independent reviewer cases passed.

Root integration passed78 focused React tests, TypeScript/build and exact Rust-exported
DTO comparison. Native and Linux HTTP suites each passed34 tests with one explicit
public-network case ignored. The older knowledge fixture mismatch was independently
corrected in [its own test-only change](2026-11-14-knowledge-http-fixture.md).
Affected-crate Linux all-target Clippy passed.

Actual Chrome at390×844 with the Rust server, isolated SQLite and a scripted loopback
model verified new/selected-thread draft reload, offline public-shell boot without
private UI, automatic online recovery of next typing and uncertain request, zero
automatic POST, explicit byte-identical resend and one saved request/run. A confirmed
answer preserved the next draft; settings navigation did too. Logout removed saved
bytes, and a new same-account session did not restore them. Public caches contained
no private API responses. Screenshots had no horizontal overflow. Browser/server were
stopped and the temporary executable was removed after its hash was verified.

Storage is plaintext sessionStorage in the trusted browser, not vault encryption:
at most16 slots,128KiB total,8192 UTF-8 bytes per text and8 hours. Media bytes and API
responses are excluded. Closed-tab/OS recovery, compromised scripts/extensions,
actual iOS/Android behavior and live model quality are not established by this slice.
