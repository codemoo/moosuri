# Design

The existing next-sequence/session/account/epoch/TTL and attachment fences stay
unchanged. Independent recognized commands still take precedence. A short literal
Korean activity label may end in nominal 기 and contain only alphanumeric words,
spaces or hyphens, at most160bytes and six words. Existing outer meta/question,
conditional and requested-command guards remain. Shared civil-time cues are checked at word boundaries throughout an unlabelled reply so a conflicting time cannot be saved under the prior schedule. Explicit
내용 labels continue to carry arbitrary bounded literal text; this is an inbox
reminder message, never a delegated executable task.

Cancellation normalizes whitespace only for a full current draft reply. Fixed
cancellation forms support polite spacing and a small dismissal prefix set.
Named saved-reminder commands and quoted/conditional text do not acquire draft
cancellation semantics. Existing schedules remain untouched. Closure and content
completion use the existing submission transaction and request replay receipt.

No additional inference, temporal reinterpretation or permission prompt is added.
The reply parser is separated from draft-time filling and lifecycle transactions.
