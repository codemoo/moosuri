ALTER TABLE accounts ADD COLUMN status TEXT NOT NULL DEFAULT 'active'
    CHECK(status IN ('active','suspended'));
ALTER TABLE accounts ADD COLUMN management_revision INTEGER NOT NULL DEFAULT 1
    CHECK(management_revision>0);
CREATE TABLE account_management_receipts (
    actor_id TEXT NOT NULL REFERENCES accounts(id),
    request_id TEXT NOT NULL,
    target_id TEXT NOT NULL REFERENCES accounts(id),
    request_digest BLOB NOT NULL CHECK(length(request_digest)=32),
    resulting_revision INTEGER NOT NULL CHECK(resulting_revision>0),
    created_at INTEGER NOT NULL,
    PRIMARY KEY(actor_id,request_id)
) STRICT;
-- Account lifecycle follows personal artifacts16 and fixed reminders17.
PRAGMA user_version=18;
