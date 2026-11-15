-- Durable bounded sweep positions; no task authority, private content or new budget.
CREATE TABLE native_mail_settlement_cursor (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),
 read_rowid INTEGER NOT NULL DEFAULT 0 CHECK(read_rowid>=0),
 dependency_rowid INTEGER NOT NULL DEFAULT 0 CHECK(dependency_rowid>=0)
) STRICT;
INSERT INTO native_mail_settlement_cursor(singleton) VALUES(1);
PRAGMA user_version=68;
