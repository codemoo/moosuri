CREATE TABLE compound_report_adoptions (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, run_id TEXT NOT NULL,
 artifact_id TEXT NOT NULL, artifact_revision INTEGER NOT NULL,
 -- NULL exists only inside the first adoption transaction before qualification.
 -- Successful adoption binds this immutable structural route before commit.
 recipe_route TEXT,
 PRIMARY KEY(tenant_id,account_id,run_id),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=64;
