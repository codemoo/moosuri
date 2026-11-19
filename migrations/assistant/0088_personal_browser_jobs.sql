CREATE TABLE personal_browser_jobs (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL, thread_id TEXT NOT NULL,
 volume_id TEXT NOT NULL, generation INTEGER NOT NULL, view TEXT NOT NULL,
 human_touched INTEGER NOT NULL DEFAULT 0, controller_digest BLOB, controller_session BLOB,
 controller_until INTEGER, pending TEXT, remaining_reads INTEGER NOT NULL DEFAULT 6,
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX personal_browser_thread ON personal_browser_jobs(tenant_id,thread_id);
CREATE TABLE personal_browser_actions (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL, request_id TEXT NOT NULL, operation TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id,request_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES personal_browser_jobs(tenant_id,run_id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=88;
