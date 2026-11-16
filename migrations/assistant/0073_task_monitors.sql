CREATE TABLE task_monitor_policies (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, id TEXT NOT NULL,
 scope_digest BLOB NOT NULL, sources TEXT NOT NULL, check_template TEXT NOT NULL,
 auth_epoch INTEGER NOT NULL, cadence_seconds INTEGER NOT NULL,
 expires_at INTEGER NOT NULL, reusable INTEGER NOT NULL CHECK(reusable IN(0,1)),
 state TEXT NOT NULL CHECK(state IN('active','revoked')),
 request_id TEXT NOT NULL, request_digest BLOB NOT NULL,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,account_id,request_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE task_monitors (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, id TEXT NOT NULL,
 parent_mission_id TEXT NOT NULL, acceptance_revision INTEGER NOT NULL,
 sources TEXT NOT NULL, scope_digest BLOB NOT NULL, model TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('proposed','active','stopped','expired','withdrawn')),
 revision INTEGER NOT NULL DEFAULT 1, policy_id TEXT, schedule_id TEXT,
 last_fingerprint BLOB NOT NULL, last_members TEXT NOT NULL, reason TEXT, local_note TEXT NOT NULL DEFAULT '',
 created_at INTEGER NOT NULL, last_checked_at INTEGER,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,parent_mission_id,acceptance_revision),
 UNIQUE(tenant_id,schedule_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE task_monitor_observations (
 tenant_id TEXT NOT NULL, monitor_id TEXT NOT NULL, ordinal INTEGER NOT NULL,
 fingerprint BLOB NOT NULL, mission_id TEXT NOT NULL, created_at INTEGER NOT NULL,
 resolved_at INTEGER, resolution_note TEXT,
 PRIMARY KEY(tenant_id,monitor_id,ordinal), UNIQUE(tenant_id,mission_id),
 FOREIGN KEY(tenant_id,monitor_id) REFERENCES task_monitors(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE UNIQUE INDEX task_monitor_active_scope ON task_monitors(tenant_id,account_id,scope_digest) WHERE state='active';
CREATE TABLE task_monitor_links (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, parent_mission_id TEXT NOT NULL,
 acceptance_revision INTEGER NOT NULL, monitor_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,parent_mission_id,acceptance_revision),
 FOREIGN KEY(tenant_id,monitor_id) REFERENCES task_monitors(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX task_monitor_thread_members ON source_provenance(tenant_id,connection_id,thread_id,source_id,source_revision);
PRAGMA user_version=73;
