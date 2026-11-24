-- Durable conversation work. A group is one ordinary completed chat receipt (turn_id) and up to six
-- independent children. Each child executes through its own member_workflows row (same id, period 0)
-- and member_work_runs, so source references are re-read with live authorization when displayed.
CREATE TABLE member_conversation_groups (
 id TEXT PRIMARY KEY NOT NULL, turn_id TEXT NOT NULL,
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0), thread_id TEXT NOT NULL,
 host_epoch INTEGER NOT NULL, auth_epoch INTEGER NOT NULL,
 message TEXT NOT NULL, payload TEXT NOT NULL CHECK(json_valid(payload)), origin_task TEXT,
 created_at INTEGER NOT NULL, admitted_order INTEGER NOT NULL UNIQUE,
 FOREIGN KEY(tenant_id,turn_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX member_conversation_group_thread ON member_conversation_groups(tenant_id,account_id,thread_id,admitted_order DESC);
CREATE TABLE member_conversation_tasks (
 id TEXT PRIMARY KEY NOT NULL REFERENCES member_workflows(id) ON DELETE CASCADE,
 group_id TEXT NOT NULL REFERENCES member_conversation_groups(id) ON DELETE CASCADE,
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0), thread_id TEXT NOT NULL,
 position INTEGER NOT NULL CHECK(position BETWEEN 0 AND 5),
 title TEXT NOT NULL, scope TEXT NOT NULL CHECK(scope IN ('memory','conversation')), query TEXT NOT NULL,
 filters TEXT NOT NULL DEFAULT '[]' CHECK(json_valid(filters) AND json_array_length(filters)<=4),
 state TEXT NOT NULL CHECK(state IN ('queued','needs_input','completed','cancelled','failed')),
 revision INTEGER NOT NULL DEFAULT 1 CHECK(revision>0), question_id TEXT,
 host_epoch INTEGER NOT NULL, auth_epoch INTEGER NOT NULL,
 queued_seq INTEGER NOT NULL, task_order INTEGER NOT NULL UNIQUE, created_at INTEGER NOT NULL,
 CHECK((state='needs_input')=(question_id IS NOT NULL)),
 UNIQUE(group_id,position)
) STRICT;
CREATE INDEX member_conversation_queue ON member_conversation_tasks(tenant_id,account_id,volume_id,generation,queued_seq) WHERE state='queued';
CREATE TABLE member_conversation_events (
 id INTEGER PRIMARY KEY, task_id TEXT NOT NULL REFERENCES member_conversation_tasks(id) ON DELETE CASCADE,
 revision INTEGER NOT NULL, state TEXT NOT NULL, text TEXT NOT NULL, at INTEGER NOT NULL
) STRICT;
CREATE INDEX member_conversation_event_order ON member_conversation_events(task_id,id);
-- One applied transition per observed revision makes an exact replay idempotent and any other use stale.
CREATE TABLE member_conversation_actions (
 task_id TEXT NOT NULL REFERENCES member_conversation_tasks(id) ON DELETE CASCADE,
 from_revision INTEGER NOT NULL, action TEXT NOT NULL, arg TEXT NOT NULL,
 PRIMARY KEY(task_id,from_revision)
) STRICT;
CREATE TABLE member_conversation_runs (
 run_id TEXT PRIMARY KEY NOT NULL REFERENCES member_work_runs(id) ON DELETE CASCADE,
 task_id TEXT NOT NULL REFERENCES member_conversation_tasks(id) ON DELETE CASCADE,
 revision INTEGER NOT NULL, filters TEXT NOT NULL CHECK(json_valid(filters))
) STRICT;
CREATE INDEX member_conversation_run_task ON member_conversation_runs(task_id);

-- Reports contain a verified occurrence and a template title. Build the body
-- from authorized source revisions on read so withdrawal reaches saved reports.
CREATE TABLE member_conversation_reports (
 id TEXT PRIMARY KEY NOT NULL, tenant_id TEXT NOT NULL, account_id TEXT NOT NULL,
 volume_id TEXT NOT NULL, generation INTEGER NOT NULL CHECK(generation>0),
 thread_id TEXT NOT NULL, task_id TEXT NOT NULL REFERENCES member_conversation_tasks(id) ON DELETE CASCADE,
 run_id TEXT NOT NULL REFERENCES member_conversation_runs(run_id) ON DELETE CASCADE,
 title TEXT NOT NULL, UNIQUE(task_id,run_id),
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;

CREATE TABLE personal_browser_progress (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL,
 fingerprint TEXT NOT NULL CHECK(length(fingerprint)=64),
 guidance INTEGER NOT NULL CHECK(guidance>=0),
 repeats INTEGER NOT NULL DEFAULT 0 CHECK(repeats>=0),
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES personal_browser_jobs(tenant_id,run_id) ON DELETE CASCADE
) STRICT;

PRAGMA user_version=92;
