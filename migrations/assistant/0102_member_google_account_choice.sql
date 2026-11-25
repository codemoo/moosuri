-- A member explicitly using their own vault login selects the member's email
-- for this one consent. Existing and ordinary account-chooser flows remain free.
CREATE TABLE member_google_account_choices (
 state_hash BLOB PRIMARY KEY NOT NULL REFERENCES member_google_oauth(state_hash) ON DELETE CASCADE
) STRICT;
PRAGMA user_version=102;
