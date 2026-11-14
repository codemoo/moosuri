## Identity and persistence
The retry identity is composite: tenant/connection, current config/auth/credential
grant, expected run generation and client UUID. It is not a forever-global UUID.
Status returns run_generation and a non-authorizing opaque start_binding derived
from that current grant; intent repeats both plus exact sequence/checkpoint.
The bearer is still required. No token/nonce is stored in the start sidecar or
returned to the client. Store guards current authority inside the immediate
transaction before allocating or resolving a start.

Schema48 maps a run to client UUID and canonical request digest; it cascades with
normal run pruning. Connection generation is monotonic through normal starts,
configuration/checkpoint reset, account lifetime and restore quarantine (which
increments it and retires credentials). A pruned old expected generation cannot
allocate again. A changed UUID/payload in an already occupied generation conflicts.
There is no finite start-key quota that stops ordinary periodic synchronization.

## Historical readback
Exact retained replay returns the same run ID/generation and current historical
state, even after a committed batch advanced the cursor, without lease renewal or
another generation. Current before-position is required for a new allocation. Expired running history is
fenced unknown. Startup/restore unknown history is never rewritten as completed.
An unrelated active run is never adopted. Legacy empty-object and official
capability starts retain their original response/lease path.

## Producer
Persist a typed immutable start intent before first dispatch. Lost ACK retries
that same composite intent. A later explicit invocation may stage one fresh
start/batch attempt only after its original historical identity is known inactive,
no batch envelope was ever staged, original batch receipt is absent, current
before-position/grant still matches and no conflicting run is active. Snapshot
entries remain fixed; no re-extraction is hidden under an old request UUID.
A staged inactive batch stays pending rather than being silently rebound. Existing
R1 committed-receipt/current-after-position recovery remains independent of finish.
Legacy pre48 unkeyed uncertain starts cannot be retroactively identified.

## Verification
Use synthetic Store fixtures for replay/expiry/pruning/scope/grant/reset/upgrade,
existing actual sealed-current restore quarantine, focused production-router
requests and actual local-file→Store/router faults after start commit before ACK.
No live account/source/provider acceptance follows from these checks.
