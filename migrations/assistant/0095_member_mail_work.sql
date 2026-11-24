-- Member-private Gmail research -> local reply draft. The cipher holds the cited
-- source metadata and draft text; it never enters chat history, recall or memory.
-- action_* points at an ordinary mail_actions row (the standard final-content
-- approval). provider_* records only a receipt returned by the Gmail adapter.
CREATE TABLE member_mail_work (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, id TEXT NOT NULL,
 thread_id TEXT NOT NULL, read_request_id TEXT NOT NULL,
 volume_id TEXT NOT NULL, generation INTEGER NOT NULL CHECK(generation>0),
 auth_epoch INTEGER NOT NULL,
 source_connection_id TEXT NOT NULL, source_epoch INTEGER NOT NULL CHECK(source_epoch>0),
 app_revision TEXT NOT NULL, source_mailbox TEXT NOT NULL, source_message_id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0), digest TEXT NOT NULL CHECK(length(digest)=64),
 phase TEXT NOT NULL CHECK(phase IN('local_draft','withdrawn')),
 cipher BLOB,
 action_id TEXT, action_revision INTEGER, action_digest TEXT, action_work_revision INTEGER,
 mail_connection_id TEXT, mail_connection_epoch INTEGER,
 provider_attempt TEXT, provider_draft_id TEXT, provider_draft_message_id TEXT, provider_draft_digest TEXT,
 created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
 PRIMARY KEY(tenant_id,account_id,id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE,
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE,
 CHECK((phase='local_draft' AND cipher IS NOT NULL) OR (phase='withdrawn' AND cipher IS NULL)),
 CHECK((action_id IS NULL AND action_revision IS NULL AND action_digest IS NULL AND action_work_revision IS NULL AND mail_connection_id IS NULL AND mail_connection_epoch IS NULL)
  OR (action_id IS NOT NULL AND action_revision>0 AND action_digest IS NOT NULL AND action_work_revision>0 AND mail_connection_id IS NOT NULL AND mail_connection_epoch>0)),
 CHECK(provider_draft_id IS NULL OR (provider_draft_message_id IS NOT NULL AND provider_draft_digest IS NOT NULL))
) STRICT;
CREATE INDEX member_mail_work_thread ON member_mail_work(tenant_id,account_id,thread_id,created_at DESC,id);
-- Revocation and cancellation share the commit that withdraws the source.
CREATE TRIGGER member_mail_source_withdrawn
AFTER UPDATE OF phase ON member_google_thread_reads
WHEN NEW.phase = 'withdrawn' AND OLD.phase != 'withdrawn'
BEGIN
 UPDATE mail_send_evidence SET withdrawn=1
 WHERE tenant_id=NEW.tenant_id AND action_id IN (
   SELECT action_id FROM member_mail_work WHERE tenant_id=NEW.tenant_id
   AND account_id=NEW.account_id AND read_request_id=NEW.request_id);
 UPDATE mail_actions SET version=version+1,state='cancelled',content=NULL,raw=NULL
 WHERE tenant_id=NEW.tenant_id AND account_id=NEW.account_id
 AND state IN ('pending_approval','approved') AND id IN (
   SELECT action_id FROM member_mail_work WHERE tenant_id=NEW.tenant_id
   AND account_id=NEW.account_id AND read_request_id=NEW.request_id);
 UPDATE member_mail_work SET phase='withdrawn',cipher=NULL,updated_at=unixepoch()
 WHERE tenant_id=NEW.tenant_id AND account_id=NEW.account_id
 AND read_request_id=NEW.request_id AND phase='local_draft';
END;
PRAGMA user_version=95;
