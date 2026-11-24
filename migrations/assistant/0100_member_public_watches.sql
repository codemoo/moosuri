-- Member public-page watch (schema 100): "check https://status.example.test/service every hour and
-- tell me only when it changes". New tables only; no existing table is altered.
--
-- One explicit grant per watch, pinned to the owner (tenant/account/volume/generation), the account
-- auth epoch, the gateway host epoch, the original thread, the exact public HTTPS URL (and its host),
-- the cadence and the end date. The timer reads the page through the verified member browser broker in a
-- fresh, disposable workspace (open, read, stop). No generic network client, no member cookies, no model.
--
-- (watch, due_at) is the dedup key: a slot is claimed once, *before* any browser IO, and a restart marks
-- an unsettled claim 'unknown' instead of re-reading it. A failed read is 'failed' and can never be a
-- change. Page text is never stored in full: only a SHA-256 of the bounded normalized visible text, and a
-- short encrypted excerpt for baseline/changed checks.
-- Requires migrations 93-99 to be registered first (version registry is contiguous).
CREATE TABLE member_public_watches (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,id TEXT NOT NULL CHECK(length(id)=36),
 volume_id TEXT NOT NULL,generation INTEGER NOT NULL CHECK(generation>0),
 auth_epoch INTEGER NOT NULL,host_epoch INTEGER NOT NULL,
 thread_id TEXT NOT NULL CHECK(length(thread_id)=36),
 url TEXT NOT NULL CHECK(length(url) BETWEEN 11 AND 2048 AND substr(url,1,8)='https://'),
 host TEXT NOT NULL CHECK(length(host) BETWEEN 3 AND 253),
 every_seconds INTEGER NOT NULL CHECK(every_seconds IN(1800,3600,86400)),
 days INTEGER NOT NULL CHECK(days BETWEEN 1 AND 90),
 state TEXT NOT NULL CHECK(state IN('active','stopped','expired','revoked')),
 end_reason TEXT,
 revision INTEGER NOT NULL CHECK(revision>0),
 created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL,next_due INTEGER,ended_at INTEGER,
 request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 stop_request_id TEXT,
 -- Evidence of the last verified read: fingerprint only (no page text) and the slot that produced it.
 fingerprint TEXT CHECK(fingerprint IS NULL OR length(fingerprint)=64),
 last_due INTEGER,last_observed_at INTEGER,last_changed_at INTEGER,
 alerts INTEGER NOT NULL DEFAULT 0 CHECK(alerts>=0),
 PRIMARY KEY(tenant_id,account_id,id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 CHECK((state='active')=(next_due IS NOT NULL))
) STRICT;
CREATE INDEX member_public_watches_due ON member_public_watches(tenant_id,account_id,state,next_due);
CREATE INDEX member_public_watches_thread ON member_public_watches(tenant_id,account_id,thread_id,created_at DESC);

-- `workspace_id` is the disposable workspace id minted for this one check (never a human's workspace).
-- `closed=0` means the broker may still hold it; the timer stops such workspaces before claiming new slots.
CREATE TABLE member_public_watch_checks (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,watch_id TEXT NOT NULL,due_at INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN('claimed','baseline','unchanged','changed','failed','unknown','withdrawn')),
 workspace_id TEXT NOT NULL CHECK(length(workspace_id)=36),
 closed INTEGER NOT NULL DEFAULT 0 CHECK(closed IN(0,1)),
 reason TEXT,
 fingerprint TEXT CHECK(fingerprint IS NULL OR length(fingerprint)=64),
 cipher BLOB,notification_id TEXT,
 skipped INTEGER NOT NULL DEFAULT 0 CHECK(skipped>=0),
 claimed_at INTEGER NOT NULL,finished_at INTEGER,
 PRIMARY KEY(tenant_id,account_id,watch_id,due_at),
 FOREIGN KEY(tenant_id,account_id,watch_id) REFERENCES member_public_watches(tenant_id,account_id,id) ON DELETE CASCADE,
 CHECK(cipher IS NULL OR state IN('baseline','changed')),
 CHECK(fingerprint IS NULL OR state IN('baseline','unchanged','changed')),
 CHECK(state!='claimed' OR finished_at IS NULL)
) STRICT;
CREATE UNIQUE INDEX member_public_watch_checks_workspace ON member_public_watch_checks(workspace_id);
CREATE INDEX member_public_watch_checks_open ON member_public_watch_checks(tenant_id,account_id) WHERE closed=0;
CREATE INDEX member_public_watch_checks_recent ON member_public_watch_checks(tenant_id,account_id,watch_id,due_at DESC);
PRAGMA user_version=100;
