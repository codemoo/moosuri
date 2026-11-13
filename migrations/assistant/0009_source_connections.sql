CREATE TABLE source_connections (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, account_id TEXT NOT NULL,
 collection_id TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN('custom_http','gmail','slack')),
 adapter_version TEXT NOT NULL DEFAULT '1',
 name TEXT NOT NULL, scope TEXT NOT NULL, remote_account TEXT NOT NULL, mail_connection_id TEXT, mail_epoch INTEGER,
 config_revision INTEGER NOT NULL DEFAULT 1 CHECK(config_revision>0), credential_epoch INTEGER NOT NULL DEFAULT 1 CHECK(credential_epoch>0),
 auth_epoch INTEGER NOT NULL, state TEXT NOT NULL DEFAULT 'active' CHECK(state IN('active','paused','disconnected','needs_reconnect')),
 sync_enabled INTEGER NOT NULL CHECK(sync_enabled IN(0,1)), poll_interval INTEGER NOT NULL CHECK(poll_interval BETWEEN 60 AND 86400),
 token_hash BLOB CHECK(token_hash IS NULL OR length(token_hash)=32), token_expires INTEGER, credential BLOB,
 checkpoint TEXT, run_generation INTEGER NOT NULL DEFAULT 0 CHECK(run_generation>=0),
 last_heartbeat INTEGER, last_commit INTEGER, last_completed INTEGER, next_due INTEGER NOT NULL DEFAULT 0,
 failure_count INTEGER NOT NULL DEFAULT 0 CHECK(failure_count BETWEEN 0 AND 16), error TEXT,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,collection_id),
 FOREIGN KEY(tenant_id,collection_id) REFERENCES knowledge_collections(tenant_id,id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id),
 FOREIGN KEY(tenant_id,mail_connection_id) REFERENCES mail_connections(tenant_id,id)
) STRICT;
CREATE UNIQUE INDEX source_token_hash ON source_connections(token_hash) WHERE token_hash IS NOT NULL;
CREATE INDEX source_connections_due ON source_connections(state,sync_enabled,next_due);
CREATE TABLE source_sync_runs (
 tenant_id TEXT NOT NULL, connection_id TEXT NOT NULL,id TEXT NOT NULL,generation INTEGER NOT NULL,
 config_revision INTEGER NOT NULL, credential_epoch INTEGER NOT NULL, nonce BLOB NOT NULL CHECK(length(nonce)=32),
 state TEXT NOT NULL CHECK(state IN('running','completed','partial','no_change','polled','failed','unknown','needs_reconnect')),
 started_at INTEGER NOT NULL, heartbeat_at INTEGER NOT NULL, lease_until INTEGER NOT NULL, finished_at INTEGER,error TEXT,
 page_count INTEGER NOT NULL DEFAULT 0 CHECK(page_count BETWEEN 0 AND 256),
 PRIMARY KEY(tenant_id,connection_id,id),UNIQUE(tenant_id,connection_id,generation),
 FOREIGN KEY(tenant_id,connection_id) REFERENCES source_connections(tenant_id,id)
) STRICT;
CREATE UNIQUE INDEX source_one_active_run ON source_sync_runs(tenant_id,connection_id) WHERE state='running';
CREATE TABLE source_batch_receipts (
 tenant_id TEXT NOT NULL,connection_id TEXT NOT NULL,request_id TEXT NOT NULL,run_id TEXT NOT NULL,
 digest BLOB NOT NULL CHECK(length(digest)=32),sequence INTEGER NOT NULL,committed_at INTEGER NOT NULL,
 checkpoint_before TEXT,checkpoint_after TEXT,receipt TEXT NOT NULL,
 PRIMARY KEY(tenant_id,connection_id,request_id),
 FOREIGN KEY(tenant_id,connection_id) REFERENCES source_connections(tenant_id,id)
) STRICT;
CREATE TABLE source_provenance (
 tenant_id TEXT NOT NULL,source_id TEXT NOT NULL,source_revision INTEGER NOT NULL,
 connection_id TEXT NOT NULL,request_id TEXT NOT NULL,run_id TEXT NOT NULL,received_at INTEGER NOT NULL,
 thread_id TEXT,parent_id TEXT,
 PRIMARY KEY(tenant_id,source_id,source_revision),
 FOREIGN KEY(tenant_id,source_id,source_revision) REFERENCES knowledge_source_versions(tenant_id,source_id,revision) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,connection_id) REFERENCES source_connections(tenant_id,id)
) STRICT;
PRAGMA user_version=9;
