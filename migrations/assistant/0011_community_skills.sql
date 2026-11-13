-- Immutable public snapshots, private per-account activation. No credentials or traces.
CREATE TABLE community_packages (
 digest TEXT PRIMARY KEY NOT NULL CHECK(length(digest)=64),
 source TEXT NOT NULL, source_commit TEXT NOT NULL CHECK(length(source_commit)=40),
 package TEXT NOT NULL
) STRICT;
CREATE TABLE community_installs (
 tenant_id TEXT NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
 package_id TEXT NOT NULL,
 active_digest TEXT REFERENCES community_packages(digest),
 previous_digest TEXT REFERENCES community_packages(digest),
 managed INTEGER NOT NULL DEFAULT 0 CHECK(managed IN (0,1)),
 enabled INTEGER NOT NULL DEFAULT 0 CHECK(enabled IN (0,1)),
 hidden INTEGER NOT NULL DEFAULT 0 CHECK(hidden IN (0,1)),
 revision INTEGER NOT NULL CHECK(revision>0),
 PRIMARY KEY(tenant_id,package_id)
) STRICT;
CREATE TABLE community_run_revocations (
 tenant_id TEXT NOT NULL, run_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=11;
