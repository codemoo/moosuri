-- No history backfill: global counters/checkpoints cannot prove planner provenance.
-- Calendar decisions may follow a completed non-command classifier. Preserve
-- historical rows exactly; they do not acquire a new planner dispatch marker.
CREATE TABLE calendar_command_plans_next (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,action_id TEXT NOT NULL,decision TEXT NOT NULL CHECK(length(CAST(decision AS BLOB))<=16384),
 state TEXT NOT NULL CHECK(state IN('sealed','committed','finished')),expires_at INTEGER NOT NULL,receipt TEXT,step INTEGER NOT NULL CHECK(step BETWEEN 1 AND 6),
 PRIMARY KEY(tenant_id,run_id),UNIQUE(tenant_id,action_id),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
INSERT INTO calendar_command_plans_next(tenant_id,run_id,action_id,decision,state,expires_at,receipt,step)
 SELECT tenant_id,run_id,action_id,decision,state,expires_at,receipt,step FROM calendar_command_plans;
DROP TABLE calendar_command_plans;
ALTER TABLE calendar_command_plans_next RENAME TO calendar_command_plans;

CREATE TABLE agent_planner_steps (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL,
 domain TEXT NOT NULL CHECK(domain IN('artifact_no_command','mission_chat')),
 step INTEGER NOT NULL CHECK(step>0), attempt INTEGER NOT NULL,
 nonce BLOB NOT NULL CHECK(length(nonce)=32),
 run_nonce BLOB NOT NULL CHECK(length(run_nonce)=32),
 PRIMARY KEY(tenant_id,run_id,domain), UNIQUE(tenant_id,run_id,step),
 FOREIGN KEY(tenant_id,run_id,step) REFERENCES agent_steps(tenant_id,run_id,step) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=58;
