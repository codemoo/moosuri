-- Browser-only, source-bound private mail observations. Never chat/model input.
CREATE TABLE member_google_thread_reads (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, request_id TEXT NOT NULL,
 thread_id TEXT NOT NULL, volume_id TEXT NOT NULL, generation INTEGER NOT NULL CHECK(generation>0),
 auth_epoch INTEGER NOT NULL, session_digest BLOB NOT NULL CHECK(length(session_digest)=32),
 app_revision TEXT NOT NULL, flow_id TEXT, connection_id TEXT, source_epoch INTEGER,
 phase TEXT NOT NULL CHECK(phase IN('awaiting_consent','ready','dispatched','completed','unknown','withdrawn')),
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, cipher BLOB,
 PRIMARY KEY(tenant_id,account_id,request_id), UNIQUE(tenant_id,account_id,flow_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 CHECK((phase='completed' AND cipher IS NOT NULL) OR (phase!='completed' AND cipher IS NULL)),
 CHECK((connection_id IS NULL AND source_epoch IS NULL) OR (connection_id IS NOT NULL AND source_epoch>0))
) STRICT;
PRAGMA user_version=85;
