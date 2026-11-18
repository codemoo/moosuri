-- Image capability proof is exact to owner, transport, revision, model and effort.
-- Synthetic image pixels and answers are never persisted.
CREATE TABLE personal_subscription_vision_probes(
 source TEXT NOT NULL CHECK(source IN('claude','chatgpt')),
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, binding_id TEXT NOT NULL,
 revision INTEGER NOT NULL, model TEXT NOT NULL, effort TEXT NOT NULL,
 harness TEXT NOT NULL, verified_at INTEGER NOT NULL,
 PRIMARY KEY(source,tenant_id,account_id,binding_id,revision,model,effort,harness),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE personal_subscription_vision_attempts(
 source TEXT NOT NULL CHECK(source IN('claude','chatgpt')),
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, id TEXT NOT NULL,
 reserved INTEGER NOT NULL CHECK(reserved IN(0,1)), expires_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,account_id,id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=79;
