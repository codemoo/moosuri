-- Current-user semantic proposal; neither classifier output nor historical receipt grants execution.
CREATE TABLE mission_chat_plans (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,anchor INTEGER NOT NULL,timezone TEXT,
 original_session BLOB NOT NULL CHECK(length(original_session)=32),original_epoch INTEGER NOT NULL CHECK(original_epoch>=0),
 target TEXT,decision TEXT CHECK(decision IS NULL OR length(CAST(decision AS BLOB))<=16384),
 state TEXT NOT NULL DEFAULT 'awaiting' CHECK(state IN('awaiting','decided','passed','applied','quarantined')),
 PRIMARY KEY(tenant_id,run_id), FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE mission_chat_commands (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,receipt TEXT NOT NULL CHECK(length(CAST(receipt AS BLOB))<=4096),
 PRIMARY KEY(tenant_id,run_id), FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=29;
