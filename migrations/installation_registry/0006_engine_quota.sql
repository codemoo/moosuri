-- Short-lived provider metadata requests, one per requesting installation administrator.
CREATE TABLE engine_quota_requests (
    account_id TEXT PRIMARY KEY, role_revision INTEGER NOT NULL, session_epoch INTEGER NOT NULL,
    probe_id TEXT NOT NULL UNIQUE, connection_revision TEXT NOT NULL, settings_revision INTEGER NOT NULL,
    requested_at INTEGER NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending','running','done')),
    observed_at INTEGER, metrics TEXT
) STRICT;
PRAGMA user_version=6;
