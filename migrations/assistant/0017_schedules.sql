-- Reserved18 after root artifacts16/checkpoint17; standalone schema15 uses16.
CREATE TABLE reminder_schedules (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,account_id TEXT NOT NULL,
 request_id TEXT NOT NULL,request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 revision INTEGER NOT NULL CHECK(revision>0),auth_epoch INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN('active','paused','revoked','completed','expired')),
 reason TEXT,title TEXT NOT NULL CHECK(length(CAST(title AS BLOB)) BETWEEN 1 AND 160),
 message TEXT NOT NULL CHECK(length(CAST(message AS BLOB)) BETWEEN 1 AND 2000),
 rule TEXT NOT NULL CHECK(length(CAST(rule AS BLOB))<=1024),valid_until INTEGER,
 timezone_db TEXT NOT NULL,next_slot TEXT,next_due INTEGER,
 created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,account_id,request_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX reminder_due ON reminder_schedules(next_due,tenant_id,id) WHERE state='active';
CREATE TABLE reminder_occurrences (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,schedule_id TEXT NOT NULL,account_id TEXT NOT NULL,
 revision INTEGER NOT NULL,auth_epoch INTEGER NOT NULL,scheduled_for INTEGER,local_slot TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('queued','claimed','delivered','skipped','expired')),
 reason TEXT,nonce BLOB,deadline INTEGER,notification_id TEXT,delayed INTEGER NOT NULL CHECK(delayed IN(0,1)),
 title TEXT NOT NULL,message TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,schedule_id,local_slot),
 FOREIGN KEY(tenant_id,schedule_id) REFERENCES reminder_schedules(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE UNIQUE INDEX reminder_occurrence_utc ON reminder_occurrences(tenant_id,schedule_id,scheduled_for) WHERE scheduled_for IS NOT NULL;
CREATE UNIQUE INDEX reminder_one_queued ON reminder_occurrences(tenant_id,schedule_id) WHERE state='queued';
CREATE UNIQUE INDEX reminder_one_claimed ON reminder_occurrences(tenant_id,schedule_id) WHERE state='claimed';
CREATE TABLE reminder_skipped_ranges (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,schedule_id TEXT NOT NULL,revision INTEGER NOT NULL,
 from_slot TEXT NOT NULL,to_slot TEXT NOT NULL,count INTEGER NOT NULL CHECK(count>0),
 reason TEXT NOT NULL,created_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id),
 FOREIGN KEY(tenant_id,schedule_id) REFERENCES reminder_schedules(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE reminder_owners (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,last_claim INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(tenant_id,account_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE reminder_clock(singleton INTEGER PRIMARY KEY CHECK(singleton=1),claim_order INTEGER NOT NULL);
INSERT INTO reminder_clock VALUES(1,0);
CREATE TABLE notification_settings (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,revision INTEGER NOT NULL,
 enabled INTEGER NOT NULL CHECK(enabled IN(0,1)),timezone TEXT NOT NULL,
 start_minute INTEGER NOT NULL CHECK(start_minute BETWEEN 0 AND 1439),
 end_minute INTEGER NOT NULL CHECK(end_minute BETWEEN 0 AND 1439),
 PRIMARY KEY(tenant_id,account_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE local_notifications (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,account_id TEXT NOT NULL,
 schedule_id TEXT NOT NULL,occurrence_id TEXT NOT NULL,title TEXT NOT NULL,message TEXT NOT NULL,
 scheduled_for INTEGER NOT NULL,delivered_at INTEGER NOT NULL,
 delayed INTEGER NOT NULL CHECK(delayed IN(0,1)),quiet INTEGER NOT NULL CHECK(quiet IN(0,1)),
 quiet_revision INTEGER NOT NULL,is_read INTEGER NOT NULL DEFAULT 0 CHECK(is_read IN(0,1)),
 dismissed INTEGER NOT NULL DEFAULT 0 CHECK(dismissed IN(0,1)),
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,occurrence_id),
 FOREIGN KEY(tenant_id,occurrence_id) REFERENCES reminder_occurrences(tenant_id,id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX local_notifications_owner ON local_notifications(tenant_id,account_id,delivered_at DESC,id DESC);
PRAGMA user_version=17;
