-- Member connected morning brief (Gmail inbox metadata + today's primary Calendar). Schema 99.
-- Read-only. One explicit recurring grant per brief, pinned to the exact Google connection id +
-- epoch + app revision the member saw, the account auth epoch and the gateway host epoch.
-- Tokens are never stored here: they stay in member_google_grants. A run row is the durable
-- dedup key (brief, due slot) and the in-thread worklog entry. Private evidence is encrypted and
-- is purged on disconnect, app change, revocation or session change.
-- Requires migrations 93-98 to be registered first (version registry is contiguous).
CREATE TABLE member_connected_briefs (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,id TEXT NOT NULL CHECK(length(id)=36),
 volume_id TEXT NOT NULL,generation INTEGER NOT NULL CHECK(generation>0),
 auth_epoch INTEGER NOT NULL,host_epoch INTEGER NOT NULL,
 thread_id TEXT NOT NULL,
 gmail_connection TEXT,gmail_epoch INTEGER,gmail_app TEXT,
 calendar_connection TEXT,calendar_epoch INTEGER,calendar_app TEXT,
 local_time TEXT NOT NULL CHECK(length(local_time)=5),
 zone TEXT NOT NULL CHECK(length(zone) BETWEEN 1 AND 64),
 interval_days INTEGER NOT NULL CHECK(interval_days BETWEEN 1 AND 7),
 state TEXT NOT NULL CHECK(state IN('active','stopped','expired','revoked')),
 end_reason TEXT,
 revision INTEGER NOT NULL CHECK(revision>0),
 created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL,next_due INTEGER,ended_at INTEGER,
 request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 stop_request_id TEXT,
 -- Explicit member choice: Gmail part reads only Google's IMPORTANT+INBOX labelled mail. Defaults to
 -- the ordinary recent-inbox brief and can never apply without a Gmail pin.
 important_only INTEGER NOT NULL DEFAULT 0 CHECK(important_only IN(0,1)),
 PRIMARY KEY(tenant_id,account_id,id),
 CHECK(important_only=0 OR gmail_connection IS NOT NULL),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 CHECK(gmail_connection IS NOT NULL OR calendar_connection IS NOT NULL),
 CHECK((gmail_connection IS NULL)=(gmail_epoch IS NULL) AND (gmail_connection IS NULL)=(gmail_app IS NULL)),
 CHECK((calendar_connection IS NULL)=(calendar_epoch IS NULL) AND (calendar_connection IS NULL)=(calendar_app IS NULL)),
 CHECK((state='active')=(next_due IS NOT NULL))
) STRICT;
CREATE INDEX member_connected_briefs_due ON member_connected_briefs(tenant_id,account_id,state,next_due);

-- Primary key = the dedup key: a slot is claimed once, before any provider IO, and a restart marks
-- an unsettled claim 'unknown' instead of re-reading. `cipher` holds only minimal selected evidence.
CREATE TABLE member_connected_brief_runs (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,brief_id TEXT NOT NULL,due_at INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN('claimed','completed','partial','unavailable','needs_connection','missed','unknown','withdrawn')),
 freshness TEXT CHECK(freshness IN('live','projection')),
 guidance TEXT,
 skipped INTEGER NOT NULL DEFAULT 0 CHECK(skipped>=0),
 gmail_count INTEGER,calendar_count INTEGER,
 cipher BLOB,notification_id TEXT,
 claimed_at INTEGER NOT NULL,finished_at INTEGER,
 PRIMARY KEY(tenant_id,account_id,brief_id,due_at),
 FOREIGN KEY(tenant_id,account_id,brief_id) REFERENCES member_connected_briefs(tenant_id,account_id,id) ON DELETE CASCADE,
 CHECK(cipher IS NULL OR state IN('completed','partial')),
 CHECK(state!='claimed' OR finished_at IS NULL)
) STRICT;
CREATE INDEX member_connected_brief_runs_recent ON member_connected_brief_runs(tenant_id,account_id,brief_id,due_at DESC);
PRAGMA user_version=99;
