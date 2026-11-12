CREATE TABLE knowledge_state (
 tenant_id TEXT PRIMARY KEY NOT NULL REFERENCES tenants(id), generation INTEGER NOT NULL DEFAULT 0 CHECK(generation>=0)
) STRICT;
CREATE TABLE knowledge_journal_meta (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), sequence INTEGER NOT NULL CHECK(sequence>=0), head BLOB NOT NULL CHECK(length(head)=32)
) STRICT;
INSERT INTO knowledge_journal_meta VALUES(1,0,zeroblob(32));
CREATE TABLE knowledge_collections (
 tenant_id TEXT NOT NULL REFERENCES tenants(id), id TEXT NOT NULL, name TEXT NOT NULL, scope TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0), cursor INTEGER NOT NULL CHECK(cursor>=0),
 connected INTEGER NOT NULL CHECK(connected IN(0,1)), deleted INTEGER NOT NULL CHECK(deleted IN(0,1)),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,scope)
) STRICT;
CREATE TABLE knowledge_sources (
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,collection_id TEXT NOT NULL,external_id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),deleted INTEGER NOT NULL DEFAULT 0 CHECK(deleted IN(0,1)),
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,collection_id,external_id),
 FOREIGN KEY(tenant_id,collection_id) REFERENCES knowledge_collections(tenant_id,id)
) STRICT;
CREATE TABLE knowledge_source_versions (
 tenant_id TEXT NOT NULL,source_id TEXT NOT NULL,revision INTEGER NOT NULL,version TEXT NOT NULL,
 title TEXT NOT NULL,text TEXT NOT NULL,speaker TEXT,observed_at INTEGER NOT NULL,valid_from INTEGER,valid_until INTEGER,
 digest BLOB NOT NULL CHECK(length(digest)=32),
 PRIMARY KEY(tenant_id,source_id,revision),
 FOREIGN KEY(tenant_id,source_id) REFERENCES knowledge_sources(tenant_id,id)
) STRICT;
CREATE TABLE knowledge_batches (
 tenant_id TEXT NOT NULL,request_id TEXT NOT NULL,collection_id TEXT NOT NULL,digest BLOB NOT NULL CHECK(length(digest)=32),receipt TEXT NOT NULL,
 PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,collection_id) REFERENCES knowledge_collections(tenant_id,id)
) STRICT;
CREATE TABLE knowledge_memories (
 tenant_id TEXT NOT NULL REFERENCES tenants(id),id TEXT NOT NULL,revision INTEGER NOT NULL,
 status TEXT NOT NULL CHECK(status IN('active','invalidated','forgotten')),created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id)
) STRICT;
CREATE TABLE knowledge_memory_versions (
 tenant_id TEXT NOT NULL,memory_id TEXT NOT NULL,revision INTEGER NOT NULL,draft TEXT NOT NULL,
 status TEXT NOT NULL CHECK(status IN('active','invalidated')),created_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,memory_id,revision),FOREIGN KEY(tenant_id,memory_id) REFERENCES knowledge_memories(tenant_id,id)
) STRICT;
-- Content-free temporal ordering survives removal of source-dependent plaintext.
-- A missing version at the selected revision is an invalidated assertion, never
-- permission to search an older surviving version.
CREATE TABLE knowledge_memory_timeline (
 tenant_id TEXT NOT NULL,memory_id TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>0),valid_from INTEGER,
 PRIMARY KEY(tenant_id,memory_id,revision),
 FOREIGN KEY(tenant_id,memory_id) REFERENCES knowledge_memories(tenant_id,id)
) STRICT;
CREATE TABLE knowledge_evidence (
 tenant_id TEXT NOT NULL,memory_id TEXT NOT NULL,memory_revision INTEGER NOT NULL,source_id TEXT NOT NULL,source_revision INTEGER NOT NULL,
 start_byte INTEGER NOT NULL,end_byte INTEGER NOT NULL,quote_hash BLOB NOT NULL CHECK(length(quote_hash)=32),
 PRIMARY KEY(tenant_id,memory_id,memory_revision,source_id,start_byte,end_byte),
 FOREIGN KEY(tenant_id,memory_id,memory_revision) REFERENCES knowledge_memory_versions(tenant_id,memory_id,revision) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,source_id) REFERENCES knowledge_sources(tenant_id,id)
) STRICT;
CREATE INDEX knowledge_evidence_source ON knowledge_evidence(tenant_id,source_id,source_revision);
CREATE TABLE knowledge_writes (
 tenant_id TEXT NOT NULL REFERENCES tenants(id),request_id TEXT NOT NULL,digest BLOB NOT NULL CHECK(length(digest)=32),memory_id TEXT NOT NULL,revision INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,request_id),FOREIGN KEY(tenant_id,memory_id) REFERENCES knowledge_memories(tenant_id,id)
) STRICT;
CREATE TABLE knowledge_suppressions (
 tenant_id TEXT NOT NULL REFERENCES tenants(id),source_id TEXT NOT NULL,quote_hash BLOB NOT NULL CHECK(length(quote_hash)=32),
 PRIMARY KEY(tenant_id,source_id,quote_hash)
) STRICT;
-- The tenant token participates in MATCH, so other tenants never enter its hit set.
-- No global bm25 score is returned or used; ordering is local and deterministic.
CREATE VIRTUAL TABLE knowledge_fts USING fts5(tenant_key,kind UNINDEXED,object_id UNINDEXED,revision UNINDEXED,normalized, tokenize='trigram');
PRAGMA user_version=4;
