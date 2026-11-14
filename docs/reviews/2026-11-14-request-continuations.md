# Explicit request continuation acceptance — 2026-11-14

Schema42 stores bounded explicit mission/Calendar requests with literal subsequent
answers, preserving the original request, time basis, typed supplied values and target
identity. A completed immediately prior confirmation or actual task/Calendar receipt
can supply context in the same owner/thread/session/model selection, for at most
30 minutes and four clarification answers. A new action still needs the current
user's instruction. Revocation, intervening dialogue, stale revisions and restore
cannot recreate authority.

Calendar movement separates original target date from destination date and preserves
actual duration and local clock seconds when unspecified; DST gaps/folds clarify.
Missions preserve partial weekly/date scope across short time answers.

Independent review found a weekly request could become daily when a follow-up time
quote omitted the original weekday. Store now validates the complete original
request and literal answers independently of that short quote. The forged daily
proposal is rejected while a valid weekly proposal with a long task still succeeds.
Independent re-review accepted the repair without additional material findings.

Focused native checks and integrated Linux Store/Runtime continuation and Calendar,
HTTP Calendar, schema migration and actual encrypted restore checks passed. Current
React/TypeScript/build checks also passed. A Clippy-only Boolean simplification was
verified with the focused continuation cases. These are synthetic behavior and
failure checks, not a broad live-language score or actual Google-account acceptance.
The original read-only mission and primary timed solo-Calendar bounds remain.
