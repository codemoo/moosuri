# Optional system invitation email implementation evidence

Base d5a2f2e; schema43 is intentionally integrated after root continuation42.
The People UI keeps existing account creation/reset/copy flow and adds separate
fixed invitation preparation, optional one-time password inclusion, complete final
preview, explicit approval and immutable effect status. Sender absent/failure does
not roll back the account. Dedicated Store effects and admin routes never expose
these passwords to model-visible personal Gmail drafts.

The pending snapshot is encrypted with the server-managed provider key, limited to
16 pending per actor and15minutes/credential expiry, bound to the original actor
session+epoch, target current temporary credential revision+epoch and configured
sender pin. A supplied password passes the existing authenticated bounded KDF and
current hash CAS. One effect per credential epoch, exact digest approval, durable
commitment/payload purge before one native send, unknown on any post-spawn failure,
startup unknown, no automatic retry, reset/delete/activation/restore fencing and
late original receipt persistence are implemented. Lists contain metadata only;
original-session GET by effect ID retrieves a still-pending preview. Deletion
scrubs address/subject/receipt metadata, including late receipt handling.

The optional native adapter was designed from local gws0.22.5 `--help`/`--version`
and public tagged source only. `gmail users messages send --upload payload.mime
--upload-content-type message/rfc822` is documented native media input. Public
v0.22.5 executor has one request.send() for this path, independent of the separate
library retry helper. Startup config explicitly supplies native executable,
0600 credential file,0700 config/staging directory and From; environment is cleared,
stdout bounded, stderr/logging disabled, timeout kill-on-drop. Version readiness
precedes commitment and the final authority check; no awaited local preflight
separates that check from native send spawn. Credential fingerprint binds sender
pin and is rechecked synchronously before send.

The CLI canonicalizes --upload under CWD, so stdin/@JSON/unlinked-descriptor inputs
are not invented. A new private staging folder/0600 MIME file is used and removed
on normal/error/timeout exits. Hard-crash residue is cleaned when this explicitly
configured sender next starts. This is not a claim of encrypted dispatch staging,
physical erasure or a hostile native process sandbox. The operator asserts the From
account/grant; configured status does not falsely mean verified Google profile or
scope. Unsupported native versions are rejected before send. No real gws
credentials, live Gmail sends or Google OAuth/account checks were used in validation.

## Focused gates

- Store3/3 passed: full frozen preview, encrypted payload, verified optional password,
  idempotent same request, mismatched digest, one commitment, unknown/no repeat,
  late receipt; actor/member/session/sender/reset/delete/expiry fences; startup unknown.
- Connector2/2 passed on final adapter: a synthetic copy of native `/usr/bin/false`
  fails version readiness; explicitly exercised send branch returns unknown after
  spawn, cleans private staging, preserves env allowlist; stale exact-format
  staging cleanup preserves unrelated config. These are native process/cleanup
  fixtures, not live gws/API acceptance. The product never executes scripts.
- React12/12 passed across InvitationEmail, AccountControls and
  AccountPasswordLifetime: explicit opt-in+preview, no send before approval,
  missing sender, lost ACK/status-only recovery and stale revision clearing.
- Affected Store/connectors/server library Clippy `-D warnings` passed. Normal
  CLI/server build during generated DTO export passed. Changed Rust was formatted;
  strict named OpenSpec and diff --check passed. No dependency or Cargo.lock change.
- Generated shared API types were exported from core. Worker TypeScript and Vite
  production build passed. Whole main TypeScript is blocked by the base checkout's
  two MailTiming.test fixture diagnostics (null optional schedule; unused init),
  already repaired on root integration. No invitation TypeScript errors were emitted.

After native-lane release, a narrow SQL-only late receipt guard was added so deleted
actor/target metadata stays scrubbed. Root integration should include this path in
its independent focused acceptance. Earlier compiler/test failures were corrected:
missing test imports, universal Mach-O fixture magic, Clippy collapsible conditions,
and multiline React text matcher. No failed gate is represented as passed.

Independent review, HTTP/platform integration and real operator/device/provider
readiness are separate remaining acceptance. No changes were made to legacy
collector/cron/services/DB, CI or personal account auth state.
