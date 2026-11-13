-- Separate ciphertext-service authority; assistant schema24.
CREATE TABLE vault_handoffs (
 digest BLOB PRIMARY KEY NOT NULL CHECK(length(digest)=32),
 session_digest BLOB NOT NULL, account_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
 auth_epoch INTEGER NOT NULL, installation_id TEXT NOT NULL, vault_origin TEXT NOT NULL,
 expires_at INTEGER NOT NULL, consumed INTEGER NOT NULL DEFAULT 0 CHECK(consumed IN(0,1))
) STRICT;
CREATE TABLE vault_grants (
 digest BLOB PRIMARY KEY NOT NULL CHECK(length(digest)=32),
 session_digest BLOB NOT NULL, account_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
 auth_epoch INTEGER NOT NULL, installation_id TEXT NOT NULL, expires_at INTEGER NOT NULL,
 closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN(0,1))
) STRICT;
CREATE TABLE vault_operation_commitments (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, installation_id TEXT NOT NULL,
 operation_id TEXT NOT NULL, payload_digest TEXT NOT NULL CHECK(length(payload_digest)=64),
 auth_epoch INTEGER NOT NULL, committed_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,operation_id)
) STRICT;
PRAGMA user_version=24;
