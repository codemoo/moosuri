-- Completed probes remain addressable while the same administrator requests another read.
ALTER TABLE engine_quota_requests RENAME TO engine_quota_requests_v6;
CREATE TABLE engine_quota_requests (
    probe_id TEXT PRIMARY KEY, account_id TEXT NOT NULL, generation INTEGER NOT NULL,
    role_revision INTEGER NOT NULL, session_epoch INTEGER NOT NULL,
    connection_revision TEXT NOT NULL, settings_revision INTEGER NOT NULL,
    requested_at INTEGER NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending','running','done')),
    observed_at INTEGER, metrics TEXT
) STRICT;
CREATE UNIQUE INDEX engine_quota_active ON engine_quota_requests
    (account_id,generation,role_revision,session_epoch,connection_revision,settings_revision)
    WHERE state IN ('pending','running');
CREATE INDEX engine_quota_expiry ON engine_quota_requests(requested_at);

-- v6 pinned the role revision and epoch. Recover its generation only for a still-valid owner.
INSERT INTO engine_quota_requests
    (probe_id,account_id,generation,role_revision,session_epoch,connection_revision,settings_revision,
     requested_at,state,observed_at,metrics)
SELECT q.probe_id,q.account_id,a.generation,q.role_revision,q.session_epoch,q.connection_revision,
       q.settings_revision,q.requested_at,q.state,q.observed_at,q.metrics
FROM engine_quota_requests_v6 q
JOIN allocations a ON a.account_id=q.account_id AND a.session_epoch=q.session_epoch
JOIN installation_members m ON m.account_id=a.account_id AND m.generation=a.generation
    AND m.tenant_id=a.tenant_id AND m.volume_id=a.volume_id AND m.role_revision=q.role_revision
WHERE a.route_state='ready' AND m.role='admin' AND m.eligible=1 AND m.suspended=0
    AND q.requested_at>unixepoch()-30;
DROP TABLE engine_quota_requests_v6;
PRAGMA user_version=7;
