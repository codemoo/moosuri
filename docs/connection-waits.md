# Scoped connections and mission waits

The development preview can admit a Gmail read mission with a configured Google
OAuth app before its mailbox is connected. One valid mailbox is reused; an
ambiguous request waits for one account choice. An address explicitly naming a mailbox takes precedence, including Korean account particles.
A missing named account never silently falls back to another connected mailbox. The original task stays in the mission list,
including after refresh, and the settings back link returns to that task.

The immutable model profile uses the logical `mission-mail-slot-v1` connection.
Admission fixes the existing Gmail read operation, resource and schema version
and the initial account-selection bounds after the selected model's saved tool
probe has been checked. The writer fills only that slot with an exact private
connection pin; it never reloads the current tool profile. Old runs without that
slot cannot acquire it. Once selected, a revoked connection cannot fall back to
another mailbox. OAuth token refresh retains the existing pinned connection
identity and does not renew execution authority.

`GET /api/v1/connections` projects an allowlisted native capability descriptor:
opaque binding/device, operation/resource, schema version, state, expiry and an
optional owner-written alias. It does not return email addresses, vault item
metadata, credentials or grants. Display-only native grants keep their explicit
display-only operation names. `PUT /api/v1/connections/alias` records a deliberate
owner alias. OAuth mission choices use the same descriptor shape.

`POST /api/v1/missions/{id}/connection` accepts a selection UUID, original wait
UUID, mission revision, slot and binding ID. Retransmission is idempotent and a
changed selection or stale revision is rejected. The request cannot add a tool,
operation or account outside the admitted slot. UI selection cards retain the
same UUID after a lost acknowledgement.

The runtime-only `Store::mission_wait_for_connection` can yield an admitted
mission at a stable completed-step boundary. It rejects unfinished tool slots,
dispatched/unknown model outcomes and started external reads. It records the
same mission/run, next step, acceptance revision, slot and wait UUID, deletes the
execution lease and retains every checkpoint and receipt. A ready event obtains
a fresh nonce when the same run is claimed. A native device event still needs its
own task-linked adapter; this API alone does not advertise native execution.

Wait expiry is the earliest original mission, execution, admitted slot and
requested operation expiry. The 120-second ordinary-chat budget is unchanged;
missions retain their original deadline of at most one day. Waiting, readiness,
unlock, refresh and retry do not reset model/tool/repair counts or time. Expired
waits retain completed results and finish without automatic inference replay.

Admission, recurring occurrences and resume share the owner/global capacity
rules: two waiting plus one runnable or explicitly paused mission per owner,
sixteen unfinished missions globally, one executing mission and the existing
foreground chat reservation. Ready waits remain durable while an explicit pause
occupies the runnable slot. On release, the same writer promotes only the oldest
valid ready mission. Both claims and the existing due loop process readiness and
expiry without a page visit.

Focused synthetic store outcomes cover account choice/replay, two ready waits
behind an explicit pause, oldest-first promotion, an actual completed read then
wait/resume at step two with unchanged profile/accounting/deadline/repairs,
unknown-dispatch rejection, idle expiry and selected-account withdrawal without
fallback. They do not establish real OAuth/provider/device acceptance. Native
model-read admission, task-linked native dispatch and trusted-device unlock event
integration are separate adapter work.

Schema60 applies during ordinary open and stopped restore. Restore removes old wait/selection/slot authority and quarantines waiting missions before any account is reenrolled. Owner-written aliases are descriptive metadata and cannot authorize access.
