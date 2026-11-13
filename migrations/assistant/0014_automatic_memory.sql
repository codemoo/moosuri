-- This isolated slice applies after12 using local13; integration follows backup13 as14.
CREATE TABLE automatic_memory_grants (
 tenant_id TEXT NOT NULL, collection_id TEXT NOT NULL, account_id TEXT NOT NULL,
 revision INTEGER NOT NULL, enabled INTEGER NOT NULL CHECK(enabled IN(0,1)),
 attribution_mode TEXT NOT NULL, reported_speaker TEXT NOT NULL, auth_epoch INTEGER NOT NULL, model TEXT NOT NULL,
 daily_calls INTEGER NOT NULL, daily_bytes INTEGER NOT NULL, daily_output INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,collection_id),
 FOREIGN KEY(tenant_id,collection_id) REFERENCES knowledge_collections(tenant_id,id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TABLE automatic_memory_jobs (
 tenant_id TEXT NOT NULL, id TEXT NOT NULL, collection_id TEXT NOT NULL,
 source_id TEXT NOT NULL, source_revision INTEGER NOT NULL, grant_revision INTEGER NOT NULL,
 state TEXT NOT NULL, reason TEXT, nonce BLOB, dispatch_nonce BLOB, deadline INTEGER, not_before INTEGER NOT NULL DEFAULT 0,
 saved_count INTEGER NOT NULL DEFAULT 0, input_bytes INTEGER NOT NULL DEFAULT 0,
 usage TEXT, model TEXT NOT NULL, created_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id),UNIQUE(tenant_id,source_id,source_revision,grant_revision),
 FOREIGN KEY(tenant_id,collection_id) REFERENCES knowledge_collections(tenant_id,id),
 FOREIGN KEY(tenant_id,source_id) REFERENCES knowledge_sources(tenant_id,id)
) STRICT;
CREATE TABLE automatic_memory_budget (
 tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,day INTEGER NOT NULL,
 calls INTEGER NOT NULL,input_bytes INTEGER NOT NULL,output_tokens INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,account_id,day)
) STRICT;
CREATE TABLE automatic_memory_facts (
 tenant_id TEXT NOT NULL,source_id TEXT NOT NULL,quote_hash BLOB NOT NULL,reported_speaker_hash BLOB NOT NULL,
 memory_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,source_id,quote_hash,reported_speaker_hash),
 FOREIGN KEY(tenant_id,memory_id) REFERENCES knowledge_memories(tenant_id,id)
) STRICT;
CREATE TABLE automatic_memory_origins (
 tenant_id TEXT NOT NULL,memory_id TEXT NOT NULL,source_id TEXT NOT NULL,
 source_revision INTEGER NOT NULL,reported_speaker TEXT NOT NULL,model TEXT NOT NULL,grant_revision INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,memory_id),
 FOREIGN KEY(tenant_id,memory_id) REFERENCES knowledge_memories(tenant_id,id)
) STRICT;
PRAGMA user_version=14;
