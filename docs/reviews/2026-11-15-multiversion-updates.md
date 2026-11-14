# Signed stopped multi-version updates — selected acceptance

This bounded P10 extension changes only stopped schema admission. A fixed explicit
source floor47 replaces current-1 in MaintenanceGuard capture and signed-installer
forward recovery. Source47..CURRENT, same-schema maintenance and intermediate
recovery schemas are admitted; earlier unproved sources and future schemas reject.
No migration SQL/schema/dependency, runtime authority or service/network operation
is added. Official release signing/distribution and platform acceptance stay separate.

## Compatibility rationale

Store contains older migration SQL, but presence of SQL is not full signed-update
compatibility evidence. Current capture requires backup config/auth watermarks,
destructive successor binding and active cutover state (introduced14/15/19).
Staged export also unconditionally strips Push tables introduced23. Thus1 is not
a valid blind backup/update floor. This unit conservatively chooses47, the oldest
provisioned actual public binary fixture, rather than promising untested23..46.
The constant stays47 as current advances, so accepted older installations and
intermediate migration recovery do not become stranded on the next schema bump.

The prebuilt Linux predecessor evidence is actual public source026e315, schema47,
x86_64-unknown-linux-gnu, embedded web, binary size90923544 and SHA256
`15b709ff57a0ffb647e79ba0e0ae83125408d418eb4aead95018ba7cc43e883f`.
Its local tooling record is `/private/tmp/moosuri-execution-20261114/schema-predecessor-artifact.json`.
These are synthetic/fixture tool paths, not runtime recovery inputs or official
release-signing claims. The fixture creates its source data through that old CLI;
no current DB is relabeled with `PRAGMA user_version` as acceptance evidence.

## Retained invariants and exact selected gates

Signature/trust pin/native target, staged size/hash/probe, sequence/SemVer and the
old signed release's exact stopped-source schema checks remain unchanged. For
cross-schema updates/recovery, the current signed target updater's executing bytes
must match the staged signed target before export/migration/recovery. The
same stopped open-file lock spans capture, source-schema encrypted export, real
Store migration and activation. Pending publication, durable migration-start latch,
manager refresh/current activation and forward-only recovery order are unchanged.
Once migration starts old selection remains refused; an interrupted intermediate
schema resumes current product Store migrations without copying back an export.

No Cargo or real installation/data operation was run in this source-only checkout
because the shared native lane was allocated elsewhere. `rustfmt` and
`git diff --check` passed. Root owns these narrow joined Linux gates:

- `cargo test --locked -p moosuri-store signed_update_support_has_fixed_schema47_floor_and_rejects_unproved_or_future_sources`: range predicate checks fixed47, below-floor and future rejection, and every admitted intermediate schema.
- With feature `test-fixtures` and `MOOSURI_TEST_PREDECESSOR_BINARY` set to the actual47 binary, run ignored `signed_multiversion_update_interruption_retry_preserves_content_and_never_rolls_database_back`. Target must be at least49; integration with current50 retains the same fixed floor.
- Existing four affected interruption points are after migration-start, Store migration, manager refresh and current-pointer publication. The fixture checks busy writer rejection, signed target/schema/signature mismatch before export, pending/start fence, old refusal, forward retry, unchanged source export, manager target hash, original content and later committed work retention.

At this record's creation joined Linux and independent extension review are pending.
Earlier predecessor-only acceptance is not relabeled as passing this multi-version
extension. No broad unrelated suite was rerun; P10 aggregate release readiness is
not inferred from these selected gates.

No selected test injects failure inside an individual Store migration transaction.
Intermediate schema admission is covered by the shared range predicate and source
review, not yet a separately interrupted47→48 physical-state fixture. The four
existing process interruption cases cover pre-migration latch and post-migration
activation/retry boundaries.

## Joined acceptance on schema50

Root verified the exact10-file freeze and reopened the compatibility/export/installer
changes. Independent review found no material regression and confirmed that valid
unapplied journal tails are exported with complete/snapshot heads and replayed before
activation; the inherited stricter documentation was corrected without changing code.

Linux fixed-floor check passed. Actual signed schema47 source-binary→schema50 target
fixture passed in171.57s, including all four existing interruption/recovery cases,
signed mismatch and stopped writer exclusions, original and later committed content,
unchanged retained backup and no rollback to the old database. Clippy with test-fixtures
and the embedded default binary build passed. No passed unrelated suite was repeated.
No actual installation/legacy DB/service was changed; CI remains off.
