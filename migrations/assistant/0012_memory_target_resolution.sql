-- Private current-intent authority and complete, bounded data-only candidate set.
-- Removed with the run or knowledge-context purge; never journal authority itself.
ALTER TABLE agent_execution ADD COLUMN memory_target_started INTEGER NOT NULL DEFAULT 0 CHECK(memory_target_started IN(0,1));
CREATE TABLE memory_target_plans (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,
 nonce BLOB NOT NULL CHECK(length(nonce)=32),
 intent TEXT NOT NULL CHECK(length(CAST(intent AS BLOB))<=8192),
 intent_digest BLOB NOT NULL CHECK(length(intent_digest)=32),
 generation INTEGER NOT NULL,planning_step INTEGER NOT NULL,
 snapshot TEXT NOT NULL CHECK(length(CAST(snapshot AS BLOB))<=65536),
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=12;
