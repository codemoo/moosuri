-- Private image bytes and revision-pinned vision capability, schema21.
CREATE TABLE media_objects (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,id TEXT NOT NULL,revision INTEGER NOT NULL DEFAULT 1,
 input_digest BLOB, digest TEXT NOT NULL DEFAULT '', state TEXT NOT NULL CHECK(state IN('processing','ready','retry_required','deleted')),
 nonce TEXT,session_digest BLOB,auth_epoch INTEGER NOT NULL,width INTEGER NOT NULL DEFAULT 0,height INTEGER NOT NULL DEFAULT 0,
 bytes BLOB,thumbnail BLOB,created_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id),CHECK(bytes IS NULL OR length(bytes)<=2097152),CHECK(thumbnail IS NULL OR length(thumbnail)<=131072)
) STRICT;
CREATE TABLE chat_media (tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,media_id TEXT NOT NULL,revision INTEGER NOT NULL,ordinal INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,run_id,media_id),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE) STRICT;
CREATE TABLE media_run_dependencies (tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,media_id TEXT NOT NULL,revision INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,run_id,media_id),FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE) STRICT;
CREATE TABLE provider_vision_probes (provider_id TEXT NOT NULL,revision INTEGER NOT NULL,model TEXT NOT NULL,effort TEXT NOT NULL,
 PRIMARY KEY(provider_id,revision,model,effort)) STRICT;
PRAGMA user_version=21;
