# Shared assistant schema registry — 2026-11-16

## Scope

Normal startup and encrypted cutover restore now select SQL from one ordered
assistant registry. Current schema remains73. Migration files are byte-identical;
no `IF NOT EXISTS` or other SQL weakening was introduced. The registry validates
contiguous versions and explicit caller support floors before returning SQL.

Startup keeps its schema4 journal barrier, schema15 successor-journal/metadata
handoff, per-migration immediate transactions, schema65/66 provider-save recovery
boundary and schema70 FK-off transaction. Restore keeps sealed schema19 admission,
its existing quarantine transaction, pre-BEGIN FK handling, trusted_schema=OFF,
current-version/integrity/FK checks, journal lineage and staged publication.
Schema15 SQL and restored schema19 guards also resolve through the registry.

## Evidence

On the isolated schema-registry checkout based on `851f43d`:

- `cargo test --locked -p moosuri-store registry_ -- --nocapture`:4 passed.
  Directory coverage compares SQL bytes and versions independently of the registry;
  invalid cases cover missing middle/tail, duplicate/out-of-order and unsupported
  ranges. SQL sweeps cover every startup prefix0..73 and restore prefix19..73;
  restore verifies the caller transaction remains open and rollback retains the
  original version. Startup asserts FK enforcement and trusted-schema policy.
- Focused schema15 handoff guard:1 passed. The generic startup runner refuses to
  execute15, preserving ownership by the durable journal initializer.
- Existing focused restore checks:4 passed: actual-current encrypted archive with
  artifacts/media/quarantine/suspended auth; schema19 encrypted restore and repeated
  journal lineage; schema55 encrypted restore with revoked native mail authority;
  schema71 upgrade with empty scoped authority and enforced owner foreign keys.
- `cargo clippy --locked --no-deps -p moosuri-store --all-targets -- -D warnings`:
  passed. Existing ts-rs serde-attribute notices remain non-failing.
- `cargo fmt --all` and `git diff --check`: passed.
- Pinned OpenSpec2.0.0 strict validation of this change: passed. Installation used
  an isolated temporary npm cache after the default cache proved unwritable.

All fixtures are synthetic. SQL sweeps prove structural coverage and transaction
rollback; they do not replace actual historic binaries or real operational
restore acceptance. Historical journal replay retains its separate schema20
floor. Independent frozen-diff review found no material regression; integrated
Store/reminder acceptance also passed. The stale reverse-drop test and its
replacement coverage are documented in the [integrated review](2026-11-16-everyday-assistant.md).
CI remains disabled and no live data/services were accessed.

See [change package](../../openspec/changes/shared-assistant-schema-registry/).
