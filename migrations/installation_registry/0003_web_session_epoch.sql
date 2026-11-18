-- Epoch zero preserves existing gateway cookies for members with no previous suspension.
ALTER TABLE allocations ADD COLUMN session_epoch INTEGER NOT NULL DEFAULT 0 CHECK(session_epoch >= 0);
UPDATE allocations SET session_epoch=1 WHERE account_id IN (
  SELECT account_id FROM installation_members WHERE suspended=1
  UNION SELECT json_extract(result,'$.account_id') FROM installation_admin_operations
    WHERE json_extract(result,'$.suspended')=1
);
PRAGMA user_version=3;
