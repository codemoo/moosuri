-- Nonsecret broker authority only. Existing ciphertext authority is unchanged.
CREATE TABLE credential_broker_pairs (
 nonce TEXT PRIMARY KEY, account_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
 session_digest BLOB NOT NULL, challenge TEXT NOT NULL, expires_at INTEGER NOT NULL,
 consumed INTEGER NOT NULL DEFAULT 0 CHECK(consumed IN(0,1))
) STRICT;
CREATE TABLE credential_broker_devices (
 id TEXT PRIMARY KEY, account_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
 session_digest BLOB NOT NULL, enrollment TEXT NOT NULL, public_key BLOB NOT NULL CHECK(length(public_key)=32),
 status TEXT NOT NULL CHECK(status IN('active','revocation_pending','revoked'))
) STRICT;
CREATE TABLE credential_broker_auth (
 nonce TEXT PRIMARY KEY, device_id TEXT NOT NULL REFERENCES credential_broker_devices(id),
 challenge TEXT NOT NULL, expires_at INTEGER NOT NULL
) STRICT;
CREATE TABLE credential_broker_bindings (
 device_id TEXT NOT NULL REFERENCES credential_broker_devices(id), binding_id TEXT NOT NULL,
 binding TEXT NOT NULL, active INTEGER NOT NULL CHECK(active IN(0,1)),
 PRIMARY KEY(device_id,binding_id)
) STRICT;
-- Only a typed fixture can mint these in this foundation. No production mint API.
CREATE TABLE credential_broker_invocations (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL, step INTEGER NOT NULL, call_id TEXT NOT NULL, invocation_nonce TEXT NOT NULL,
 request TEXT NOT NULL, cancelled INTEGER NOT NULL DEFAULT 0 CHECK(cancelled IN(0,1)),
 PRIMARY KEY(tenant_id,run_id,step,call_id,invocation_nonce)
) STRICT;
CREATE TABLE credential_broker_commitments (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL, step INTEGER NOT NULL, call_id TEXT NOT NULL, invocation_nonce TEXT NOT NULL,
 commitment TEXT NOT NULL, receipt TEXT,
 PRIMARY KEY(tenant_id,run_id,step,call_id,invocation_nonce)
) STRICT;
PRAGMA user_version=37;
