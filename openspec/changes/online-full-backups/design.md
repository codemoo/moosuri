# Design

One Store job freezes committed SQLite pages plus exact fully applied journals
into an owner-private temporary stage. Existing size/deadline caps apply. Return
the stage and release the writer before existing export validation and age
encryption. Refactor the stopped path's immutable export inputs to share its
manifest/Push stripping/publication code without changing either format.

An optional serve worker validates the private destination and configured public
recipient. It runs immediately and serially after each configured interval,
drains in-flight work on shutdown and never overwrites or deletes old exports.
No HTTP export route, automatic retention, SSH transfer or latestness authority
is introduced. The existing receiver still pins its original baseline.
