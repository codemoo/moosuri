-- Content-free exact-version provenance for every delivered history candidate/read.
CREATE TABLE conversation_recall_dependencies (
 tenant_id TEXT NOT NULL,run_id TEXT NOT NULL,source_run_id TEXT NOT NULL,
 source_version INTEGER NOT NULL CHECK(source_version>0),
 discovered INTEGER NOT NULL DEFAULT 0 CHECK(discovered IN(0,1)),
 PRIMARY KEY(tenant_id,run_id,source_run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE INDEX conversation_recall_source ON conversation_recall_dependencies(tenant_id,source_run_id);
PRAGMA user_version=47;
