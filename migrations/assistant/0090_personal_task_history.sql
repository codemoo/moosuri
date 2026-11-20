-- Browser input, page contents, prompts and control capabilities stay out of history.
CREATE TABLE personal_task_history (
 id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL, run_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('starting','reading','ready','user_control','resuming','stopping','completed','closed','outcome_unknown')),
 at INTEGER,
 FOREIGN KEY(tenant_id,run_id) REFERENCES personal_browser_jobs(tenant_id,run_id) ON DELETE CASCADE
) STRICT;
CREATE INDEX personal_task_events ON personal_task_history(tenant_id,run_id,id);
CREATE INDEX personal_task_order ON chat_runs(tenant_id,account_id,admitted_order DESC);

-- Earlier versions kept only the latest browser state. Its exact time is unknown.
INSERT INTO personal_task_history(tenant_id,run_id,state,at)
 SELECT b.tenant_id,b.run_id,'starting',r.created_at
 FROM personal_browser_jobs b JOIN chat_runs r ON r.tenant_id=b.tenant_id AND r.id=b.run_id;
INSERT INTO personal_task_history(tenant_id,run_id,state,at)
 SELECT tenant_id,run_id,json_extract(view,'$.state'),NULL FROM personal_browser_jobs
 WHERE json_extract(view,'$.state')!='starting';
CREATE TRIGGER personal_task_started AFTER INSERT ON personal_browser_jobs BEGIN
 INSERT INTO personal_task_history(tenant_id,run_id,state,at)
 VALUES(NEW.tenant_id,NEW.run_id,'starting',unixepoch());
END;
CREATE TRIGGER personal_task_changed AFTER UPDATE OF view ON personal_browser_jobs
 WHEN json_extract(OLD.view,'$.state')!=json_extract(NEW.view,'$.state') BEGIN
 INSERT INTO personal_task_history(tenant_id,run_id,state,at)
 VALUES(NEW.tenant_id,NEW.run_id,json_extract(NEW.view,'$.state'),unixepoch());
END;
PRAGMA user_version=90;
