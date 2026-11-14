-- Start identities expire with ordinary run history. Monotonic connection
-- generation, not a forever-global UUID promise, fences every pruned old intent.
CREATE TABLE source_run_starts (
 tenant_id TEXT NOT NULL,connection_id TEXT NOT NULL,run_id TEXT NOT NULL,
 client_request_id TEXT NOT NULL,request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
 PRIMARY KEY(tenant_id,connection_id,run_id),
 FOREIGN KEY(tenant_id,connection_id,run_id) REFERENCES source_sync_runs(tenant_id,connection_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=48;
