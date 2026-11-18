# Offline per-owner manual knowledge extraction

This is a bounded implementation slice of
[account-runtime migration](../openspec/changes/isolate-account-runtime/implementation.md),
built on the [schema inventory](account-runtime-inventory.md). It creates an
**inert partial archive**, not a runnable account volume or a complete migration.
Use it for synthetic rehearsals at this milestone. No production extraction,
authenticated lineage transfer or cutover acceptance has been established.

```sh
cargo run --locked -p xtask -- account-runtime-extract \
  --scope manual-knowledge-v1 \
  --snapshot /private/work/synthetic-stopped-copy \
  --output /private/work/account-archive \
  --account 11111111-1111-4111-8111-111111111111 \
  --tenant aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa \
  --volume 99999999-9999-4999-8999-999999999999 \
  --generation 3

# Original snapshot is unnecessary for archive inspection.
cargo run --locked -p xtask -- account-runtime-extract \
  --verify /private/work/account-archive
```

AccountId, TenantId, intended VolumeId and generation are explicit inputs. The
account/tenant pair must exist, be active, not deleted and not in recovery. The
volume and generation are an operator-supplied binding, not installation-registry
authorization. Email never selects an owner. UUIDs and positive generation are
validated; schemas 81–85 must match the frozen inventory and public migration hashes.
Future or unclassified schemas fail before output creation.

The source must be a canonical owner-only 0700 directory containing **only** one
regular owner-only, single-link `assistant.sqlite` file. Writer locks, journals,
keys, WAL/SHM, symlinks and unknown files are refused. The source DB is opened as
immutable/read-only with a query-only transaction; its SHA-256 must remain the
same after extraction. Supply a complete, stable stopped/checkpointed copy and
do not modify it concurrently. The tool cannot prove that a live DB was copied
consistently or that its original runner was drained.

The output parent must be canonical owner-only 0700. The destination must be new
and disjoint from the snapshot; existing files/directories are never overwritten.
It contains private 0600 `knowledge.sqlite` and `manifest.json`. The manifest is
published last after reconciliation. Ordinary failures remove only this newly
created attempt. A hard process/host crash can leave an incomplete directory;
verification rejects missing manifests, journals and extra entries, and a retry
requires a different absent destination. No source or existing output is removed.

## Supported graph

The extractor copies the selected owner's rows in 11 classified knowledge tables:
collections, sources and all retained source versions, manual ingest receipts,
memories and all retained versions, temporal barriers, evidence, write receipts,
suppressions and knowledge generation. It also copies the tenant ID and projects
only account ID, tenant ID, name and email into archive metadata. Password hashes,
sessions, roles, auth epochs, provider secrets and runtime authority are excluded.

Every copied table is reconciled by row count and a deterministic typed-row
SHA-256. The full source inventory first checks owner joins, declared foreign
keys and cross-owner relationships. The archive additionally verifies source
content fingerprints, embedded memory evidence against exact retained owner
source revisions/UTF-8 byte ranges, evidence-table quote hashes and ingest receipt
source membership. Missing or ambiguous provenance fails the extraction; it does
not guess another owner or an older source revision.

Connected-source provenance and automatic-memory grants/jobs/origins are currently
unsupported: any such selected-owner rows fail closed before output creation.
Manual collection `connected` state alone is not interpreted as provider provenance.
All other classified tables are explicitly outside this **partial knowledge scope**;
the manifest records their source and selected-owner row counts and dispositions.
Their omission is not a migration-completion claim. Credentials, external-effect
receipts, shared spend and live jobs remain in the original installation.

Deletion markers and suppressions survive. Deleted source/forgotten-memory
plaintext must already be purged; retained plaintext under those markers is an
error. Content-free temporal barriers may survive purged historical memory
versions. Their removal would allow older statements to reappear, so the archive
copies them exactly.

FTS and its shadow tables are never copied from the mixed source. Search is
rebuilt from current retained live source versions and retained active memory
versions, using the existing tenant token and whitespace/lowercase normalization.
Temporal eligibility still chooses the newest begun memory revision before
checking whether its body survives; an absent purged revision never falls back
to an older indexed assertion. The verifier compares the complete rebuilt index
with that retained graph and rejects foreign/stale index rows.

## Integrity and remaining boundaries

`knowledge.sqlite` has a distinct extraction application ID, format version and
schema, including `extraction_owner`; it is not compatible with Store startup.
The manifest always says `cutover_ready:false`, `complete_account_export:false`,
`runtime_store_compatible:false` and `authenticated_lineage:false`.

Verification can run after deleting the synthetic source. It checks the archive
file/schema/row fingerprints, owner binding, scope accounting, foreign keys,
retained evidence, deletion invariants and rebuilt FTS using public pinned SQL.
This is source-free **archive inspection**, not restoring a running assistant.
Fingerprints are unkeyed integrity checks, not signatures or independently
trusted recovery evidence. DB journal heads are recorded as unauthenticated
source checkpoint metadata; no global journal or key file is copied or replayed.

The remaining 3.2 work includes authenticated frozen checkpoints, full-account
extraction, credential re-encryption/AAD proof, session/role handoff, independent
receiver-approved owner genesis, encrypted backups and original-free runtime
restore. This slice does not start a runtime, create a login, dispatch an effect,
release a usage allowance or open a vault.

Rehearsal bounds: source DB 512MiB, selected payload 128MiB, 100,000 rows per table,
1MiB per value and 512MiB archive file. Larger inputs fail explicitly. Stdout and
errors contain only public table labels, counts, fingerprints and reasons; owner
IDs and private content stay in the private artifact files.

Validation uses synthetic A/B rows only. The fixtures include retained source and
memory revisions, exact evidence, forgotten/deleted tombstones, a purged temporal
barrier and deliberately poisoned mixed-owner FTS. Tests cover byte-level absence
of B plaintext and password hashes, source immutability, CLI round-trip, inspection
without the source, partial-scope accounting, evidence/digest corruption, source
schema/owner mismatch, unsupported provenance, output preservation, symlinks,
hard links, permissions, sidecars and incomplete publication. Relevant inventory
regressions remain included in `cargo test --locked -p xtask account_runtime_`.
