-- Owner replies fill bounded values, never create new operation authority.
CREATE TABLE mission_input_waits (
 tenant_id TEXT NOT NULL,mission_id TEXT NOT NULL,run_id TEXT NOT NULL,
 request_id TEXT NOT NULL,acceptance_revision INTEGER NOT NULL,mission_revision INTEGER NOT NULL,
 step INTEGER NOT NULL,field TEXT NOT NULL CHECK(field IN('mail_to','calendar_date','calendar_start','calendar_duration')),
 question TEXT NOT NULL,value TEXT,status TEXT NOT NULL CHECK(status IN('waiting','ready','resumed','expired','cancelled')),
 expires_ms INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,request_id),UNIQUE(tenant_id,run_id,acceptance_revision,field),
 FOREIGN KEY(tenant_id,mission_id) REFERENCES missions(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE mission_input_replies (
 tenant_id TEXT NOT NULL,mission_id TEXT NOT NULL,request_id TEXT NOT NULL,payload TEXT NOT NULL,
 PRIMARY KEY(tenant_id,mission_id,request_id),
 FOREIGN KEY(tenant_id,mission_id) REFERENCES missions(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX mission_input_ready ON mission_input_waits(status);
PRAGMA user_version=69;
