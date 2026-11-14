-- Logical account destruction; no private content belongs in these tombstones.
ALTER TABLE accounts ADD COLUMN deleted INTEGER NOT NULL DEFAULT 0 CHECK(deleted IN(0,1));
CREATE TABLE account_deletions (
 account_id TEXT PRIMARY KEY REFERENCES accounts(id), tenant_id TEXT UNIQUE NOT NULL,
 deleted_at INTEGER NOT NULL, local_state TEXT NOT NULL CHECK(local_state IN('pending','complete'))
) STRICT;
CREATE TABLE vault_lifecycle_outbox (
 id TEXT PRIMARY KEY, account_id TEXT NOT NULL, tenant_id TEXT NOT NULL,
 installation_id TEXT NOT NULL, auth_epoch INTEGER NOT NULL,
 action TEXT NOT NULL CHECK(action IN('revoke','delete')),
 acknowledged INTEGER NOT NULL DEFAULT 0 CHECK(acknowledged IN(0,1)),
 UNIQUE(account_id,auth_epoch,action)
) STRICT;
CREATE TABLE deleted_account_effects (
 account_id TEXT NOT NULL, effect_id TEXT NOT NULL, kind TEXT NOT NULL,
 state TEXT NOT NULL, PRIMARY KEY(account_id,kind,effect_id)
) STRICT;
PRAGMA user_version=41;
