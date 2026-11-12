-- Slot/receipt identity survives deletion of the conversation and its private plan.
-- A destructive journal action can therefore resolve a committed opaque intent
-- without retaining user text or recreating a deleted run.
CREATE TABLE memory_command_slots (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,slot INTEGER NOT NULL DEFAULT 0 CHECK(slot=0),
 command_id TEXT NOT NULL,nonce BLOB NOT NULL CHECK(length(nonce)=32),
 account_id TEXT NOT NULL,intent_digest BLOB NOT NULL CHECK(length(intent_digest)=32),
 action TEXT NOT NULL CHECK(action IN('remember','correct','forget')),
 target_id TEXT,target_revision INTEGER,target_effective_revision INTEGER,clarification TEXT,
 proof_attempt INTEGER NOT NULL,proof_step INTEGER NOT NULL,
 bound_attempt INTEGER NOT NULL,bound_nonce BLOB NOT NULL CHECK(length(bound_nonce)=32),
 session_digest BLOB NOT NULL CHECK(length(session_digest)=32),auth_epoch INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN('prepared','committed')),outcome TEXT,
 PRIMARY KEY(tenant_id,run_id,slot),UNIQUE(tenant_id,command_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id),
 CHECK((state='prepared' AND outcome IS NULL) OR (state='committed' AND outcome IS NOT NULL))
) STRICT;
CREATE TABLE memory_command_plans (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,intent TEXT NOT NULL CHECK(length(CAST(intent AS BLOB))<=8192),
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=6;
