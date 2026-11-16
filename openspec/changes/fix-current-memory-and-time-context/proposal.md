# Current memory and original-message time context

## Why

A large population of expired assertions consumes the semantic target selector's
64-row budget before expiry filtering, preventing a correction to a single current
fact. Ordinary chat also lacks a trusted original-message date/time reference,
forcing relative-date answers to guess when no Calendar context is available.
Two explicit read-only memory request forms additionally spend an unnecessary
mutation-classifier round before using existing read tools.

## What Changes

- Filter expired effective versions before the bounded candidate limit without
  reviving older assertions or weakening snapshot/current-authority checks.
- Supply the stored original-message UTC/local reference in trusted model context,
  preserve it through profile replacement and retries, and explicitly mark UTC
  fallback when older requests lack a timezone.
- Route narrowly recognized Korean memory lookups through normal read tools;
  quoted/mixed/unknown mutation cues keep the existing classifier route.

## Impact

F01/F02/F03 chat/context and personal-memory behavior. No schema migration,
provider/model/effort change, new tool authority or automatic inference retry.
Synthetic source/tests only; no live provider or production acceptance claim.
