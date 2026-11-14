# CredentialBroker authority and device-local foundation

This bounded source unit supplies broker pairing, device authentication, online
writer commitments and a native local enforcement library. It does **not** connect
vault items to chat, Gmail, Calendar, browser controls, missions or a model tool.
It does not ship a companion executable, signed installer or production adapter.
The full [credential-use contract](credential-use.md) remains open. Every actual
email send still requires approval of its final frozen preview.

## Availability and trust

The assistant routes are absent from default builds. The explicit server feature
`credential-broker-foundation` compiles the experimental protocol routes; it does
not add production invocation minting or external dispatch. `test-fixtures` can
mint only a typed synthetic FixedProbe invocation. This fixture is not a production
capability and cannot accept an arbitrary effect ID or URL/header/body/secret field.
No deployment is part of this unit. Independent review is required before enabling
any production credential path.

First pairing and local account/item/scope selection are connection steps. The
native local grant ceiling is authoritative; the assistant stores only opaque
binding commitments. Compatible requests reuse the selected grant automatically.
The foundation's only grant is a synthetic, fixed-destination resource probe; there
is no OAuth registration/refresh, vault unlock, TOTP or secret transport.

A paired assistant can invent a new task **inside** the selected scope. Signatures,
pins and digests do not prove that a real user uttered that task. A trusted local
executable and OS are assumptions. This library is not an independently signed
client distribution. Served web JavaScript retains its malicious-server replacement
limit; neither a Worker nor another subdomain establishes a signed client boundary.
Coordinated rollback of every local file requires an independent monotonic anchor
that this foundation does not supply. Missing/truncated/corrupt journals, mismatched
anchors and stale enrollment generations fail closed; do not claim hardware rollback
resistance or secure deletion/provider token revocation.

## Protocol and writer

Pairing starts with a current unrestricted normal assistant session, never a helper
session. A 60-second single-use challenge pins the real tenant/account/auth epoch,
installation, prospective public key, device identity/generation and configured
HTTPS origin. The device proves possession using Ed25519. The assistant cookie is
never transferred to a device. Failed key proof cannot enroll a different key.
Concurrent consumption admits only one enrollment.

Device endpoints reject cookies and Origin headers. A fresh 30-second challenge is
signed with the paired key over method, exact path, canonical typed body and full
owner/device/origin/expiry context. Challenges are bounded and successful requests
consume them in the writer transaction. Bodies are bounded to16KiB. HTTP host/origin
checks remain in the assistant's existing middleware; device auth does not borrow
cookie authority. A future native transport must pin the enrolled HTTPS origin and
reject proxies/redirects. This unit tests server HTTP but ships no network client.

The native-only signature dependency is explicitly pinned `ring = 0.17.14`, already
present in the existing graph (`Apache-2.0 AND ISC` upstream license; see ring LICENSE and per-file notices). The protocol crate has
no new native or crypto dependency and remains WASM-compatible. Ed25519 signatures
use ring's RFC8032 implementation, with an independent RFC8032 reference vector.
Signing never depends on JSON/map iteration: binary schema v1 uses fixed domain
strings, explicit field order, big-endian integers and length-prefixed variable
fields. Request authentication hashes that bounded tuple with SHA256 under a
separate domain. Receipts use their own domain. A Python `struct.pack` reference
vector covers canonical pairing bytes. These source tests are protocol evidence,
not an independent cryptographic audit.

SQLite's existing single writer is the authority linearization point. New
commitments validate the current real enrollment session, account/auth epoch,
installation, active device and binding, exact stored typed invocation, digest,
grant/item/key/binding versions and expiry. The identity is the complete
`(tenant, run, step, call, original invocation nonce)` tuple. There is no public
`commit(effect_id, digest)` shortcut. Existing ciphertext authority and receipts
are unchanged and cannot act as credential permission.

An exact repeated commitment returns the same adopted owner and nonce. Revocation
before commitment blocks it; commitment before revocation remains possibly in
flight. Another device/generation cannot recover that adoption. A lost response
requires the original device to query the original identity. Disconnect, timeout
and lease expiry never prove that dispatch did not happen. Server revoke blocks
new commitments and records `revocation_pending`; authenticated device ACK records
`revoked`. Neither state claims upstream token revocation. Repeated revoke preserves
an existing ACK.

Receipts bind the exact commitment, owner/generation, original tuple, digest,
operation, grant version, outcome and fixed projection. Paired-key signatures are
required; server flags/HMAC are not device evidence. Exact duplicates are idempotent;
a changed receipt conflicts. Unknown, positively not-dispatched, rejected and fixed
projected-success are distinct. Authenticated late receipts survive session loss,
cancellation and restore quarantine in private non-authorizing records. Receipt
arrival writes no chat output and never clears a cancelled invocation.

Migration0037 is transactional. Restore migrates predecessor images, clears pairing
and auth challenges, quarantines enrollment/bindings and preserves commitment/receipt
evidence. New pairing is required before new credential authority. Password reset
cannot unlock vault material or revive local grants.

## Native files and crash semantics

Explicit enrollment creates a fresh0700 owner directory,0600 single-link regular
files and a device-private Ed25519 key file. Reopen never creates missing state.
Descriptor-relative `openat`, `O_NOFOLLOW`, owner/mode/link checks and a process-wide
exclusive OS file lock fence competing owners. Ancestors are checked for unsafe
ownership/write permissions and symlinks. Darwin extended ACL inspection rejects
entries, including inherited grants; Linux rejects extended/default/NFSv4 ACLs.
Unsupported platforms fail closed.

The separate key file is secret; the journal is not. The bounded journal stores
only local grant metadata, opaque local references and prepared/adopted/entered/
receipt tuples. No master key, credential plaintext, token, seed, TOTP, provider body
or model trace enters it. Every transition fsyncs the journal before its separate
length/hash anchor. Partial writes or a crash between those writes fail closed on
reopen. This prefers unavailable work to replay. The anchor does not protect against
an attacker restoring both files together.

Preparation is durable before contacting the writer. A known commitment is durable
before adapter entry, and entry is durable before an adapter call. The only adapter
entry code in this unit is compiled in tests. A pre-entry known state can be
settled as positively not-dispatched; entry without a retained receipt remains
unknown. Reopened retained preparations/commitments can only reconcile/settle,
never enter an adapter. Local revoke blocks entry immediately. This conservative
foundation does not implement user-facing waiting/unlock/resume or failover sends.

The test-only fixed probe projects a fixed enum and discards malicious echo/error
bodies, including synthetic secret/TOTP canaries. This establishes only the bounded
foundation protocol property. Full agent/model/log/export containment, native
credential unlocking, real provider/device behavior and signed packaging require their own evidence.
Integrated Linux checks are recorded in the [foundation verification](reviews/2026-11-14-credential-broker-foundation.md). See the [foundation OpenSpec delta](../openspec/changes/credential-broker-foundation/proposal.md) and the
parent's frozen review record for commands actually executed against integrated
source. No real account/device acceptance is claimed here.
