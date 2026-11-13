-- Direct current-user reminder receipts; historical results do not grant authority.
CREATE TABLE schedule_chat_commands (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,schema_version INTEGER NOT NULL CHECK(schema_version=1),
 receipt TEXT NOT NULL CHECK(length(CAST(receipt AS BLOB))<=4096),
 PRIMARY KEY(tenant_id,run_id),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=22;
