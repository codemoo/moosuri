-- Private current-owner guidance. No row grants another tool or effect.
CREATE TABLE run_instruction_channels (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,attempt INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,run_id,attempt),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE run_instructions (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,request_id TEXT NOT NULL,
 attempt INTEGER NOT NULL,execution_attempt INTEGER NOT NULL,sequence INTEGER NOT NULL CHECK(sequence BETWEEN 1 AND 8),
 text TEXT NOT NULL CHECK(length(CAST(text AS BLOB)) BETWEEN 1 AND 2000),
 state TEXT NOT NULL CHECK(state IN('received','applied','closed')),
 reason TEXT,session_digest BLOB NOT NULL CHECK(length(session_digest)=32),
 auth_epoch INTEGER NOT NULL,dispatch_step INTEGER,
 PRIMARY KEY(tenant_id,run_id,request_id),UNIQUE(tenant_id,run_id,sequence),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=72;
