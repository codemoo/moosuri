-- Metadata only. Browser profiles/frames/control plaintext never enter SQLite.
CREATE TABLE browser_workspaces (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, thread_id TEXT NOT NULL,
 id TEXT NOT NULL, auth_epoch INTEGER NOT NULL, deployment_pin TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('starting','ready','waiting_for_user','user_control','closed','expired','outcome_unknown')),
 revision INTEGER NOT NULL DEFAULT 1, human_touched INTEGER NOT NULL DEFAULT 0,
 url TEXT, title TEXT, reason TEXT,
 created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
 control_hash BLOB, control_session BLOB, control_until INTEGER,
 resume_request TEXT, resumed_request TEXT,
 PRIMARY KEY(tenant_id,thread_id), UNIQUE(tenant_id,id),
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=8;
