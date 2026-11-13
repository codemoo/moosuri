-- Private immutable UTF-8 attachments. Calendar schema26 integrates separately.
ALTER TABLE media_objects ADD COLUMN filename TEXT;
ALTER TABLE media_objects ADD COLUMN media_type TEXT;
ALTER TABLE media_objects ADD COLUMN extracted_text TEXT;
PRAGMA user_version=27;
