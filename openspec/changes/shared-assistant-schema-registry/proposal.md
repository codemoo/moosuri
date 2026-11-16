# Shared assistant schema registry

The normal Store startup and encrypted cutover restore duplicated the assistant
migration list. A migration could reach normal startup while restore silently
missed it, as happened for schema72. Keep one ordered version/SQL registry while
retaining the two callers' admission, durability and quarantine contracts.

This is a P1/P9 maintainability repair. Current schema remains73; no SQL,
account/auth behavior, support floor or operational data changes are introduced.
