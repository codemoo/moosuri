-- A durable owner grant to read ONE observed Gmail thread until its next reply.
-- Leases are independent of login sessions; no synthetic UserContext is created.
CREATE TABLE mail_reply_watches (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, id TEXT NOT NULL,
 request_id TEXT NOT NULL, request_digest BLOB NOT NULL,
 thread_id TEXT NOT NULL, origin_run TEXT, origin_action TEXT,
 auth_epoch INTEGER NOT NULL, connection_id TEXT NOT NULL, connection_epoch INTEGER NOT NULL,
 app_revision INTEGER, client_id TEXT NOT NULL, provider_thread TEXT NOT NULL,
 baseline_ids TEXT NOT NULL, seen_ids TEXT NOT NULL,
 title TEXT NOT NULL, mailbox TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN('active','replied','stopped','expired','blocked','withdrawn')),
 cadence_seconds INTEGER NOT NULL CHECK(cadence_seconds BETWEEN 300 AND 86400),
 expires_at INTEGER NOT NULL, next_due INTEGER, last_checked_at INTEGER,
 failures INTEGER NOT NULL DEFAULT 0, reason TEXT,
 lease_nonce BLOB, lease_deadline INTEGER, replies TEXT NOT NULL DEFAULT '[]',
 notification_id TEXT, created_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,id), UNIQUE(tenant_id,account_id,request_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE UNIQUE INDEX mail_reply_watch_active ON mail_reply_watches(tenant_id,account_id,connection_id,provider_thread) WHERE state='active';
CREATE INDEX mail_reply_watch_due ON mail_reply_watches(next_due) WHERE state='active';
CREATE TABLE mail_reply_watch_requests (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, request_id TEXT NOT NULL,
 request_digest BLOB NOT NULL, watch_id TEXT NOT NULL,
 PRIMARY KEY(tenant_id,account_id,request_id),
 FOREIGN KEY(tenant_id,watch_id) REFERENCES mail_reply_watches(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TRIGGER mail_reply_watch_delete BEFORE DELETE ON mail_reply_watches BEGIN
 UPDATE local_notifications SET dismissed=1,push_suppressed=1,message='메일 답장 감시가 삭제되었습니다.' WHERE tenant_id=OLD.tenant_id AND schedule_id=OLD.id;
 DELETE FROM notification_source_preferences WHERE tenant_id=OLD.tenant_id AND account_id=OLD.account_id AND schedule_id=OLD.id;
END;
PRAGMA user_version=74;
