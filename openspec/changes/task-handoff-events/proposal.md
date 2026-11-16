# Keep original task conversations current

A chat-origin mission can finish with no outstanding instruction receipt. Its
original conversation then retains an active instruction target: the next text
is sent to a finished carrier and rejected until the page refreshes.

Publish changed original-conversation targets from existing lifecycle writers,
independently of receipt state. Retain the same task/carrier, owner, attempt and
budgets. No schema, DTO, approval, model dispatch or SSE-read mutation is added.
