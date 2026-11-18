-- A resumed latest-INBOX read stays on the exact explicitly confirmed helper.
CREATE TABLE gmail_resume_bindings (
 tenant_id TEXT NOT NULL,
 run_id TEXT NOT NULL,
 fingerprint BLOB NOT NULL CHECK(length(fingerprint)=32),
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=81;
