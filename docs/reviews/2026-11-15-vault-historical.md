# Manual pinned vault journal recovery evidence

Scope: distinct full-journal age format/head, required receipt hash, non-sealing
manual export, private exact-prefix typed forward recovery and provenance. No
schema/dependency, general crash-tail admission, HTTP export, permission UI,
production data/credentials or deployed service changes.

Native historical gates: negative DB/pin/chain/CAS/ciphertext1 and required-pin CLI1
passed. Actual multi-owner ciphertext recovery1 passed after fixing the fixture's
missing ClientVault activation after its verified root-rotation receipt. Production
code did not change for that fixture error. Recovery preserves later root/key
rotation, receipts, item tombstone, deleted-owner fence, epoch revoke, new-owner
ciphertext and cancelled/pending nonce reservations. Imported sessions are absent,
pending commits return aborted, altered reserved ciphertext rejects, and exact
rerun preserves a fresh session. Snapshot/journal use different synthetic age keys.

Negative fixture rejects wrong pin, corrupt ciphertext, older journal, same-source
divergent prefix, truncated record, foreign source and hash-valid impossible CAS
before destination publication. A valid same-source Close tail succeeds. Affected
prior plain snapshot1 and sealed-current receipt/tombstone/reservation1 also passed
through their unchanged no-forward branches. No full legacy suite was repeated.
Vault-service all-target Clippy with `-D warnings` passed on final source; exact
commands and hashes are in the freeze handoff. Main owns independent review
and Linux integration. No actual off-host/current-latest/zero-loss, real account or
production recovery claim is made.

## Independent and Linux acceptance

Root independently reviewed the15 frozen files and exact patch/file hashes. The
review covered independent receipt pinning, complete authenticated age bounds,
source/exact-prefix admission, every typed intent and retained table reconciliation,
nonce/deletion/key/receipt effects, staged publication and repeat-restore preservation.
No material finding remained. Linux passed the three historical gates, the affected
plain snapshot and sealed-current recovery cases, all-target Clippy and default
vault binary build. The integrated15 file hashes match that frozen source exactly.
No production DB, real credentials or service rollout was used.
