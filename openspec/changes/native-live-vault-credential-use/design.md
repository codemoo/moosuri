# Design

Use existing assistant login/handoff and vault session/current/full-ciphertext APIs;
no vault schema, cryptography or master-password transport changes. Retain only the
vault session and unlocked root in native memory. Persist selected ciphertext pins
only in the private native journal. Recheck owner/header/inventory/selected item
before preparation and after decrypt before fixed provider entry.

Transient network/timeout/5xx suspends while preserving valid in-memory unlock and
a same-process not-entered commitment. Auth rejection or selected item/header/key
change retires it. A restart cannot recreate that in-memory permission and follows
existing positive abandon/not-dispatched/unknown reconciliation. The final fresh
read is admission; later changes cannot recall already-entered requests.

Explicit native reconnect may refresh the same binding ID only for unchanged
item/vault/origin/repository/expiry scope. Writer CAS advances versions and updates
only proven uncommitted uncancelled requests in one transaction. CommitCurrent
signs their exact request digest; old digests and legacy commits after refresh are
fenced. Existing archive journals deserialize with absent live pins; archive mode
cannot consume a live grant. No real provider/account/credential is needed to test.
