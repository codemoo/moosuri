-- Member-owned change-only watches and daily local briefs.
-- Every monitor owns one hidden member_workflows row (period 0, disabled), so fixed schedules, their
-- listing and the fixed-period timer are untouched. Runs that carry source references are ordinary
-- member_work_runs; only the timing, the last evidence fingerprint and the occurrence ledger live here.
-- Nothing in these tables stores source text: references are re-read with live authority on display.
CREATE TABLE member_work_monitors (
 id TEXT PRIMARY KEY NOT NULL REFERENCES member_workflows(id) ON DELETE CASCADE,
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0), auth_epoch INTEGER NOT NULL, host_epoch INTEGER NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('watch','brief')),
 every_seconds INTEGER CHECK(every_seconds IN (1800,3600,86400)),
 local_time TEXT CHECK(local_time IS NULL OR (length(local_time)=5 AND substr(local_time,3,1)=':')),
 zone TEXT,
 days INTEGER NOT NULL CHECK(days BETWEEN 1 AND 90),
 state TEXT NOT NULL CHECK(state IN ('active','stopped','expired','revoked')),
 revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0),
 created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
 next_due INTEGER, ended_at INTEGER, stop_request_id TEXT,
 fingerprint TEXT, source_count INTEGER NOT NULL DEFAULT 0,
 last_checked_at INTEGER, last_changed_at INTEGER, last_run_id TEXT,
 alerts INTEGER NOT NULL DEFAULT 0,
 CHECK((kind='watch')=(every_seconds IS NOT NULL)),
 CHECK((kind='brief')=(local_time IS NOT NULL AND zone IS NOT NULL)),
 CHECK((state='active')=(next_due IS NOT NULL)),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX member_work_monitor_due ON member_work_monitors(next_due) WHERE state='active';
CREATE INDEX member_work_monitor_owner ON member_work_monitors(tenant_id,account_id,volume_id,generation,state);

-- One row per processed occurrence. (monitor_id,due_at) makes a repeated or concurrent tick idempotent,
-- including unchanged checks that deliberately create no run and no notification.
CREATE TABLE member_work_monitor_checks (
 monitor_id TEXT NOT NULL REFERENCES member_work_monitors(id) ON DELETE CASCADE,
 due_at INTEGER NOT NULL, checked_at INTEGER NOT NULL,
 outcome TEXT NOT NULL CHECK(outcome IN ('baseline','unchanged','changed','briefed','missed','failed')),
 source_count INTEGER NOT NULL DEFAULT 0, skipped INTEGER NOT NULL DEFAULT 0 CHECK(skipped>=0),
 run_id TEXT, notification_id TEXT,
 PRIMARY KEY(monitor_id,due_at)
) STRICT;

PRAGMA user_version=94;
