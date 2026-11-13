# Local operator account recovery — 2026-11-13

The development CLI now enrolls a new temporary credential for an existing account
locked by an auth-journal/SQLite crash gap. The operator must stop the installation,
select its account UUID and current role explicitly, and supply an owner-only
password file. The new credential must differ from the stored revoked password.
No session is returned; login requires password exchange within24 hours.

The existing opaque auth journal is fsynced before the role/password/session/head
transaction. Failure after append poisons the writer; reopening revokes old
authority and requires enrollment again if the credential commit is missing. A
lost response after commit preserves the new temporary credential. Healthy and
missing accounts cannot be reset, and a missing installation is not initialized.

## Synthetic verification

macOS arm64 with Rust1.89, SQLite local files and no provider/network calls:

- The initial five focused store cases cover explicit member/admin roles, temporary exchange,
  old-password rejection, other-user preservation, competing attempts, missing or
  healthy accounts, preexisting installation/lock, failed SQLite update after the
  real fsynced append, poisoned writer/replay/retry and lost postcommit response.
- A held model run lease, custom-source token/lease and mail dispatch carrier stay
  unauthorized after recovery and password exchange; the current shared provider
  revision is preserved. No model or mail is actually sent.
- CLI subprocess verification rejects running, missing and healthy installations,
  malformed account/role and unsafe password files without printing the password.
  Successful enrollment and commit-fault behavior are checked through the trusted
  local wrapper/store boundary.
- Password-file checks cover symlinks, hard links, permissive mode, directories,
  sockets, FIFO and size limits; code also requires current-OS-user ownership.
- The store/CLI regression run passes122 store tests,3 CLI unit tests and2 CLI
  integration tests (one existing init/HTTP/restart journey, one recovery boundary
  journey). The final focused rerun also checks the revoked-password reuse guard.
- Independent review found that an empty existing DB file could trigger Store
  initialization before rejection. The repair checks required installed files and
  inspects a private disposable DB+WAL copy under the existing writer lock before
  opening the live Store. Empty, schema0 and accountless installed fixtures retain
  exactly their original file set and byte hashes. Seven final focused cases pass,
  including a real child-process exit without Rust drops whose account/schema
  remain only in committed WAL; that case confirms the preflight reads WAL.
  Temporary copies are removed on normal/error return; abrupt process death can
  leave an owner-only preflight directory for the operator to remove.
- Format and workspace/all-target clippy with warnings denied pass.

The fault test uses a test-only SQLite TEMP trigger to abort the update after the
real journal fsync, rather than a production environment failpoint. The lost-ACK
test commits and deliberately drops its success response. The child-exit WAL check was also run on Linux. It is a real process exit without Rust drops, not a SIGKILL-at-every-instruction test.

The integrated Rust workspace passes340 top-level tests on both macOS arm64 and
Linux x86_64, with one opt-in live-model test ignored. A nested writer-lock child
repeats an existing test and is excluded from that total. Linux uses Rust1.89.0,
network none, a nonroot pinned container and read-only source; frozen source hashes
match the tested tree. The existing React134 checks remain applicable because this
unit changes no web code. Format, clippy, plan and strict OpenSpec checks pass.

Independent reviewers reproduced the uninitialized-DB defect, then rechecked the
12-file repair: required files/lock, private clone, WAL preservation, cleanup/drop
order, live Store lock and auth epoch boundaries. No remaining blocking defect was
found. The zero-byte rejected DB and its directory remain unchanged.

## Limits

This is a trusted local operator boundary with no HTTP recovery endpoint, schema
or crypto dependency change. It is not a general password reset, account
suspension/deletion override, encrypted archive, empty-host restore or vault
recovery. It does not rewrite old work/connector epochs or automatically reconnect
credentials. Original services/cron/DBs and production data are untouched. CI
remains disabled; backup/journal/restore release gates remain pending.
