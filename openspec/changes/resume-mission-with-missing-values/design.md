## Context

Acceptance changes intentionally fence old receipts. A missing typed value must retain their acceptance floor and must not use that edit route. Existing connection waits own finite runnable admission.

## Decisions

A completed model round can return `input_request` instead of a candidate. The writer checks its completed step, accepted action kind, missing field, original operation, remaining budget and stable boundary before persisting one UUID question and retiring the lease. Only consumed round text is cleared; checkpoints and receipts stay.

Replies have request UUID, exact wait UUID, mission revision and one typed value. The authenticated owner and epoch are required. Identical reply retry returns current state, conflicting UUID payload fails. One field can be filled once per acceptance. Ready input rows join connection/native waits in admitted order; explicit paused work still reserves the owner runnable slot. Promotion uses ordinary mission/session/dependency fences, without creating UserContext or fetching a new profile.

The action path checks original intent and operation before using typed parameters. A literal original recipient/date/clock/duration cannot be replaced. Replies cannot create CC/BCC/reply/Calendar target/title/account/all-day/end authority. Calendar date answers use ISO civil dates, start answers use HH:MM, duration answers use1–1440minutes. All fields retain existing Calendar validation and journal. No approval or mail send tool is added.

## Limits

Only one-off admitted mail/Calendar actions support these waits. General report/source clarifications, source-conflict selection, Calendar target ambiguity, recurrence and free-form steering are unsupported here. Conservative existing-value detection can decline a clarification rather than reinterpret a supplied value. The current schedule/occurrence notification helper does not support one-off mission action-needed notices; the original task card is durable across refresh. Waiting time consumes the original deadline; no model polling occurs.

## Recovery

Schema69 is included in normal-open and restored snapshot migrations. Restore clears input waits/replies and quarantines unfinished mission grants. Mission deletion clears question/reply data through the original run's FK and explicit reply cleanup. No new effect journal is created.
