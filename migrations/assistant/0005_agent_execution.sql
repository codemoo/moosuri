ALTER TABLE provider_model_probes ADD COLUMN function_tools INTEGER NOT NULL DEFAULT 0 CHECK(function_tools IN(0,1));
ALTER TABLE threads ADD COLUMN chat_reset_floor INTEGER NOT NULL DEFAULT 0 CHECK(chat_reset_floor>=0);
ALTER TABLE model_call_receipts RENAME TO legacy_model_call_receipts;
CREATE TABLE model_call_receipts (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,run_id TEXT NOT NULL,attempt INTEGER NOT NULL,
 step INTEGER NOT NULL DEFAULT 0 CHECK(step>=0),nonce BLOB NOT NULL CHECK(length(nonce)=32),
 usage TEXT NOT NULL,dispatched_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,run_id,attempt,step),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
INSERT INTO model_call_receipts SELECT tenant_id,account_id,run_id,attempt,0,nonce,usage,dispatched_at FROM legacy_model_call_receipts;
DROP TABLE legacy_model_call_receipts;
CREATE TABLE agent_execution (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,agent INTEGER NOT NULL CHECK(agent IN(0,1)),
 started_ms INTEGER,deadline_ms INTEGER,last_clock_ms INTEGER,
 checkpoint_seq INTEGER NOT NULL DEFAULT 0 CHECK(checkpoint_seq>=0),
 remote_calls INTEGER NOT NULL DEFAULT 0 CHECK(remote_calls BETWEEN 0 AND 6),
 tool_calls INTEGER NOT NULL DEFAULT 0 CHECK(tool_calls BETWEEN 0 AND 12),
 generation INTEGER,invalidated INTEGER NOT NULL DEFAULT 0 CHECK(invalidated IN(0,1)),
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE agent_run_leases (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,attempt INTEGER NOT NULL,
 nonce BLOB NOT NULL CHECK(length(nonce)=32),
 PRIMARY KEY(tenant_id,run_id,attempt),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE agent_steps (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,step INTEGER NOT NULL CHECK(step BETWEEN 1 AND 6),
 attempt INTEGER NOT NULL,nonce BLOB NOT NULL CHECK(length(nonce)=32),
 state TEXT NOT NULL CHECK(state IN('dispatched','completed','unknown','purged')),
 completion_digest BLOB CHECK(completion_digest IS NULL OR length(completion_digest)=32),
 checkpoint BLOB CHECK(checkpoint IS NULL OR length(checkpoint)<=262144),
 PRIMARY KEY(tenant_id,run_id,step),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE agent_tool_slots (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,step INTEGER NOT NULL,call_id TEXT NOT NULL,
 input TEXT, result TEXT CHECK(result IS NULL OR length(CAST(result AS BLOB))<=16384),
 PRIMARY KEY(tenant_id,run_id,step,call_id),
 FOREIGN KEY(tenant_id,run_id,step) REFERENCES agent_steps(tenant_id,run_id,step) ON DELETE CASCADE
) STRICT;
-- Revision-specific, content-free provenance. The union contains only delivered context.
CREATE TABLE agent_dependencies (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,kind TEXT NOT NULL CHECK(kind IN('source','memory')),
 object_id TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>0),
 PRIMARY KEY(tenant_id,run_id,kind,object_id,revision),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX agent_dependency_object ON agent_dependencies(tenant_id,kind,object_id,revision);
PRAGMA user_version=5;
