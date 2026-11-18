-- NULL metadata means not yet fetched (legacy probes); [] confirms no supported efforts.
ALTER TABLE claude_code_probes ADD COLUMN supported_efforts TEXT;
ALTER TABLE personal_claude_selections ADD COLUMN effort TEXT CHECK(effort IS NULL OR effort IN ('low','medium','high','xhigh'));
PRAGMA user_version=76;
