-- Original missions.payload and accounting remain untouched.
CREATE TABLE mission_acceptances (
 tenant_id TEXT NOT NULL, mission_id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>=1), request_id TEXT NOT NULL,
 operation TEXT NOT NULL, payload TEXT NOT NULL, first_step INTEGER NOT NULL CHECK(first_step>=1),
 PRIMARY KEY(tenant_id,mission_id,revision), UNIQUE(tenant_id,mission_id,request_id),
 FOREIGN KEY(tenant_id,mission_id) REFERENCES missions(tenant_id,id) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=52;
