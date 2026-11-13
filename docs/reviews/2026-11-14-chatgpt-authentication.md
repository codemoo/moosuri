# ChatGPT authentication protocol acceptance

2026-11-14. Connector foundation only. This does not enable a personal account,
subscription selector, credential import or production login.

The official OSS SIWC flow uses fixed discovery/endpoints, PKCE/state/nonce,
consuming callbacks and real RS256/JWKS claim verification. Only granted response
scopes enable plan access; expired access cannot be retrieved for inference.
Private token carriers have an explicit sealing boundary and no generic
deserialization, Debug or Clone escape. Refresh/revocation use the documented
forms without adding scope or retrying an ambiguous rotating-token operation.

Independent review found two real compatibility defects. Returning callbacks may
omit client_id; the repaired flow retains its issued client while rejecting a
supplied mismatch and still checks the signed subject. A failed or expired first
exchange must reuse the retained issued registration; the repair creates a fresh
state/nonce/PKCE attempt without registering a second app. Known identities still
use the returning path, so incomplete-registration recovery cannot replace the
subject pin. Both failures and their corrected evidence were retained.

Independent rereview accepted the exact five-file repair and all frozen source
hashes. Native18 authentication tests, three compile-fail documentation tests and
connector Clippy/format checks passed. Integrated Linux repeated18 auth tests and
three documentation tests; affected-crate Clippy passed. Fixtures use loopback HTTP
and a deliberately public synthetic RSA key, never an actual provider credential.
The reviewed direct ring dependency was already present at the same locked version.

Owner-authorized durable storage, callback/listener lifecycle, cross-process refresh
reservation/CAS, protected VM transfer, disconnect/run fencing and personal model
selection are separate integration work. No actual account eligibility, catalog,
text/tool capability or live authentication is established by these checks.
See [the protocol contract](../chatgpt-authentication.md) for the caller duties.
