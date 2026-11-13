-- Opt-in Web Push and ordered per-device enrollment after schema22.
CREATE TABLE push_vapid_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),public_key TEXT NOT NULL,contact TEXT NOT NULL,retired INTEGER NOT NULL DEFAULT 0 CHECK(retired IN(0,1))) STRICT;
CREATE TABLE push_subscriptions(
 binding_id TEXT PRIMARY KEY,tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,device_id TEXT NOT NULL,
 endpoint_hash BLOB UNIQUE,credentials BLOB,auth_epoch INTEGER NOT NULL,session_digest BLOB,
 state TEXT NOT NULL CHECK(state IN('active','off','reconnect','restore_required')),
 cursor_after INTEGER NOT NULL,last_scan INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL,last_outcome TEXT,
 UNIQUE(tenant_id,account_id,device_id),FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id)
) STRICT;
CREATE TABLE push_queue(
 tenant_id TEXT NOT NULL,id TEXT NOT NULL,account_id TEXT NOT NULL,binding_id TEXT NOT NULL,notification_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('queued','claimed','dispatched','accepted','expired','rejected','unknown','suppressed','cancelled','quarantined')),
 attempt INTEGER NOT NULL DEFAULT 0 CHECK(attempt BETWEEN 0 AND 4),nonce TEXT,ready_at INTEGER NOT NULL,deadline INTEGER,expires_at INTEGER NOT NULL,
 created_at INTEGER NOT NULL,finished_at INTEGER,reason TEXT,
 PRIMARY KEY(tenant_id,id),UNIQUE(binding_id,notification_id),FOREIGN KEY(tenant_id,notification_id) REFERENCES local_notifications(tenant_id,id)
) STRICT;
CREATE INDEX push_due ON push_queue(state,ready_at);
CREATE INDEX push_expiry ON push_queue(state,expires_at);
CREATE TABLE push_owners(tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,last_claim INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(tenant_id,account_id)) STRICT;
CREATE TABLE push_worker_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),counter INTEGER NOT NULL DEFAULT 0) STRICT;
INSERT INTO push_worker_state(singleton) VALUES(1);
CREATE TABLE push_enrollment_receipts(tenant_id TEXT NOT NULL,account_id TEXT NOT NULL,request_id TEXT NOT NULL,request_digest BLOB NOT NULL,binding_id TEXT,device_id TEXT NOT NULL,operation_revision INTEGER NOT NULL CHECK(operation_revision BETWEEN 1 AND 9007199254740991),PRIMARY KEY(tenant_id,account_id,request_id)) STRICT;
CREATE INDEX push_intent_order ON push_enrollment_receipts(device_id,operation_revision);
PRAGMA user_version=23;
