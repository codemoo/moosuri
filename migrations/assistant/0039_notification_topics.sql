-- A stable source is an owned schedule, not a semantic/keyword grouping.
CREATE TABLE notification_source_preferences (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,schedule_id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),muted INTEGER NOT NULL CHECK(muted IN(0,1)),
 PRIMARY KEY(tenant_id,account_id,schedule_id),
 FOREIGN KEY(tenant_id,schedule_id) REFERENCES reminder_schedules(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX notification_sources_muted ON notification_source_preferences(tenant_id,account_id,schedule_id) WHERE muted=1;
CREATE INDEX local_notifications_source ON local_notifications(tenant_id,account_id,schedule_id);
ALTER TABLE local_notifications ADD COLUMN source_suppressed INTEGER NOT NULL DEFAULT 0 CHECK(source_suppressed IN(0,1));
ALTER TABLE local_notifications ADD COLUMN push_suppressed INTEGER NOT NULL DEFAULT 0 CHECK(push_suppressed IN(0,1));
CREATE TRIGGER notification_suppression_monotonic BEFORE UPDATE OF source_suppressed,push_suppressed ON local_notifications
WHEN NEW.source_suppressed!=OLD.source_suppressed OR NEW.push_suppressed<OLD.push_suppressed
BEGIN SELECT RAISE(ABORT,'notification suppression is permanent'); END;
PRAGMA user_version=39;
