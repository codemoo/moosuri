## Why
F32 has inbox, quiet hours and per-source mute but no current-evidence suggestion
policy or shared product-topic mute. Date-free saved followups have a concrete,
truthful next step: choose when to review the actual record.

## What Changes
- One fixed `followup_review` topic, literal reason/current owned artifact link,
  one suggestion publication per record and at most5 pending/visible cards.
- Explicit-save admission only; no migration/idle backfill, classifier or execution.
- Independent card hiding and category-wide mute/unmute through the existing
  notification preference, inbox, quiet-hour and opaque Push mechanisms.
- Schema0057 normal/restore hooks and atomic evidence/edit/account cleanup.

## Impact
Artifact saves/invalidation, internal fixed reminders, source/Push eligibility,
React inbox cards and generated API contracts. Dated followup alerts remain
independent; no dependencies or provider permissions are added.
