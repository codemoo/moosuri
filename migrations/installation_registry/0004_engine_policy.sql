-- Connection IDs are immutable even after an operator switches away from one.
CREATE TABLE engine_connections (id TEXT PRIMARY KEY) STRICT;
CREATE TABLE engine_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    revision INTEGER NOT NULL CHECK(revision>0),
    connection_revision TEXT NOT NULL, catalog TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK(enabled IN (0,1)),
    default_choice TEXT, default_connection TEXT,
    CHECK ((default_choice IS NULL)=(default_connection IS NULL))
) STRICT;
CREATE TABLE engine_bindings (
    account_id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, volume_id TEXT NOT NULL,
    generation INTEGER NOT NULL, choice TEXT, choice_connection TEXT,
    CHECK ((choice IS NULL)=(choice_connection IS NULL))
) STRICT;
PRAGMA user_version=4;
