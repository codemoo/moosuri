# Resolve shared conversational civil time

## Why

Reminder, mission and Calendar requests must resolve the same literal civil day
and clock against one captured instant and IANA zone. Inconsistent interpretation
can save the wrong week, turn one-shot work into recurrence, or accept an invalid
clock by ignoring its malformed suffix.

## What Changes

Use one deterministic Store oracle for bounded Korean/English relative days,
weekdays, current/next ISO week and native-word clocks. Preserve existing
continuation anchors and authority; reject ambiguous/missing clocks and one-shot
DST folds/gaps. Bind model temporal quotes to complete overlapping source spans
and retain weekly versus one-shot intent. Extend cheap routing cues without an
ordinary-chat classification call.

## Impact

P5/F29 reminder, delegated mission and Calendar admission. No schema, dependency,
lockfile, public API, scheduler recurrence policy, new approval or live-account
change. Existing draft/continuation budgets and storage/owner fences remain.
