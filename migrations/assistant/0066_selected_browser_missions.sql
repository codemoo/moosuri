-- A user-selected, bounded observation is a private source, never browser authority.
CREATE TABLE mission_browser_selections (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, request_id TEXT NOT NULL,
 auth_epoch INTEGER NOT NULL, request_digest TEXT NOT NULL,
 origin_thread TEXT NOT NULL, workspace_id TEXT NOT NULL,
 mission_id TEXT NOT NULL, run_id TEXT NOT NULL, acceptance_revision INTEGER NOT NULL,
 source_id TEXT NOT NULL, source_revision INTEGER NOT NULL, receipt TEXT NOT NULL,
 PRIMARY KEY(tenant_id,request_id), UNIQUE(tenant_id,source_id),
 FOREIGN KEY(tenant_id,origin_thread) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,source_id) REFERENCES knowledge_sources(tenant_id,id)
) STRICT;
CREATE INDEX browser_selection_origin ON mission_browser_selections(tenant_id,origin_thread);
PRAGMA user_version=66;
