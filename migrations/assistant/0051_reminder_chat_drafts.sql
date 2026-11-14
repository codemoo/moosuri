-- Bounded current-user fixed-reminder clarification state; never an execution grant.
CREATE TABLE schedule_chat_drafts (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,
 draft TEXT NOT NULL CHECK(length(CAST(draft AS BLOB))<=8192),
 pending INTEGER NOT NULL CHECK(pending IN(0,1)),
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=51;
