-- Explicit new request allowances; historical accounting remains unknown.
CREATE TABLE recovery_forward_allocations (
 id TEXT PRIMARY KEY NOT NULL CHECK(length(id)=36),
 restore_id TEXT NOT NULL REFERENCES installation_restores(restore_id),
 installation_id TEXT NOT NULL CHECK(length(installation_id)=36),
 inference_calls INTEGER NOT NULL CHECK(inference_calls BETWEEN 1 AND 1000),
 stt_duration_ms INTEGER NOT NULL CHECK(stt_duration_ms BETWEEN 0 AND 3600000),
 created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE recovery_forward_dispatches (
 nonce TEXT PRIMARY KEY NOT NULL CHECK(length(nonce) BETWEEN 1 AND 256),
 allocation_id TEXT NOT NULL REFERENCES recovery_forward_allocations(id),
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN('chat','agent','memory','probe','stt','stt_probe')),
 duration_ms INTEGER NOT NULL CHECK(duration_ms BETWEEN 0 AND 120000),
 committed_at INTEGER NOT NULL,
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE INDEX recovery_forward_allocation_current ON recovery_forward_allocations(restore_id,installation_id,created_at,id);
CREATE INDEX recovery_forward_dispatch_allocation ON recovery_forward_dispatches(allocation_id);
CREATE TRIGGER recovery_forward_allocations_no_update BEFORE UPDATE ON recovery_forward_allocations
BEGIN SELECT RAISE(ABORT,'forward allocation immutable'); END;
CREATE TRIGGER recovery_forward_allocations_no_delete BEFORE DELETE ON recovery_forward_allocations
BEGIN SELECT RAISE(ABORT,'forward allocation immutable'); END;
CREATE TRIGGER recovery_forward_dispatches_no_update BEFORE UPDATE ON recovery_forward_dispatches
BEGIN SELECT RAISE(ABORT,'forward reservation immutable'); END;
CREATE TRIGGER recovery_forward_dispatches_no_delete BEFORE DELETE ON recovery_forward_dispatches
BEGIN SELECT RAISE(ABORT,'forward reservation immutable'); END;
PRAGMA user_version=49;
