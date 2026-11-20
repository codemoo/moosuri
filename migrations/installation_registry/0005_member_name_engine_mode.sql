-- Verified session display name and public broker connection mode.
ALTER TABLE installation_members ADD COLUMN display_name TEXT;
ALTER TABLE engine_settings ADD COLUMN connection_mode TEXT
 CHECK(connection_mode IN ('api', 'chatgpt_plan', 'claude_code'));
PRAGMA user_version=5;
