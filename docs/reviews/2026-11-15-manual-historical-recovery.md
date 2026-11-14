# Manual pinned historical recovery — bounded implementation

This unit adds `backup journal` and `backup restore-historical`. It does not
complete ongoing/live/unattended/off-host publication, independent latestness
services, vault-service historical recovery or real installation migration.

## Trust and storage boundaries

The publisher holds the stopped source writer lock and does not seal or mutate
the source. A current-schema, complete applied three-journal checkpoint encrypts
with the active verified public age recipient. Its separate head binds both this
ciphertext and the exact operator-selected retained export ciphertext. Age alone
does not identify a writer. Restore requires an independently obtained latest
head digest; a stale head cannot match a newer digest. A caller supplying a stale
head and stale digest remains outside local rollback detection.

A distinct `assistant-historical-journal-v1` purpose and head provenance never
claim a source seal. Shared sealed-restore quarantine now accepts purpose-neutral
provenance; sealed checkpoint validation/admission and its original trust boundary
remain in their existing path. The compatible `installation_restores.seal_id`
column carries an opaque recovery event UUID for the historical protocol, while
`trusted_head` preserves a distinct `assistant-historical-restore-v1` receipt with
the actual journal purpose, checkpoint identity, independently pinned head digest
and immutable `accounting_unknown` condition.
No schema or new host-local signing secret was introduced.

Restore authenticates both age EOFs before bounded fixed archive extraction,
checks all inventory hashes, source installation/epochs, complete original
predecessor/successor watermarks, exact prefix extension and latest pinned heads.
The existing explicit migration ladder precedes destructive suffix replay.
Historical withdrawal handles revision drift conservatively instead of replaying
missing ordinary edit history. Artifact withdrawal does not invent a successful
original mutation receipt at the snapshot's different revision.

Before atomic publication, every account needs local enrollment; all historical
roles are `member`. Opaque auth epochs cannot reconstruct latest role/suspension,
so the operator must choose current account eligibility/role. Existing suspended
status stays suspended; deletions are applied and tenant private rows purged.
The shared restore quarantine retires old sessions/provider credentials,
connections, browser control, Push and automatic work. Journal-supported UUID
withdrawals are covered; non-journaled removals such as community uninstall are
not claimed reconstructed. Later unexported additions/receipts/usage are absent.

The recovery is data-only until an explicit new-budget/reconcile flow exists.
Freshly configured shared/personal model binding/probe access, audio/vision and
automatic-memory binding, subscription access and current usage-total views are
blocked by immutable accounting provenance. All prior restore provenance remains
in scope, preventing a subsequent sealed restore from laundering that gap.

Matching retries fully verify supplied inputs, match immutable provenance and only
sync the existing destination; later enrollment/provider changes survive. The CLI
prints opaque enrollment IDs and requires an explicitly chosen current role.
Supported journal schemas span20..CURRENT with snapshot<=journal; migration is
explicit and happens before replay.

## Focused evidence and remaining gates

Final native base schema47 real Store gates passed on 2026-11-15:

- `cargo test --locked -p moosuri-store cutover::restore::historical::tests --no-default-features -- --nocapture`: 2 passed (11.28s).
- `cargo test --locked -p moosuri-store exact_restore_requires_enrollment_preserves_tenants_withdrawal_and_quarantines_external_work --no-default-features -- --nocapture`: 1 passed (5.33s). This checks the shared purpose-neutral quarantine still supports the existing sealed trust path.
- First historical test exercises retained old export, later forget, post-snapshot absent UUID forget, latest account deletion/private tenant purge, unrelated tenant retention, independent old/new outer keys, source reopening, stale head against newer pin, wrong/missing key, missing/mismatched journal and failed head publication leaving only orphan ciphertext.
- It injects failure after destination rename, verifies matching retry completes publication without replaying quarantine, reenrolls an admin with a new password, configures a new synthetic provider and proves shared probe, personal subscription access and both usage-total views remain unavailable under immutable `accounting_unknown`. A later matching retry preserves the new enrollment/password/provider state.
- Second historical test edits an artifact after export, deletes its later revision, restores conservative snapshot withdrawal without inventing the original deletion mutation receipt, and rejects a fully valid independently pinned Store journal fork that does not extend the chosen snapshot prefix.

The current source is frozen in `/private/tmp/moosuri-ongoing-recovery-source-freeze-r1.json`; this local path is review tooling, not a product recovery input. `cargo run --locked -p moosuri -- backup restore-historical --help` also built the real CLI and passed with all required trust/key inputs. Independent root review accepted R1 after repairing accounting admission/usage guards, supported journal-schema handling, opaque enrollment IDs and matching published retry. Joined Linux x86_64 actual historical Store tests (2) and selected shared sealed-restore test (1) passed on schema48. Root simplified one nested artifact-withdrawal condition for Clippy; the affected artifact/fork test, Clippy, default embedded-web build and plan verification then passed. The already-passed full historical/sealed gates were not repeated for that syntax-only correction. These selected checks do not claim every destructive effect type, cross-schema historical fixture, live publisher or real provider/device validation.

No real account/provider/model/mail calls, private records, original services,
cron/collectors or original databases were used or changed. CI stays off.
