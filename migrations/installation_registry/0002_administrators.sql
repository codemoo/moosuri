-- Installation control only; never changes private assistant schema/roles.
CREATE TABLE installation_members (
 account_id TEXT PRIMARY KEY,tenant_id TEXT NOT NULL,volume_id TEXT NOT NULL,
 generation INTEGER NOT NULL CHECK(generation>0),
 role TEXT NOT NULL CHECK(role IN('member','admin')),
 role_revision INTEGER NOT NULL CHECK(role_revision>0),
 eligible INTEGER NOT NULL CHECK(eligible IN(0,1)),
 suspended INTEGER NOT NULL CHECK(suspended IN(0,1))
) STRICT;
CREATE TABLE installation_admin_bootstrap (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),request_id TEXT NOT NULL UNIQUE,
 account_id TEXT NOT NULL,tenant_id TEXT NOT NULL,volume_id TEXT NOT NULL,generation INTEGER NOT NULL
) STRICT;
CREATE TABLE installation_admin_operations (
 request_id TEXT PRIMARY KEY,fingerprint TEXT NOT NULL,result TEXT NOT NULL
) STRICT;
PRAGMA user_version=2;
