-- Private structural routes contain no training arguments, source IDs or text.
CREATE TABLE learned_skills (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,id TEXT NOT NULL,
 route TEXT NOT NULL,schema_version INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN('candidate','usable','invalidated','quarantined','removed')),
 enabled INTEGER NOT NULL DEFAULT 1 CHECK(enabled IN(0,1)),
 hidden INTEGER NOT NULL DEFAULT 0 CHECK(hidden IN(0,1)),
 failures INTEGER NOT NULL DEFAULT 0,revision INTEGER NOT NULL DEFAULT 1,
 PRIMARY KEY(tenant_id,account_id,id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TABLE learned_skill_proofs (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,skill_id TEXT NOT NULL,
 input_digest TEXT NOT NULL,run_id TEXT NOT NULL,receipt_digest TEXT NOT NULL,subject_digest TEXT,
 PRIMARY KEY(tenant_id,account_id,skill_id,input_digest),
 UNIQUE(tenant_id,account_id,skill_id,run_id),
 UNIQUE(tenant_id,account_id,skill_id,subject_digest),
 FOREIGN KEY(tenant_id,account_id,skill_id) REFERENCES learned_skills(tenant_id,account_id,id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
-- Content-free observation marker prevents a withdrawn finalized run from
-- being harvested again; proof hashes/arguments are not retained here.
CREATE TABLE learned_skill_observations (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE learned_skill_uses (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,skill_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id,skill_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE learned_skill_receipts (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,step INTEGER NOT NULL,call_id TEXT NOT NULL,
 ordinal INTEGER NOT NULL,input TEXT NOT NULL,result TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id,step,call_id,ordinal),
 FOREIGN KEY(tenant_id,run_id,step,call_id) REFERENCES agent_tool_slots(tenant_id,run_id,step,call_id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=38;
