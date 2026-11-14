# Invitation delivery boundary

Schema0043 follows standalone account-deletion41; root integration includes42 first.
The new invitation effect is keyed by a caller-generated UUID and target credential
epoch. One effect per target credential epoch prevents a second automatic send under
another request UUID. Target name/email/revision/expiry are read from accounts; the
request cannot choose an arbitrary recipient or email content. A selected temporary
password must verify against that current temporary credential using the existing
bounded authenticated KDF gate, then the exact hash/revision is checked again.

The admin-only pending snapshot contains greeting/name, account email, configured
login URL, credential expiry, first-login change instructions and optionally that
one-time password. It freezes exact MIME/Message-ID and digest, encrypts payload with
the server installation provider key and never enters chat or a model-visible mail
object. At most16 pending effects per actor; lists show latest50, and GET by effect
UUID remains available. Only the original admin/session can recover pending content
or approve. A distinct admin cannot read or approve the actor's effect. The preview
expires in15minutes or at credential expiry, whichever comes first.

Approval accepts only digest and effect ID, revalidates admin session/role/epoch,
target status/auth epoch/management revision/temporary credential expiry and sender
pin, then records commitment and removes the encrypted payload before external IO.
One invocation is permitted. A duplicate approval reads the existing result only.
A further current-authority check precedes adapter invocation. Resets/account
lifecycle changes immediately expire pending effects and clear payload; deletion
also scrubs address/subject/provider metadata. Password activation fences delivery.
The existing supervised five-second DB housekeeping tick erases expired snapshots;
startup and authenticated reads also do this. Restore expires all pending effects
and turns committed delivery into unknown, preserving historical outcomes.

No delivery worker automatically sends. The approved request owns a bounded native
invocation; cancellation/response loss after commitment cannot requeue it. Native
spawn/config failure is definite pre-delivery failure; every failure after the
native send process starts is conservatively unknown. Repeated status/approval
requests and restart do not invoke send again. A late original receipt can update
that original committed/unknown effect without granting another send. Reinvitation
requires an explicit password reset which invalidates the previous credential.

# Native gws adapter

Operator-only CLI options select an absolute native gws executable, dedicated0600
credentials JSON, dedicated0700 config/staging directory and exact sender address.
Nothing defaults to the host user's gws account, HOME, token, proxy or config.
The spawned process gets env_clear, only explicit HOME/config/credential paths and
logging off, null stdin/stderr, bounded16384byte stdout and35second timeout with
kill-on-drop. Credentials and MIME/password are never argv values or logs. The
configured address is an operator assertion; configured does not mean a verified
Google profile/scope. A minimum gmail.send grant is sufficient to attempt sending;
this slice does not silently add read scope just to verify the sender.

The supported native version is gws0.22.5, checked with a bounded local --version
before send. Its documented `--upload` + `--upload-content-type message/rfc822` input
is used with fixed `gmail users messages send --params {userId:me}` arguments.
Public tagged source confirms the generic executor performs one request.send()
and does not use the library's separate retry helper. Other CLI versions are
rejected before send pending separate source/behavior review.

Public source also confirms --upload canonicalizes paths under CWD and reads file
metadata/streams. An unlinked descriptor, stdin sentinel or invented @JSON syntax
is not claimed. The adapter creates a new0700 staging directory and0600 MIME file,
uses that directory as CWD, and passes only `payload.mime`. Rust removes it after
success/error/timeout. A hard process crash can leave this transient plaintext
file; the configured sender's next startup removes only owned exact-format stale
stages. This is private transient dispatch storage, not encrypted pending storage
or guaranteed physical erasure; operators must protect the dedicated directory.

Gmail response message ID means acceptance, not recipient delivery/reading. Unknown
stays unknown because send-only credentials do not authorize Sent inspection. No
blind resend, automatic new password, live credential test or real Google email
acceptance is claimed. CLI and Google credentials remain separately installed and
managed by the operator; the product does not launch OAuth in this flow.
