-- No backfill. Only an explicit current artifact save admits a suggestion.
CREATE TABLE proactive_topics (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL,
 topic TEXT NOT NULL CHECK(topic='followup_review'), schedule_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,account_id,topic), UNIQUE(tenant_id,schedule_id),
 FOREIGN KEY(tenant_id,schedule_id) REFERENCES reminder_schedules(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE proactive_suggestions (
 tenant_id TEXT NOT NULL, artifact_id TEXT NOT NULL, artifact_revision INTEGER NOT NULL,
 schedule_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,artifact_id), UNIQUE(tenant_id,schedule_id),
 FOREIGN KEY(tenant_id,schedule_id) REFERENCES reminder_schedules(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,artifact_id,artifact_revision) REFERENCES personal_artifact_versions(tenant_id,artifact_id,revision)
) STRICT;
PRAGMA user_version=57;
