-- One combined report per conversation group. The row is the durable request (created with the
-- group, or later once work exists) and, once every child is terminal, the report itself. It stores
-- only references to verified child runs and a template title: excerpts are resolved from current
-- member sources on every read, so source edits and withdrawals reach saved reports.
--   pending     requested; at least one child is still queued or waiting for an answer
--   ready       built from the completed children listed in ..._items
--   unavailable no completed child could be verified (failed/cancelled work, changed authority)
CREATE TABLE member_conversation_group_reports (
 id TEXT PRIMARY KEY NOT NULL,
 group_id TEXT NOT NULL REFERENCES member_conversation_groups(id) ON DELETE CASCADE,
 request_id TEXT NOT NULL UNIQUE,
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0), thread_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('pending','ready','unavailable')),
 host_epoch INTEGER NOT NULL, auth_epoch INTEGER NOT NULL,
 superseded INTEGER NOT NULL DEFAULT 0 CHECK(superseded IN (0,1)),
 title TEXT NOT NULL DEFAULT '',
 created_at INTEGER NOT NULL, finalized_at INTEGER,
 CHECK((state='pending')=(finalized_at IS NULL)),
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE UNIQUE INDEX member_conversation_group_report_current ON member_conversation_group_reports(group_id) WHERE superseded=0;
CREATE INDEX member_conversation_group_report_pending ON member_conversation_group_reports(tenant_id,account_id,created_at) WHERE state='pending';

CREATE TABLE member_conversation_group_report_items (
 report_id TEXT NOT NULL REFERENCES member_conversation_group_reports(id) ON DELETE CASCADE,
 task_id TEXT NOT NULL REFERENCES member_conversation_tasks(id) ON DELETE CASCADE,
 run_id TEXT NOT NULL REFERENCES member_conversation_runs(run_id) ON DELETE CASCADE,
 position INTEGER NOT NULL CHECK(position BETWEEN 0 AND 5),
 PRIMARY KEY(report_id,task_id)
) STRICT;
CREATE INDEX member_conversation_group_report_item_run ON member_conversation_group_report_items(run_id);

-- Only the closed, locally validated plan and result reference survive a restart.
-- Source excerpts are never captured here. A thread deletion removes its replay records.
CREATE TABLE member_conversation_interpretations (
 request_id TEXT PRIMARY KEY NOT NULL,
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL, thread_id TEXT NOT NULL, host_epoch INTEGER NOT NULL, auth_epoch INTEGER NOT NULL,
 fingerprint BLOB NOT NULL CHECK(length(fingerprint)=32),
 plan TEXT CHECK(plan IS NULL OR json_valid(plan)),
 outcome TEXT CHECK(outcome IS NULL OR (plan IS NOT NULL AND json_valid(outcome))),
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE member_conversation_exclusions (
 task_id TEXT NOT NULL REFERENCES member_conversation_tasks(id) ON DELETE CASCADE,
 word TEXT NOT NULL COLLATE NOCASE,
 PRIMARY KEY(task_id,word)
) STRICT;
PRAGMA user_version=93;
