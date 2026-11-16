# Everyday reminder and unsupported-action outcomes

## Why
A frozen synthetic everyday sequence created a reminder but could not postpone
or cancel it with natural follow-up wording. A separate unsupported booking
request failed without explanation because the model schema allowed prose for a
Store reason-code field.

## What Changes
- Resolve bounded relative reminder changes against the actual owned schedule.
- Accept Korean topic particles for reminder controls, retaining target/replay guards.
- Share clarification reason codes between the model schema and Store admission.
- Preserve failures, prove final saved outcomes and report real-model rechecks separately.

## Impact
Rust reminder admission, semantic mission schema, focused outcome tests and docs.
No migration, provider/model default, external send authority or new user approval.
