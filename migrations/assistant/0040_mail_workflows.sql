-- Compose consent is explicit and one-use/session-bound with its OAuth flow.
ALTER TABLE mail_oauth_sessions ADD COLUMN compose INTEGER NOT NULL DEFAULT 0 CHECK(compose IN(0,1));
CREATE TABLE mail_schedules (
 tenant_id TEXT NOT NULL,action_id TEXT NOT NULL,
 due_at INTEGER NOT NULL,timezone TEXT NOT NULL,
 approved_revision INTEGER,auth_epoch INTEGER,
 PRIMARY KEY(tenant_id,action_id),
 FOREIGN KEY(tenant_id,action_id) REFERENCES mail_actions(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX mail_schedules_due ON mail_schedules(due_at);
PRAGMA user_version=40;
