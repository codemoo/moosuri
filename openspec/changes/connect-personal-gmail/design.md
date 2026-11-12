# Design

Migration7 stores tenant/account-scoped OAuth staging, encrypted connection tokens,
frozen draft snapshots, monotonic action views, send commitments and asynchronous
read dispatch identities. The existing private installation key encrypts connector
tokens; no model or public DTO receives them. This is server-managed encryption.

The Strict-cookie OAuth callback stores an encrypted code behind an opaque handle.
Only same-origin completion by the initiating session/account/auth epoch exchanges it.
PKCE, one-use state, expiry, exact redirect and granted scopes are checked. Refresh
uses per-connection locks and revision fencing; disconnect cannot be undone by a
late refresh. HTTP occurs outside SQLite's writer actor.

Run profile2 pins the available personal Gmail connections. Legacy profile1 and
unprofiled runs keep their admitted vocabulary. Shared external-tool permits bind
the original run/step/call, nonce, current authority, generation and cumulative
budget. A previously started unfinished read returns unknown instead of replaying.
Cancellation and connection revocation fence late read output.

Mail preparation is a local transaction producing an exact immutable MIME snapshot
and digest. Approval accepts only its revision/digest, never replacement content.
The agent has no approve/send tool. The UI disables approval during editing or a
pending write and discards copied plaintext when its source snapshot is purged.

A separate worker commits one dispatch before network IO. Success requires an actual
Gmail receipt. Crashes, lost acknowledgements and uncertain results do not trigger
automatic replay. Late receipts remain tied to their original commitment. Local
source/thread deletion purges uncommitted derived drafts. Connection-derived runs
and inherited context are revoked through the existing SSE delivery gate.

Current send scope is plaintext from the connected primary address with ASCII
address lists. Attachments, aliases, HTML sending and automatic uncertain-send
reconciliation remain outside this unit. Multipart reads explicitly report omitted
sections. Real Google OAuth/send testing remains distinct from synthetic boundary tests.
