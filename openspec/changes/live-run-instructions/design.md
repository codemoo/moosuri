# Durable guidance and safe inference boundaries

Schema72 adds private per-run/attempt readiness markers and at most eight private
guidance receipts. The original UUID, exact UTF-8 text and original submit attempt
remain immutable. A separate execution-attempt field carries only guarded stable
mission continuations; ordinary explicit Retry does not replay instructions.
Existing owner, epoch, lease, provider, source and deadline guards remain.
Foreground and current-session missions retain the original login; durable
missions admit a currently validated same-owner login under existing mission
authority and retain its digest only as receipt audit data. The request names an attempt, never a changing per-token version.

POST `/api/v1/runs/{id}/instructions` accepts `{request_id,expected_attempt,text}`
with UUID, nonblank text and a2000-byte UTF-8 limit. Exact replay returns the
current receipt; different text/attempt under the same UUID conflicts. Received
POST results return202 and resolved POST receipts return200. GET
`/api/v1/runs/{id}/instructions/{request_id}` retrieves a durable uncertain ACK.
No model, new task or budget allocation is used by receipt admission. New requests
after terminal or outside the supported path conflict. Receipt state is
`received`, `applied` or `closed`, with exact text and nullable fixed reason.

Ordinary readiness is enabled only after local intent/credential branches have
selected a supported function continuation. Known read-only mission carriers can
accept while queued/planning/executing/verifying. Original Mail/Calendar effects,
native credential/mail stages, private reports/foreground artifact writes and unsupported plain
text paths are gated out. Mission inference strips effect-preparation/write tools
when live guidance is supported or present. Original grants are never enlarged.

Each model dispatch assigns only the prepared receipt prefix. The receipt stays
received during the stream and becomes applied only after a valid completed or
tool-continuation round is committed. Refusal, malformed or incomplete outcomes
close unconsumed guidance instead. Existing tool results precede new user-role
guidance in the provider continuation; network output cannot forge a user item.
Completed tools are not executed again. The next model round consumes the same
remote/tool/time budgets. A late received instruction transactionally blocks old
final completion, plan acceptance, mission input waiting and mission verification.
The same run continues; superseded visible final text becomes commentary.

RunView carries support, optional receipts and an optional instruction target.
The original completed one-off mission creation receipt can address only its
actual mission carrier. MissionView exposes the same target/receipts. Recurring
templates do not resolve an implicit current occurrence. Internal mission threads
remain unavailable through ordinary conversation APIs. Applied-only guidance is
projected once into the prior turn's next-turn model history without changing its
displayed user text. Received/closed guidance is excluded.

Terminal transitions, mission stopping and restore quarantine close received
receipts explicitly; deletion cascades with the original chat/mission thread.
Store and migration tests cover transactional final barriers, owner/session,
replay/mismatch, count/UTF-8/attempt limits, history, deletion, restore quarantine,
schema rollback and foreign-key cascades.
