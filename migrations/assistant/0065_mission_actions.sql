-- Links only: MailAction and the existing Calendar ledger retain effect authority.
CREATE TABLE mission_action_contracts (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL, acceptance_revision INTEGER NOT NULL,
 account_id TEXT NOT NULL, kind TEXT NOT NULL CHECK(kind IN('mail','calendar')),
 action_id TEXT NOT NULL, payload_digest TEXT, action_revision INTEGER, action_digest TEXT,
 evidence TEXT, PRIMARY KEY(tenant_id,run_id,acceptance_revision,kind),
 UNIQUE(tenant_id,action_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=65;
