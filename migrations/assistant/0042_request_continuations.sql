-- Explicit request/answer lineage, never general chat history authority.
CREATE TABLE request_continuations (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL,
 context TEXT CHECK(context IS NULL OR length(CAST(context AS BLOB))<=32768),
 pending INTEGER NOT NULL DEFAULT 0 CHECK(pending IN(0,1)),
 calendar_target TEXT,
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=42;
