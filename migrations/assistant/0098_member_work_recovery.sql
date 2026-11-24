-- Only one lease may record a proposed plan. An interrupted classifier has no external effects.
ALTER TABLE member_conversation_interpretations ADD COLUMN lease TEXT;
ALTER TABLE member_conversation_interpretations ADD COLUMN lease_until INTEGER NOT NULL DEFAULT 0;
ALTER TABLE member_conversation_interpretations ADD COLUMN attempt INTEGER NOT NULL DEFAULT 1;
UPDATE member_conversation_interpretations SET plan='null' WHERE outcome IS NOT NULL;
-- A thread links to a domain operation and its actual state transitions. No copied mail, page or event text.
CREATE TABLE member_linked_work (
 id TEXT PRIMARY KEY, kind TEXT NOT NULL CHECK(kind IN ('application','shopping','mail_draft','calendar')),
 operation_id TEXT NOT NULL, origin_turn_id TEXT NOT NULL,
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, volume_id TEXT NOT NULL, generation INTEGER NOT NULL,
 thread_id TEXT NOT NULL, auth_epoch INTEGER NOT NULL, created_at INTEGER NOT NULL, admitted_order INTEGER NOT NULL UNIQUE,
 UNIQUE(tenant_id,account_id,kind,operation_id),
 FOREIGN KEY(tenant_id,thread_id) REFERENCES threads(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE member_linked_work_events (
 work_id TEXT NOT NULL REFERENCES member_linked_work(id) ON DELETE CASCADE,
 sequence INTEGER PRIMARY KEY, state TEXT NOT NULL, at INTEGER NOT NULL
) STRICT;
CREATE INDEX member_linked_work_events_page ON member_linked_work_events(work_id,sequence DESC);
CREATE TRIGGER member_linked_research_action AFTER INSERT ON member_research_outcome_actions BEGIN
 INSERT INTO member_linked_work_events(work_id,state,at)
 SELECT l.id,NEW.action,unixepoch() FROM member_linked_work l JOIN member_research_outcomes o
 ON l.operation_id=o.id AND l.tenant_id=o.tenant_id AND l.account_id=o.account_id AND l.kind=o.kind
 WHERE o.id=NEW.outcome_id;
END;
CREATE TRIGGER member_linked_mail_edit AFTER UPDATE OF phase,revision,action_id,provider_draft_id ON member_mail_work
WHEN NEW.phase!=OLD.phase OR NEW.revision!=OLD.revision OR NEW.action_id IS NOT OLD.action_id OR NEW.provider_draft_id IS NOT OLD.provider_draft_id BEGIN
 INSERT INTO member_linked_work_events(work_id,state,at) SELECT id,
 CASE WHEN NEW.phase='withdrawn' THEN 'withdrawn' WHEN NEW.provider_draft_id IS NOT OLD.provider_draft_id THEN 'draft_saved'
 WHEN NEW.revision!=OLD.revision THEN 'revised' ELSE 'preview' END,unixepoch() FROM member_linked_work
 WHERE kind='mail_draft' AND tenant_id=NEW.tenant_id AND account_id=NEW.account_id AND operation_id=NEW.id;
END;
CREATE TRIGGER member_linked_mail_delivery AFTER UPDATE OF state ON mail_actions WHEN NEW.state!=OLD.state BEGIN
 INSERT INTO member_linked_work_events(work_id,state,at)
 SELECT l.id,NEW.state,unixepoch() FROM member_linked_work l JOIN member_mail_work w
 ON w.tenant_id=l.tenant_id AND w.account_id=l.account_id AND w.id=l.operation_id
 WHERE l.kind='mail_draft' AND w.action_id=NEW.id AND w.tenant_id=NEW.tenant_id AND w.account_id=NEW.account_id;
END;
CREATE TRIGGER member_linked_calendar_dispatch AFTER INSERT ON member_calendar_actions BEGIN
 INSERT INTO member_linked_work_events(work_id,state,at) SELECT id,NEW.state,unixepoch()
 FROM member_linked_work WHERE kind='calendar' AND tenant_id=NEW.tenant_id AND account_id=NEW.account_id AND operation_id=NEW.request_id;
END;
CREATE TRIGGER member_linked_calendar_settle AFTER UPDATE OF state ON member_calendar_actions WHEN NEW.state!=OLD.state BEGIN
 INSERT INTO member_linked_work_events(work_id,state,at) SELECT id,NEW.state,unixepoch()
 FROM member_linked_work WHERE kind='calendar' AND tenant_id=NEW.tenant_id AND account_id=NEW.account_id AND operation_id=NEW.request_id;
END;
-- Only a reading from the member's browser can substantiate a price, deadline or receipt.
ALTER TABLE personal_browser_jobs ADD COLUMN research_reading BLOB;
CREATE TRIGGER member_reading_output_deleted AFTER DELETE ON chat_output
WHEN OLD.item IN ('browser-answer','browser-observation') BEGIN
 UPDATE personal_browser_jobs SET research_reading=NULL WHERE tenant_id=OLD.tenant_id AND run_id=OLD.run_id;
END;
PRAGMA user_version=98;
