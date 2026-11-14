# Design

The target bundle executable invokes update. Manifest signature/pin, native target, sequence/SemVer, staged hash/probe and target updater's executing bytes are verified before exporting or mutating private state. The explicit supported range is fixed schema47 through this build's current
schema; backup's shared compatibility API is the admission authority. Schema47 is
the oldest actual-binary fixture selected for this bounded path, while structural
export prerequisites beginning at23 do not establish older signed-update support. The old slot's signed schema must match the stopped source snapshot.

MaintenanceGuard captures either supported schema under the original exclusive writer lock. Its migration wrapper duplicates that same locked open description into Store::open_admitted; the original guard retains admission even if migration fails. Migration SQL remains solely in the product Store. Ordinary backup/cutover capture still requires exact current schema; source export records the captured schema, including multi-version gaps.

A durable pending record is published before migration; migration_started is durably true before Store opens. It conservatively forbids choose-old thereafter. Store transactions resume idempotently under stopped admission. Activation atomically refreshes manager and its managed digest, publishes current, then retires pending. Every interrupted window retains managed startup's pending fence. The same signed target bundle executable remains an explicit recovery entry point while an old installed manager cannot understand the new state.

Recovery never copies the source export back. It verifies signed old/new slots and current selection, validates schemas, retries forward migration and activation, preserving newer work/authority. Choosing old is supported for same-schema activation or an unstarted cross-schema migration whose database still has source schema. An unreadable or invalid data snapshot fails closed for manual investigation. No service/network action occurs.


Multi-version recovery accepts a captured intermediate schema only between the
recorded signed source and target, with a source inside the fixed supported range.
The current target binary hash pin is still required before the migration-start
latch. No schema/dependency, migration SQL or source-data rewriting is added by
this extension. The actual47 executable must create its own accounts/content and
backup configuration; relabeling a current fixture as47 is not acceptance evidence.
