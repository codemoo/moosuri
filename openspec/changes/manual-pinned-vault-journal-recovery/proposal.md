## Why

A live snapshot cannot preserve later deletions or nonce/key effects by itself.
Recovery needs independently selected journal evidence without broadening normal
service crash-tail admission or assuming current/latest history.

## What changes

Add distinct encrypted full journal checkpoint and non-sealing manual export.
Historical restore requires an independently retained head hash, same source,
exact snapshot prefix and typed replay. Preserve later effects, quarantine
sessions/pending authority and bind both inputs in fresh-root provenance. Existing
sealed and plain snapshot contracts remain compatible; off-host transport is separate.
