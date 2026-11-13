-- Explicit recurring mission templates reuse the proven schedule occurrence oracle.
CREATE TABLE recurring_missions (
 tenant_id TEXT NOT NULL,schedule_id TEXT NOT NULL,account_id TEXT NOT NULL,
 request_id TEXT NOT NULL,payload_digest BLOB NOT NULL,template TEXT NOT NULL,model TEXT NOT NULL,
 changes_only INTEGER NOT NULL CHECK(changes_only IN(0,1)),
 PRIMARY KEY(tenant_id,schedule_id),UNIQUE(tenant_id,account_id,request_id),
 FOREIGN KEY(tenant_id,schedule_id) REFERENCES reminder_schedules(tenant_id,id)
) STRICT;
CREATE TABLE recurring_mission_occurrences (
 tenant_id TEXT NOT NULL,occurrence_id TEXT NOT NULL,schedule_id TEXT NOT NULL,
 schedule_revision INTEGER NOT NULL,mission_id TEXT NOT NULL,scheduled_for INTEGER NOT NULL,
 notification_id TEXT,publication_state TEXT NOT NULL DEFAULT 'pending' CHECK(publication_state IN('pending','published','exact_unchanged','fenced')),
 PRIMARY KEY(tenant_id,occurrence_id),UNIQUE(tenant_id,mission_id),
 FOREIGN KEY(tenant_id,mission_id) REFERENCES missions(tenant_id,id),
 FOREIGN KEY(tenant_id,occurrence_id) REFERENCES reminder_occurrences(tenant_id,id)
) STRICT;
PRAGMA user_version=28;
