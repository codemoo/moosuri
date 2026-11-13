-- New chat-only grants. Historical profiles and mission templates are not upgraded.
CREATE TABLE calendar_agent_profiles (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,account_id TEXT NOT NULL,version INTEGER NOT NULL CHECK(version=1),
 profile TEXT NOT NULL CHECK(length(CAST(profile AS BLOB))<=16384),
 PRIMARY KEY(tenant_id,run_id),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE calendar_run_dependencies (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,connection_id TEXT NOT NULL,epoch INTEGER NOT NULL,app_revision INTEGER,client_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id,connection_id),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE calendar_agent_observations (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,handle TEXT NOT NULL,connection_id TEXT NOT NULL,epoch INTEGER NOT NULL,
 step INTEGER NOT NULL,call_id TEXT NOT NULL,request TEXT NOT NULL,event_id TEXT,etag TEXT,observed_at INTEGER NOT NULL,digest TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id,handle),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE calendar_command_plans (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,action_id TEXT NOT NULL,decision TEXT NOT NULL CHECK(length(CAST(decision AS BLOB))<=16384),
 state TEXT NOT NULL CHECK(state IN('sealed','committed','finished')),expires_at INTEGER NOT NULL,receipt TEXT,step INTEGER NOT NULL CHECK(step=1),
 PRIMARY KEY(tenant_id,run_id),UNIQUE(tenant_id,action_id),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE calendar_agent_action_links (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,action_id TEXT NOT NULL,step INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,run_id),UNIQUE(tenant_id,action_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,action_id) REFERENCES calendar_actions(tenant_id,id)
) STRICT;
-- Minimum origin tombstone survives conversation deletion; it cannot authorize reads/writes.
CREATE TABLE calendar_agent_action_origins (
 tenant_id TEXT NOT NULL,action_id TEXT NOT NULL,run_id TEXT NOT NULL,
 withdrawn INTEGER NOT NULL DEFAULT 0 CHECK(withdrawn IN(0,1)),
 PRIMARY KEY(tenant_id,action_id),FOREIGN KEY(tenant_id,action_id) REFERENCES calendar_actions(tenant_id,id)
) STRICT;
CREATE TRIGGER calendar_action_origin_withdrawal_no_reset
BEFORE UPDATE OF withdrawn ON calendar_agent_action_origins
WHEN OLD.withdrawn=1 AND NEW.withdrawn!=1
BEGIN SELECT RAISE(ABORT,'Calendar withdrawal is irreversible'); END;
PRAGMA user_version=30;
