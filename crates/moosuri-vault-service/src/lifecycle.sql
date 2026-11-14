CREATE TABLE lifecycle(id TEXT PRIMARY KEY,tenant TEXT NOT NULL,owner TEXT NOT NULL,installation TEXT NOT NULL,epoch INTEGER NOT NULL,action TEXT NOT NULL CHECK(action IN('revoke','delete'))) STRICT;
PRAGMA user_version=2;
