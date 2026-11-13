CREATE TABLE installation_cutover (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 phase TEXT NOT NULL CHECK(phase IN('active','sealed','checkpoint_ready')),
 seal TEXT CHECK(seal IS NULL OR length(CAST(seal AS BLOB))<=16384),
 checkpoint TEXT CHECK(checkpoint IS NULL OR length(CAST(checkpoint AS BLOB))<=65536),
 CHECK((phase='active' AND seal IS NULL AND checkpoint IS NULL)
 OR (phase='sealed' AND seal IS NOT NULL AND checkpoint IS NULL)
 OR (phase='checkpoint_ready' AND seal IS NOT NULL AND checkpoint IS NOT NULL))
) STRICT;
INSERT INTO installation_cutover VALUES(1,'active',NULL,NULL);
CREATE TRIGGER installation_cutover_monotonic BEFORE UPDATE ON installation_cutover
WHEN NOT ((OLD.phase='active' AND NEW.phase='sealed' AND NEW.singleton=1)
 OR (OLD.phase='sealed' AND NEW.phase='checkpoint_ready' AND NEW.singleton=1 AND NEW.seal=OLD.seal))
BEGIN SELECT RAISE(ABORT,'cutover transition forbidden'); END;
CREATE TRIGGER installation_cutover_no_delete BEFORE DELETE ON installation_cutover
BEGIN SELECT RAISE(ABORT,'cutover deletion forbidden'); END;
-- Applies after artifacts16, schedules17 and account management18.
PRAGMA user_version=19;
