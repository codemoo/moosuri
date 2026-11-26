-- NULL preserves schema 102's member-email requirement for pending choices.
ALTER TABLE member_google_account_choices ADD COLUMN selected_email TEXT
 CHECK(selected_email IS NULL OR (length(selected_email) BETWEEN 3 AND 254));
PRAGMA user_version=103;
