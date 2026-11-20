-- Task receipts refer to member data. Source text is resolved again when opened.
CREATE TABLE member_workflows (
 id TEXT PRIMARY KEY NOT NULL, tenant_id TEXT NOT NULL, account_id TEXT NOT NULL,
 volume_id TEXT NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), auth_epoch INTEGER NOT NULL, host_epoch INTEGER NOT NULL,
 thread_id TEXT, message TEXT,
 title TEXT NOT NULL, scope TEXT NOT NULL CHECK(scope IN ('memory','conversation')), query TEXT NOT NULL,
 period INTEGER NOT NULL CHECK(period IN (0,1800,3600,86400,604800)), first_due INTEGER,
 next_due INTEGER, enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), revision INTEGER NOT NULL DEFAULT 1,
 removed INTEGER NOT NULL DEFAULT 0 CHECK(removed IN (0,1)),
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX member_work_due ON member_workflows(enabled,next_due) WHERE period>0;
CREATE TABLE member_work_runs (
 id TEXT PRIMARY KEY NOT NULL, workflow_id TEXT NOT NULL REFERENCES member_workflows(id) ON DELETE CASCADE,
 request_id TEXT, due_at INTEGER, created_at INTEGER NOT NULL, searched_at INTEGER NOT NULL, finished_at INTEGER NOT NULL, admitted_order INTEGER NOT NULL UNIQUE,
 sources TEXT NOT NULL CHECK(json_valid(sources)), limited INTEGER NOT NULL CHECK(limited IN (0,1)),
 UNIQUE(workflow_id,request_id), UNIQUE(workflow_id,due_at),
 CHECK((request_id IS NULL)!=(due_at IS NULL))
) STRICT;
ALTER TABLE personal_text_previews ADD COLUMN context_messages INTEGER;
ALTER TABLE personal_text_previews ADD COLUMN finished_at INTEGER;
CREATE TRIGGER personal_text_work_finished AFTER UPDATE OF state ON chat_runs
 WHEN NEW.state!=OLD.state AND NEW.state NOT IN ('running','queued') BEGIN
 UPDATE personal_text_previews SET finished_at=unixepoch() WHERE tenant_id=NEW.tenant_id AND run_id=NEW.id;
END;
ALTER TABLE personal_task_history ADD COLUMN page_host TEXT;
ALTER TABLE personal_task_history ADD COLUMN output_id TEXT;
-- Only the phase is copied. Mail content remains in the permission-checked Gmail view.
ALTER TABLE member_google_thread_reads ADD COLUMN work_id TEXT;
ALTER TABLE member_google_thread_reads ADD COLUMN work_order INTEGER;
CREATE TABLE member_inbox_task_events (
 id INTEGER PRIMARY KEY, tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, request_id TEXT NOT NULL,
 phase TEXT NOT NULL, at INTEGER NOT NULL,
 FOREIGN KEY(tenant_id,account_id,request_id) REFERENCES member_google_thread_reads(tenant_id,account_id,request_id) ON DELETE CASCADE
) STRICT;
CREATE INDEX member_inbox_task_history ON member_inbox_task_events(tenant_id,account_id,request_id,id);
CREATE TRIGGER member_inbox_task_started AFTER INSERT ON member_google_thread_reads BEGIN
 UPDATE chat_admission_counter SET value=value+1 WHERE singleton=1;
 UPDATE member_google_thread_reads SET work_order=(SELECT value FROM chat_admission_counter WHERE singleton=1),
 work_id=lower(hex(randomblob(4))||'-'||hex(randomblob(2))||'-4'||substr(hex(randomblob(2)),2)||'-8'||substr(hex(randomblob(2)),2)||'-'||hex(randomblob(6)))
 WHERE rowid=NEW.rowid;
 INSERT INTO member_inbox_task_events(tenant_id,account_id,request_id,phase,at) VALUES(NEW.tenant_id,NEW.account_id,NEW.request_id,NEW.phase,NEW.created_at);
END;
UPDATE member_google_thread_reads SET work_order=(SELECT value FROM chat_admission_counter WHERE singleton=1)+rowid,
 work_id=lower(hex(randomblob(4))||'-'||hex(randomblob(2))||'-4'||substr(hex(randomblob(2)),2)||'-8'||substr(hex(randomblob(2)),2)||'-'||hex(randomblob(6)));
UPDATE chat_admission_counter SET value=max(value,coalesce((SELECT max(work_order) FROM member_google_thread_reads),0)) WHERE singleton=1;
CREATE UNIQUE INDEX member_inbox_work_id ON member_google_thread_reads(work_id);
INSERT INTO member_inbox_task_events(tenant_id,account_id,request_id,phase,at)
 SELECT tenant_id,account_id,request_id,phase,updated_at FROM member_google_thread_reads;
CREATE TRIGGER member_inbox_task_changed AFTER UPDATE OF phase ON member_google_thread_reads WHEN NEW.phase!=OLD.phase BEGIN
 INSERT INTO member_inbox_task_events(tenant_id,account_id,request_id,phase,at) VALUES(NEW.tenant_id,NEW.account_id,NEW.request_id,NEW.phase,NEW.updated_at);
END;
PRAGMA user_version=91;
