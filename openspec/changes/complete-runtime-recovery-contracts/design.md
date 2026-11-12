## Context

SQLite is the state authority, but required attachment and artifact bytes may live outside it. Schedule claims also need concrete policies when clocks, outages or user edits intervene.

## Goals / Non-Goals

Restore all retained user data and execute only the intended occurrences. Keep a single-host writer model. Do not add a distributed storage/scheduler service or port old test files.

## Decisions

Publish immutable files durably before committing database references. Freeze snapshot, manifest and durable GC pins under a bounded writer fence, then copy/encrypt files outside the fence. Mark archives complete only after integrity checks. Apply the latest deletion journal before exposing a restored dataset; regenerated thumbnails are caches.

The scheduling policy table in docs/architecture.md specifies wall-clock versus UTC interval rules, skipped DST gaps, first-offset folds, latest-only recurring misfires, delayed valid one-shots, pause-period skipping and revision fencing. Fixed external sends retain their five-minute expiration. The writer serializes pause/edit with dispatch commitment; earlier commitments are in flight and need reconciliation.

## Risks / Trade-offs

Large backups can remain busy rather than holding the database indefinitely. Backups may retain historical encrypted bytes; current deletion visibility is enforced by the latest journal. A committed remote action cannot be recalled merely by pausing or editing a schedule.

## Migration Plan

Specify storage and commitment boundaries in P1, implement fake-clock scheduler cases in P5, authoritative media bytes in P7 and empty-data-directory restore in P9. The plan is complete only as documentation; runtime and failure checks below remain pending.

## Recovery key dependency

Use a standard public-recipient encryption format with a Rust library candidate, not a custom cipher. Normal backup writers retain only public recipients; an operator exports purpose-scoped private backup identities to an independent recovery kit. Require a small setup decrypt check before enabling unattended backups. Setup may be skipped without blocking normal app use.

The kit and independent recovery storage must supply all retained key epochs, backup target access and any journal-verification material without relying on the lost server or the vault being restored. The latest journal/head is maintained separately from the static kit. Outer decryption leaves inner user vault ciphertext intact. A clean-host test removes all original local secrets and exercises backups from before/after rotation, wrong/missing keys and missing latest journal.
