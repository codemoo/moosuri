CREATE TABLE google_setup (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 revision INTEGER NOT NULL CHECK(revision>0),
 client_id TEXT NOT NULL, redirect_uri TEXT NOT NULL, secret BLOB NOT NULL
) STRICT;
ALTER TABLE mail_oauth_sessions ADD COLUMN app_revision INTEGER;
ALTER TABLE mail_oauth_sessions ADD COLUMN client_id TEXT;
ALTER TABLE mail_oauth_sessions ADD COLUMN redirect_uri TEXT;
ALTER TABLE mail_oauth_sessions ADD COLUMN setup_return INTEGER NOT NULL DEFAULT 0 CHECK(setup_return IN(0,1));
ALTER TABLE mail_connections ADD COLUMN oauth_client_id TEXT;
PRAGMA user_version=10;
