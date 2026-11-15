-- Local draft identities only. No content, recipient, binding or approval copy.
CREATE TABLE compound_draft_adoptions (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, run_id TEXT NOT NULL,
 action_id TEXT NOT NULL, action_revision INTEGER NOT NULL, action_digest TEXT NOT NULL,
 recipe_route TEXT,
 PRIMARY KEY(tenant_id,account_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;

PRAGMA user_version=67;
