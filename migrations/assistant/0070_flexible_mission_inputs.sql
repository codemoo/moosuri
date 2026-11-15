-- Caller disables FK enforcement outside the transaction, then reenables and
-- checks all relationships before admitting the upgraded/restored Store.
-- Receipt-bound choices and context remain within the original acceptance.
CREATE TABLE mission_input_waits_new (
 tenant_id TEXT NOT NULL,mission_id TEXT NOT NULL,run_id TEXT NOT NULL,
 request_id TEXT NOT NULL,acceptance_revision INTEGER NOT NULL,mission_revision INTEGER NOT NULL,
 step INTEGER NOT NULL,field TEXT NOT NULL CHECK(field IN('mail_to','calendar_date','calendar_start','calendar_duration','context','source_choice','calendar_target')),
 question TEXT NOT NULL,value TEXT,status TEXT NOT NULL CHECK(status IN('waiting','ready','resumed','expired','cancelled')),
 expires_ms INTEGER NOT NULL,candidates TEXT NOT NULL DEFAULT '[]',notification_id TEXT,
 PRIMARY KEY(tenant_id,request_id),UNIQUE(tenant_id,run_id,acceptance_revision,field),
 FOREIGN KEY(tenant_id,mission_id) REFERENCES missions(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
INSERT INTO mission_input_waits_new SELECT *, '[]', NULL FROM mission_input_waits;
DROP TABLE mission_input_waits;
ALTER TABLE mission_input_waits_new RENAME TO mission_input_waits;
CREATE INDEX mission_input_ready ON mission_input_waits(status);
-- NULL occurrence identifies a direct action-needed notice, with schedule_id
-- containing the actual mission identity. No schedule/execution authority exists.
CREATE TABLE local_notifications_new (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,account_id TEXT NOT NULL,
 schedule_id TEXT NOT NULL,occurrence_id TEXT,title TEXT NOT NULL,message TEXT NOT NULL,
 scheduled_for INTEGER NOT NULL,delivered_at INTEGER NOT NULL,
 delayed INTEGER NOT NULL CHECK(delayed IN(0,1)),quiet INTEGER NOT NULL CHECK(quiet IN(0,1)),
 quiet_revision INTEGER NOT NULL,is_read INTEGER NOT NULL DEFAULT 0 CHECK(is_read IN(0,1)),
 dismissed INTEGER NOT NULL DEFAULT 0 CHECK(dismissed IN(0,1)),
 source_suppressed INTEGER NOT NULL DEFAULT 0 CHECK(source_suppressed IN(0,1)),
 push_suppressed INTEGER NOT NULL DEFAULT 0 CHECK(push_suppressed IN(0,1)),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,occurrence_id),
 FOREIGN KEY(tenant_id,occurrence_id) REFERENCES reminder_occurrences(tenant_id,id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
INSERT INTO local_notifications_new(rowid,tenant_id,id,account_id,schedule_id,occurrence_id,title,message,scheduled_for,delivered_at,delayed,quiet,quiet_revision,is_read,dismissed,source_suppressed,push_suppressed)
 SELECT rowid,* FROM local_notifications;
DROP TABLE local_notifications;
ALTER TABLE local_notifications_new RENAME TO local_notifications;
CREATE INDEX local_notifications_owner ON local_notifications(tenant_id,account_id,delivered_at DESC,id DESC);
CREATE INDEX local_notifications_source ON local_notifications(tenant_id,account_id,schedule_id);
CREATE TRIGGER notification_suppression_monotonic BEFORE UPDATE OF source_suppressed,push_suppressed ON local_notifications
WHEN NEW.source_suppressed!=OLD.source_suppressed OR NEW.push_suppressed<OLD.push_suppressed
BEGIN SELECT RAISE(ABORT,'notification suppression is permanent'); END;
CREATE TABLE notification_source_preferences_new (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,schedule_id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),muted INTEGER NOT NULL CHECK(muted IN(0,1)),
 PRIMARY KEY(tenant_id,account_id,schedule_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
INSERT INTO notification_source_preferences_new SELECT * FROM notification_source_preferences;
DROP TABLE notification_source_preferences;
ALTER TABLE notification_source_preferences_new RENAME TO notification_source_preferences;
CREATE INDEX notification_sources_muted ON notification_source_preferences(tenant_id,account_id,schedule_id) WHERE muted=1;
PRAGMA user_version=70;
