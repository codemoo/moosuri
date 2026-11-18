# Official Claude subscription login

The web now opens the authorization URL produced by unchanged Claude Code2.1.322
and submits the returned code to the original process. The five-minute in-memory
attempt belongs to one Moosuri session and binding revision. Raw terminal output,
URLs, codes and Claude bearer tokens are not persisted. Login success and real
exact-model verification remain separate; tools/media/background are unsupported.

Independent review covered session ownership, Origin/no-store boundaries, process
cancellation, output bounds and proxy admission. A late-status UI race was found,
fixed with operation generations and checked by deferred-response tests.

Validation: focused Rust Claude suites28 checks; React connection/provider/app
60 checks; web typecheck/build. Synthetic tests include original-process code
delivery, nonzero exit, untrusted URL rejection, repeated start, duplicate code,
separate sessions and logout cleanup. Official per-block stream snapshots and
rate-limit frames were reconciled with the published message contract.

Actual credential-free Linux acceptance used rootless Docker, the immutable image,
unchanged official CLI and synthetic canaries. Host/sibling/symlink/FD isolation,
direct DNS/egress denial, read-only root, empty capabilities, no-new-privileges,
CPU/memory/process ceilings and bounded auth byte/inode exhaustion passed. The
filesystem limit is global to this runtime, not a per-user quota. Official piped
login produced the documented authorization link and code prompt. Private receipts
retain artifact hashes and fixed statuses only.

User-completed official authentication and real exact-model inference remain
pending. These tests do not claim subscription eligibility or broad assistant
capabilities. See [operator setup and owner flow](../claude-execution.md).
