-- New baseline. This schema does not import or execute legacy migrations.
CREATE TABLE store_meta (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    journal_seq INTEGER NOT NULL DEFAULT 0 CHECK(journal_seq >= 0),
    journal_head BLOB NOT NULL CHECK(length(journal_head) = 32)
) STRICT;
INSERT INTO store_meta(singleton, journal_head) VALUES (1, zeroblob(32));
CREATE TABLE tenants (
    id TEXT PRIMARY KEY NOT NULL
) STRICT;
CREATE TABLE accounts (
    id TEXT PRIMARY KEY NOT NULL,
    tenant_id TEXT UNIQUE NOT NULL REFERENCES tenants(id),
    name TEXT NOT NULL,
    email TEXT UNIQUE NOT NULL,
    role TEXT NOT NULL CHECK(role IN ('admin', 'member')),
    password_hash TEXT NOT NULL,
    must_change_password INTEGER NOT NULL CHECK(must_change_password IN (0,1)),
    temporary_expires_at INTEGER,
    auth_epoch INTEGER NOT NULL CHECK(auth_epoch >= 0),
    recovery_required INTEGER NOT NULL DEFAULT 0 CHECK(recovery_required IN (0,1)),
    UNIQUE(tenant_id, id),
    CHECK((must_change_password=1 AND temporary_expires_at IS NOT NULL) OR
          (must_change_password=0 AND temporary_expires_at IS NULL))
) STRICT;
CREATE TABLE sessions (
    digest BLOB PRIMARY KEY NOT NULL CHECK(length(digest) = 32),
    tenant_id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    auth_epoch INTEGER NOT NULL,
    password_change_only INTEGER NOT NULL CHECK(password_change_only IN (0,1)),
    expires_at INTEGER NOT NULL,
    FOREIGN KEY(tenant_id, account_id) REFERENCES accounts(tenant_id, id) ON DELETE CASCADE
) STRICT;
CREATE INDEX sessions_account ON sessions(tenant_id, account_id);
CREATE TABLE threads (
    tenant_id TEXT NOT NULL,
    id TEXT NOT NULL,
    account_id TEXT NOT NULL,
    title TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(tenant_id, id),
    FOREIGN KEY(tenant_id, account_id) REFERENCES accounts(tenant_id, id) ON DELETE CASCADE
) STRICT;
CREATE INDEX threads_recent ON threads(tenant_id, updated_at DESC, id);
PRAGMA user_version = 1;
