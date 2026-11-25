-- Member public-page watch price condition (schema 101): "tell me when the displayed KRW price on this
-- public page is at or below N won". Additive only: both columns are NULL for every ordinary
-- (content-change) watch, including all legacy rows, which keep their behaviour and request digests.
--
-- The band state (in / out of the price condition) needs no new column: for a price watch the existing
-- `fingerprint` holds a digest of the predicate result, so it commits atomically with the check row and
-- the notification, and a failed read never touches it.
ALTER TABLE member_public_watches ADD COLUMN price_won INTEGER
 CHECK(price_won IS NULL OR price_won BETWEEN 1 AND 999999999999);
ALTER TABLE member_public_watches ADD COLUMN price_inclusive INTEGER
 CHECK((price_won IS NULL)=(price_inclusive IS NULL) AND (price_inclusive IS NULL OR price_inclusive IN(0,1)));
PRAGMA user_version=101;
