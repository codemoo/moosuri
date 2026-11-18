-- Server OAuth continuations never choose a helper or a replacement mailbox.
CREATE TABLE gmail_server_resume_bindings (
 tenant_id TEXT NOT NULL,
 run_id TEXT NOT NULL,
 connection_id TEXT NOT NULL,
 connection_epoch INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,connection_id) REFERENCES mail_connections(tenant_id,id)
) STRICT;
PRAGMA user_version=82;
