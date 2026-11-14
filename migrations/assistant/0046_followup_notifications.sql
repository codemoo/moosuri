-- Only explicit artifact saves create schedules; historical dates are not backfilled.
CREATE TABLE followup_reminders (
 tenant_id TEXT NOT NULL, schedule_id TEXT NOT NULL, source_schedule_id TEXT NOT NULL,
 artifact_id TEXT NOT NULL, artifact_revision INTEGER NOT NULL,
 date_kind TEXT NOT NULL CHECK(date_kind IN('due','review','due_review','snooze')),
 date_utc INTEGER NOT NULL, date_timezone TEXT NOT NULL, scheduled_for INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,schedule_id),
 FOREIGN KEY(tenant_id,schedule_id) REFERENCES reminder_schedules(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,source_schedule_id) REFERENCES reminder_schedules(tenant_id,id),
 FOREIGN KEY(tenant_id,artifact_id,artifact_revision) REFERENCES personal_artifact_versions(tenant_id,artifact_id,revision)
) STRICT;
CREATE INDEX followup_artifact ON followup_reminders(tenant_id,artifact_id,artifact_revision);
CREATE INDEX followup_source ON followup_reminders(tenant_id,source_schedule_id);
PRAGMA user_version=46;
