# Durable text chat

The web app can send plain-text questions through a verified shared connection.
Choose a model in `/settings/model`; an installation default is also supported.
A new thread is created on the first message; its title uses that question’s opening text. Each account sees its own history,
model choice and usage. The administrator usage page shows counters, never text.

## Conversation behavior

- Submitted text is saved before dispatch. The browser retains a request UUID if
  the acknowledgement is lost, so sending that request again cannot duplicate it.
- Streaming is independent of the browser connection. Closing a tab does not stop
  the model task. Returning loads saved output and resumes committed events.
- Cancel stops accepting more visible output. Retry starts another attempt on the
  same question and preserves the composer draft. It keeps the original verified
  provider revision, model and effort; it never silently selects a fallback.
- Only the latest turn can be retried while conversation branching is unavailable.
  A cancelled/failed attempt can also be followed by a new question.
- Server restart marks interrupted dispatched work as `outcome_unknown`; it never
  automatically calls the model again. The interface offers an explicit retry.
- Deleting a conversation removes its text and event history and fences late model
  results. A minimal usage receipt remains without prompt or answer content.

Failed, incomplete, refused, cancelled and unknown outcomes remain distinct. Partial
answers remain readable with their status and are not reused as successful history.
Reported model identity must exactly match the selected ID. Unknown alias mappings
are not guessed. Commentary/answer phase is preserved when supplied by the provider.

## Bounds

One active run per thread and one dispatched call per account; at most two calls
run globally. Admission allows eight queued turns per account and64 globally.
A question is limited to8,192 UTF-8 bytes, with a64 KiB JSON envelope to accommodate
escaping. A call requests at most4,096 output tokens, has a180-second deadline,
and stores at most256 KiB across128 output items. Explicit attempts are capped at32.
These are engineering limits, not monetary budgets or price estimates.

Context contains the latest whole successful turns that fit the message/byte caps.
It stops at an oversized turn, without bringing back even older context past that
gap. An omission note is explicit. Failed partial text is excluded. These are chat
history rules, not persistent personal memory.

## API and storage

Authenticated requests assert the expected account in `X-Moosuri-Account`.
Session authority supplies the tenant; caller-provided identifiers cannot change it.

| Endpoint | Operation |
| --- | --- |
| `GET /api/v1/threads/{id}/messages?before=N` |20 chronological turns and a same-snapshot event cursor |
| `POST /api/v1/threads/{id}/messages` | `{request_id,text}` → durable turn |
| `GET /api/v1/threads/{id}/events?after=N` | Authenticated fetch SSE |
| `POST /api/v1/runs/{id}/cancel` | `{expected_attempt}` |
| `POST /api/v1/runs/{id}/retry` | `{expected_attempt}` |
| `GET /api/v1/usage` | Current account's observed counters |
| `GET /api/v1/admin/usage` | Account counters for administrators |

SSE `change` events use the committed sequence as their ID. `reset` requests a
fresh snapshot when the cursor is too old or ahead of the log; `closed` contains a
fixed error code. Each thread retains512 events. Streams use bounded queues, four
per account/64 globally, and revalidate authority before delivery. Already-sent
network bytes cannot be recalled; the UI rejects stale attempts and terminal deltas.

SQLite migration3 adds turn/run/output/event and receipt tables. Claims transactionally
record dispatch intent before network IO. Opaque leases bind tenant, attempt, nonce
and authority epoch. All text writes repeat current lease checks. Supervisor exit
settlement cannot replay a model call. Process shutdown stops new calls and bounds
HTTP draining to3 seconds while the runtime drains within5 seconds.

Usage snapshots replace cumulative values rather than adding duplicates. Missing
provider counters remain unknown. Admin/user screens label totals as reported usage;
cancellation does not claim a refund. Cost accounting and configurable budgets remain
future work. Tools, source ingestion, persistent memory, media and subscriptions are
not available in this unit.

See [verification](reviews/2026-11-12-durable-text-chat.md) for actual acceptance and
[status](status.md) for the remaining assistant implementation.
