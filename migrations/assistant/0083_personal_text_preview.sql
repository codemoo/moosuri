-- Private dispatch binding, not a second conversation store or spending ledger.
CREATE TABLE personal_text_previews (
 tenant_id TEXT NOT NULL,
 run_id TEXT NOT NULL,
 volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0),
 connection_revision TEXT NOT NULL,
 call_id TEXT NOT NULL,
 nonce BLOB NOT NULL CHECK(length(nonce)=32),
 receipt TEXT,
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=83;
