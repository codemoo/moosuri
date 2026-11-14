# Personal ChatGPT native transfer to a Linux server

2026-11-14. Bounded development-preview acceptance. No live owner OAuth,
operational installation, production deployment or CI activation occurred.

## Behavior

`moosuri subscriptions chatgpt-vm --server https://your-server.example` runs on
the browser computer. Missing account email and existing SSH alias are prompted;
the password prompt is hidden. A matching target executable receives one bounded
encrypted frame through the fixed `chatgpt-receive --stdin` command and calls the
running owner API. It never opens the target SQLite file or receives its root key.

Fresh official OAuth is bound to the server's pre-browser nonce commitment. The
target independently verifies signed ID evidence, owner/session/epoch, runtime,
host and staged revision before atomically adopting the session and authenticated
receipt. Failed staging preserves the previous active connection. Only the target
refreshes; import does not select a model or grant tools/media/background access.
The authorized helper/OS remains trusted to carry the provider's opaque token pair;
signed ID evidence is not a cryptographic refresh-token binding or binary attestation.

The source persists its encrypted pending envelope before SSH. On lost response it
uses the same request and independently verifies the durable receipt before removing
the envelope. Reauthentication can recover an already imported receipt without
performing another OAuth exchange. Removal does not imply physical SSD erasure.

## Findings repaired before acceptance

Independent review found that a transported ordinary login could change passwords
and create lasting authority, and that initial/recovery crashes could leave uncapped
sessions. Password-authenticated helper sessions now receive persisted purpose and
an immutable ten-minute absolute deadline atomically at issuance. Default Store and
HTTP operation validators reject this purpose; only handoff, safe session/status and
own logout remain available. Normal browser/P1 session behavior is unchanged.

Inherited macOS ACL grants could expose mode-private journal/key files. Descriptor
ACL checks now precede private reads/writes on ancestors, state, lock, key and atomic
temporary files. Real inherited-ACL RED/GREEN tests and independent Darwin probes
verified rejection; Linux mode/link/xattr checks were executed separately.

Retained staged revisions could collide with ordinary P1 reconnect and lose incomplete
issued-client metadata. P1/P2 now share monotonic reservations and separately inherit
verified identity pins and usable issued registration. Signed failed-exchange,
cancelled/expired/still-pending transfer to fresh-code P1 completion cases pass.

Root integration found that the newer PWA origin query also needed to exclude helper
rows. A real failing test demonstrated acceptance of an internally reconstructed helper
namespace; requiring persisted normal purpose fixes it while preserving naturally
expired ordinary origins. A real JWKS response barrier additionally verifies that helper
logout, account epoch change, disconnect, P1 revision change and ticket expiry each
prevent a late verified import. Independent repair and root integration reviews found
no remaining material source defect within their stated scope.

## Executed verification

- Linux Rust1.89, pinned offline build: Store407 passed, with two explicit reference
  Go-age interoperability tests ignored; agent97 passed, with the opt-in live-account
  evaluation ignored; connectors184/core3 passed; HTTP39 passed, with the opt-in
  public GitHub fixture ignored. Child-process output is not counted twice.
- Linux native helper4 and actual receiver subprocess1 passed. Affected all-target
  Clippy passed with warnings denied. Default and explicit fixture builds passed;
  the default executable rejected HTTP fixture origins before login even with the
  fixture environment variable set. Native default feature graph excludes fixtures;
  generated TypeScript DTOs match the checked-in contract.
- Native root PWA6, five-boundary JWKS race and helper HTTP tests passed. Worker
  evidence includes abrupt process exits at issuance, preparation and receipt recovery,
  restore10, signed P1 recovery, helper import and actual Darwin ACL tests.
- Full React execution initially had377 passing tests and four old public-shell VM
  harness failures caused by its new module import. The harness-only repair passes all
  four affected tests; frontend/worker TypeScript and production build pass. This is
  composed evidence, not a claimed fresh full381-test rerun after that repair.
- Actual Chrome with the default Rust/SQLite server verifies the native connection
  guide at390px, no browser-held transfer material or handoff request, draft preservation
  through settings and reload, one explicitly sent shared-model reply and saved reload.
  Its model response is synthetic; live OAuth and physical mobile devices were not used.

The composed Linux test uses actual OpenSSH, pinned generated host keys, the actual
native helper/receiver and running Rust server, plus a local signed OAuth/JWKS/model
fixture. An owned opener forwards the synthetic official authorization URL only to
that fixture, which verifies state/PKCE and drives the actual loopback listener.

| Case | Observed result |
| --- | --- |
| Normal transfer | One OAuth exchange, independent source/target JWT checks, one receiver/import, target-only refresh, exact model proof/selection and saved chat after server restart. |
| Lost SSH response | Receiver commits before stdout is held; helper and local SSH are killed, target restarts, same helper reconciles the receipt without a second OAuth/receiver/import. |
| Wrong host key | Strict host verification rejects before any receiver; original envelope survives, trusted retry uses the same digest and one OAuth exchange. |
| Target killed during JWT verification | Actual SIGKILL after durable digest/45-second lease admission but before verification/CAS; same DB restarts with no installed session/receipt. Real lease expiry permits the original envelope retry, one OAuth exchange and exactly one committed import. |

All four cases prove target proof/selection/chat and saved conversation after restart.
The last case invokes the receiver twice, with only the retry producing an imported
receipt. Both helper and target retain distinct host identities. Source envelopes are
removed only after receipt confirmation. No provider revoke is issued by source cleanup.

The first SSH fixture run was rejected because OpenSSH StrictModes disallows an
authorized-key path beneath `/tmp`. The fixture retained StrictModes and client host
pinning, placing only its generated public authorized key in a verified private user
runtime directory. Every such directory was removed after its owned sshd stopped.
No operational SSH configuration, account policy or authorized keys were changed.
The first default-isolation probe omitted the helper's required private HOME; its
corrected dedicated test HOME still produced the expected production HTTP rejection.

Frozen integrated code:890 source files before documentation closure; the Linux
archive includes904 files with public web assets. Fixture binary SHA256:
`0bdd52281e1e19b8753e4d89c25d024545abb88b382093693c39bbbd8e5c5aa7`.
Composition harness SHA256:
`5e6cba502ca9d7262471747196611f77e351c47333e123c487b040c62d8eaec1`.
No secret, journal, synthetic private key or local fixture DB is published as evidence.

## Remaining acceptance

This is a single Linux host with separate client/target state and real loopback SSH,
not a physical multi-host or real-account trial. Actual OAuth eligibility, operational
HTTPS/SSH trust, device/browser behavior, signed installers and managed-host conditions
remain separate. Tests do not inject a crash at every instruction between journal
publication, verification and SQLite commit; restore verification covers the supported
stopped/quarantined path, not an online restore feature. Personal tools/media/background,
Claude execution and the rest of the product roadmap remain incomplete.
