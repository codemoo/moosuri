-- Exact typed request/status metadata only. No token, item ID or vault secret.
CREATE TABLE credential_broker_repositories (
 device_id TEXT NOT NULL REFERENCES credential_broker_devices(id), binding_id TEXT NOT NULL,
 repository TEXT NOT NULL, PRIMARY KEY(device_id,binding_id),
 FOREIGN KEY(device_id,binding_id) REFERENCES credential_broker_bindings(device_id,binding_id)
) STRICT;
CREATE TABLE credential_broker_jobs (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,request_id TEXT NOT NULL,
 run_id TEXT NOT NULL,call_id TEXT NOT NULL,invocation_nonce TEXT NOT NULL,
 repository TEXT NOT NULL,auth_epoch INTEGER NOT NULL,expires_at INTEGER NOT NULL,
 request TEXT,PRIMARY KEY(tenant_id,request_id),UNIQUE(tenant_id,run_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
PRAGMA user_version=44;
