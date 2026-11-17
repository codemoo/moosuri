-- One installation bot, independently authorized private user conversations.
CREATE TABLE telegram_bot (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL,
 bot_id INTEGER, username TEXT, token BLOB, offset INTEGER NOT NULL DEFAULT 0,
 status TEXT NOT NULL DEFAULT 'disabled', last_poll_at INTEGER
) STRICT;
INSERT INTO telegram_bot(singleton,revision) VALUES(1,0);
CREATE TABLE telegram_pairings (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, digest BLOB NOT NULL UNIQUE,
 session_digest BLOB NOT NULL REFERENCES sessions(digest) ON DELETE CASCADE,
 bot_revision INTEGER NOT NULL, expires_at INTEGER NOT NULL,
 timezone TEXT NOT NULL, notifications INTEGER NOT NULL CHECK(notifications IN(0,1)),
 PRIMARY KEY(tenant_id,account_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE telegram_bindings (
 tenant_id TEXT NOT NULL, account_id TEXT NOT NULL, id TEXT PRIMARY KEY,
 telegram_user_id INTEGER NOT NULL UNIQUE CHECK(telegram_user_id>0),
 chat_id INTEGER NOT NULL UNIQUE CHECK(chat_id>0),
 session_digest BLOB NOT NULL UNIQUE REFERENCES sessions(digest) ON DELETE CASCADE,
 bot_revision INTEGER NOT NULL, thread_id TEXT, timezone TEXT NOT NULL,
 notifications INTEGER NOT NULL CHECK(notifications IN(0,1)),
 notification_cursor INTEGER NOT NULL, last_delivery TEXT, created_at INTEGER NOT NULL,
 UNIQUE(tenant_id,account_id),
 FOREIGN KEY(tenant_id,account_id) REFERENCES accounts(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE telegram_turns (
 tenant_id TEXT NOT NULL, binding_id TEXT NOT NULL REFERENCES telegram_bindings(id) ON DELETE CASCADE,
 run_id TEXT NOT NULL, attempt INTEGER NOT NULL, settled INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(tenant_id,run_id,attempt),
 FOREIGN KEY(tenant_id,run_id) REFERENCES chat_runs(tenant_id,id) ON DELETE CASCADE
) STRICT;
CREATE TABLE telegram_outbox (
 tenant_id TEXT NOT NULL, id TEXT PRIMARY KEY, binding_id TEXT NOT NULL REFERENCES telegram_bindings(id) ON DELETE CASCADE,
 dedupe TEXT NOT NULL, body TEXT, run_id TEXT, attempt INTEGER, notification_id TEXT,
 position INTEGER NOT NULL DEFAULT 0, body_digest BLOB,
 state TEXT NOT NULL CHECK(state IN('queued','dispatched','accepted','unknown','rejected','cancelled')),
 nonce TEXT, part_bytes INTEGER NOT NULL DEFAULT 0, attempts INTEGER NOT NULL DEFAULT 0,
 ready_at INTEGER NOT NULL, created_at INTEGER NOT NULL, dispatched_at INTEGER,
 UNIQUE(binding_id,dedupe)
) STRICT;
CREATE INDEX telegram_outbox_ready ON telegram_outbox(state,ready_at);
PRAGMA user_version=75;
