-- Server-managed bounded profiles; no profile paths, cookies or input in SQLite.
ALTER TABLE browser_workspaces ADD COLUMN idle_until INTEGER NOT NULL DEFAULT 0;
ALTER TABLE browser_workspaces ADD COLUMN owner_session BLOB;
-- Pre-migration browsers were ephemeral and cannot be recovered.
PRAGMA user_version=53;
