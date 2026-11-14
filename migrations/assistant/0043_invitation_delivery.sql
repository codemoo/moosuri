-- System invitation effects are not personal Gmail drafts. Payload is encrypted.
CREATE TABLE invitation_deliveries (
 id TEXT PRIMARY KEY,actor_id TEXT NOT NULL,target_id TEXT NOT NULL,
 target_epoch INTEGER NOT NULL,target_revision INTEGER NOT NULL,
 actor_epoch INTEGER NOT NULL,session_digest BLOB NOT NULL,
 sender_pin TEXT NOT NULL,request_digest BLOB NOT NULL,digest TEXT NOT NULL,
 from_address TEXT NOT NULL,to_address TEXT NOT NULL,subject TEXT NOT NULL,
 payload BLOB,state TEXT NOT NULL CHECK(state IN('created','delivery_pending','dispatch_committed','sent','delivery_unknown','failed_definite','expired','activated')),
 expires_at INTEGER NOT NULL,provider_message TEXT,
 FOREIGN KEY(actor_id) REFERENCES accounts(id),FOREIGN KEY(target_id) REFERENCES accounts(id)
) STRICT;
CREATE UNIQUE INDEX invitation_target ON invitation_deliveries(target_id,target_epoch);
PRAGMA user_version=43;
