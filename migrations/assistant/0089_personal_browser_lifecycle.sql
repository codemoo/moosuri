-- This row survives deletion of a conversation solely until its worker has stopped.
-- It contains no page, input, controller token, or browser profile data.
CREATE TABLE personal_browser_workers (
 tenant_id TEXT NOT NULL, volume_id TEXT NOT NULL, generation INTEGER NOT NULL,
 workspace_id TEXT NOT NULL PRIMARY KEY, run_id TEXT NOT NULL,
 session_digest BLOB NOT NULL CHECK(length(session_digest)=32),
 expires_at INTEGER NOT NULL, stopping INTEGER NOT NULL DEFAULT 0 CHECK(stopping IN(0,1))
) STRICT;
INSERT INTO personal_browser_workers
 SELECT b.tenant_id,b.volume_id,b.generation,json_extract(b.view,'$.id'),b.run_id,
 coalesce(b.controller_session,r.session_digest),r.created_at+28800,0
 FROM personal_browser_jobs b JOIN chat_runs r ON r.tenant_id=b.tenant_id AND r.id=b.run_id
 WHERE json_extract(b.view,'$.state')!='closed';
CREATE TRIGGER personal_browser_deleted AFTER DELETE ON personal_browser_jobs BEGIN
 UPDATE personal_browser_workers SET stopping=1 WHERE tenant_id=OLD.tenant_id AND run_id=OLD.run_id;
END;
CREATE TRIGGER personal_browser_logged_out AFTER DELETE ON sessions BEGIN
 UPDATE personal_browser_workers SET stopping=1 WHERE session_digest=OLD.digest;
END;
PRAGMA user_version=89;
