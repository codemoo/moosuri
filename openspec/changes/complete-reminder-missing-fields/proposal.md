# Fixed-reminder missing-field replies

## Why
P5/F29 explicit reminder requests currently ask for complete time/content again when one field is missing. Preserve already supplied literal fields and allow a bounded immediately next answer without adding a model classifier or repeated approval.

## What Changes
Add a small persisted creation draft and deterministic next-answer completion under the same current owner/session/thread. Freeze supplied time/zone/reference; entirely missing time is anchored to the answer. Retire drafts on cancellation, unrelated replies/new commands, expiry, media or restore; keep exact request retry and atomic schedule receipt behavior.

## Impact
Store scheduler/chat and schema51 sidecar only; no provider/UI/type/API/dependency/CI changes. This is fixed-content creation, not general actions or task execution. [Contract](../../../docs/reminder-followups.md).
