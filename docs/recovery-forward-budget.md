# Forward requests after historical recovery

Historical recovery retains `accounting_unknown`: retained usage and reservations
are lower bounds, and later provider charges cannot be reconstructed from the
snapshot. Fresh credentials do not reconcile that history. The default blocks
new model dispatch, including capability probes, automatic memory and STT.

An installation operator can explicitly allocate **new** requests while the
server is stopped. This changes neither historical usage nor provider/account
re-enrollment. It never resumes imported quarantined work automatically.

```sh
moosuri --data-dir /absolute/private/restored-data recovery-budget status
moosuri --data-dir /absolute/private/restored-data recovery-budget allocate \
  --restore-id RESTORE_UUID_FROM_STATUS --request-id NEW_UUID \
  --calls 100 --audio-seconds 600
```

Use the current signed executable. Managed `installation run -- ...` supports
`recovery-budget`; stop its service first. The same Store writer admission guards
allocation and rejects a running writer. Inspect status, then explicitly restart.
The exact request UUID/restore/caps can be retried safely; changed parameters with
the same UUID reject. A new UUID is a separate explicit allocation. A restore ID
mismatch rejects. New historical or sealed recovery receives a new restore
identity and cannot reuse allowances imported in its snapshot.

The allowance is installation-wide across shared/personal model connections and
accounts. `--calls` is 1–1000 new model inference requests per allocation,
including STT and capability probes. `--audio-seconds` is 0–3600 validated STT
input seconds per allocation; STT consumes both one request and duration.
Two-round tool capability verification consumes two requests. Model catalogs,
credential registration and token refresh do not consume inference requests.
Existing per-run six-model/twelve-tool limits, STT concurrency/day limits,
request/output validation, transport byte limits and timeouts still apply.

Each actual dispatch transaction permanently commits one unique reservation
before transport. A duplicate commitment grants no new request. Timeout,
cancellation, failed acknowledgement and process loss consume the allowance;
there is no automatic refund. The final permitted request can finish: heartbeat
checks identity and authority rather than requiring another remaining request.
Exhaustion rejects the next dispatch with `recovery_forward_allowance_exhausted`.

These are **request-count and STT-input-duration limits**, not money or token
ceilings. Personal ChatGPT's SIWC transport does not accept a provider output-token
cap; closing a local connection cannot prove remote processing or charging stopped.
The product does not reconstruct a billing period or claim complete accounting
recovery. Historical totals remain unknown in usage views; the settings banner
and `/api/v1/recovery-budget` report only the separate new allowance and permanent
commitment counts. Existing historical usage/reservations are never cleared,
relabelled as reconciled or converted to free requests.

Account deletion retains only the immutable admission lower bound: opaque
account/tenant/allocation IDs, a random dispatch nonce, operation kind, timestamp
and duration. These rows contain no prompt, response, email/name, credential,
provider ID or destination. Like existing content-free model-call receipts, they
are exempt from private tenant-content purge so deleting an account cannot refund
spent allowance, block restart or prevent latest deletion-journal replay.
