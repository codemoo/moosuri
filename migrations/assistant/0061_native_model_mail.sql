-- No credentials/provider IDs: consent descriptor and bounded typed request only.
CREATE TABLE credential_model_mail_grants (
 device_id TEXT NOT NULL,
 binding_id TEXT NOT NULL,
 background INTEGER NOT NULL CHECK(background IN (0,1)),
 PRIMARY KEY(device_id,binding_id),
 FOREIGN KEY(device_id,binding_id) REFERENCES credential_broker_bindings(device_id,binding_id) ON DELETE CASCADE
) STRICT;
CREATE TABLE credential_model_mail_payloads (
 tenant_id TEXT NOT NULL,
 account_id TEXT NOT NULL,
 request_id TEXT NOT NULL,
 device_id TEXT NOT NULL,
 binding_id TEXT NOT NULL,
 payload TEXT NOT NULL,
 expires_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,request_id),
 FOREIGN KEY(tenant_id,request_id) REFERENCES credential_broker_jobs(tenant_id,request_id) ON DELETE CASCADE,
 FOREIGN KEY(device_id,binding_id) REFERENCES credential_broker_bindings(device_id,binding_id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=61;
