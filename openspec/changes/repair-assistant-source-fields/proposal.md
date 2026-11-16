# Read ordinary source fields without losing evidence

## Why

The actual assistant rejected a user's literal field name `재료 단가` when the
saved source serialized it as `재료 단가: 13.75`. It then spent extra model rounds
reading and calculating manually. The attachment prompt also contradicted the
existing PDF text extraction capability by declaring all PDF reading unavailable.

## What changes

- Accept one explicit field separator after a literal unique source label.
- Retain exact source byte ranges, arithmetic receipts, ownership and budgets.
- Describe supplied PDF extraction accurately, retaining no-OCR and no-effect
  boundaries.

## Impact

Source-bound calculation and its existing learned route, model tool guidance and
media-answer instructions. No new external action or model setting.
