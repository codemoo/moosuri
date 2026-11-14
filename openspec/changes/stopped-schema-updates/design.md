# Design

The target bundle executable invokes update. Manifest signature/pin, native target, sequence/SemVer, staged hash/probe and target updater's executing bytes are verified before exporting or mutating private state. Only this build's schema and its immediate predecessor are supported. The old slot's signed schema must match the stopped source snapshot.

MaintenanceGuard captures either supported schema under the original exclusive writer lock. Its migration wrapper duplicates that same locked open description into Store::open_admitted; the original guard retains admission even if migration fails. Migration SQL remains solely in the product Store. Ordinary backup/cutover capture still requires exact current schema; predecessor export records the captured schema.

A durable pending record is published before migration; migration_started is durably true before Store opens. It conservatively forbids choose-old thereafter. Store transactions resume idempotently under stopped admission. Activation atomically refreshes manager and its managed digest, publishes current, then retires pending. Every interrupted window retains managed startup's pending fence. The same signed target bundle executable remains an explicit recovery entry point while an old installed manager cannot understand the new state.

Recovery never copies the source export back. It verifies signed old/new slots and current selection, validates schemas, retries forward migration and activation, preserving newer work/authority. Choosing old is supported for same-schema activation or an unstarted cross-schema migration whose database still has source schema. An unreadable or invalid data snapshot fails closed for manual investigation. No service/network action occurs.
