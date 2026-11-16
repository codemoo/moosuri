# Consolidate mission capacity decisions

## Why

New admission, connection/input/device waits and explicit resume duplicate the
same owner slot decisions. Their distinction matters: applying the new-work global
limit to existing transitions would strand already admitted work at capacity.

## What Changes

Move the global new-mission limit and owner waiting/runnable predicates into one
mission-owned module. Preserve the separate new-work, existing-wait and resume
entry points, original admission order, paused occupancy and native chat exemption.

## Impact

F27–30, F34, F69. Internal SQLite policy consolidation; no API, migration,
permission prompt, new tool or model call. Automatic CI remains disabled.
