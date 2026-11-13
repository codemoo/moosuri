# Change: Standalone vault sealed-current recovery

## Why
F50 needs separate ciphertext service recovery without decrypting a user vault or replaying stale sessions, pending operations, deleted items or nonce reservations.

## What Changes
Add owner-only stopped sealed-current checkpoint and verified new-root restore CLI/API, existing age outer encryption, exact journal/SQLite identity and semantic storage replay, durable source retirement, authority quarantine, fresh service identity and immutable provenance. Preserve current service invocation and current cryptographic wire formats.

## Impact
Only vault-service source/dependency feature and targeted tests/docs change. No assistant schema, live-provider effects, historical/off-host freshness, distributed singleton guarantee, old-v1 migration, broker or account deletion completion. See [contract](../../../docs/vault-service-recovery.md).
