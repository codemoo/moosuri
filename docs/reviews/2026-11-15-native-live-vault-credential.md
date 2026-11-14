# Native live-vault credential use: bounded implementation evidence

The existing fixed GitHub metadata adapter can now use the independent live vault
via native account login/handoff/session and actual vault-core local unlock. This
is a bounded experimental P8 slice; general Google/browser/credential tools,
signed client distribution and real-account acceptance remain open.

## Actual local verification

- `cargo check --locked -p moosuri-credential-broker --features test-fixtures -p moosuri-store`
  passed on native macOS arm64 (11.68s).
- `cargo test --locked -p moosuri-vault-service --test native_broker -- --nocapture`
  passed (one integration test, 17.51s). This runs actual assistant HTTP, vault
  service HTTP/DB, vault-core setup/unlock/rewrap/rotation, native signed transport
  and a fixed synthetic loopback GitHub provider. No real credential/provider was
  used. An additional origin-ceiling guard was added after this run; joined gates
  must verify the final freeze.

The fixture observes two compatible requests reusing selection, wrong repository
waiting without dispatch, 503 before commitment and after commitment/final-check
suspending with unlock retained then automatically revalidating, and a held old
state followed by peer item change failing the final fence with zero new provider
calls. Password rewrap refreshes the same binding/invocation, and root rotation
retires the old root. A previously signed old request digest and a legacy commit
are rejected after refresh. Provider uncertainty stays unknown without reentry.
Lost settlement ACK/reopen resends the exact signed receipt without another call.
Selected-item deletion, device revoke and account-epoch/session invalidation retire
native authority. Synthetic token/master canaries are absent from assistant files,
projected jobs and the private nonsecret native journal.

## Contracts and limits

Transient failure retains only a valid same-process unlocked root/session and
not-entered pending commitment; restart follows existing retained reconciliation
without entry permission. Security mismatches/auth rejection retire memory state.
The final fresh vault read is the admission point: later peer changes cannot recall
an already-entered provider request. Local grant refresh preserves exact scope and
expiry; the assistant sees no vault item ID, key or ciphertext pins. Explicit
snapshot mode cannot consume a live grant or supply live freshness.

Account password is sent only to the pinned assistant login. Vault master stays
local. Native sessions/keys are not persisted and no OS-keystore/background unlock
or process/physical-memory threat guarantee is added. Reconnection currently uses
native commands. A deleted/replaced selected item ID requires retiring the previous
connection and fresh enrollment; there is no local binding-removal UI in this slice.
The native live client caps the fetched inventory at256 items/32MiB. Actual TLS
operator setup, interactive TTY behavior, socket-timeout expiry and real devices
were not exercised by this synthetic HTTP fixture. Default experimental routes
remain feature-gated and CI remains disabled.

Independent frozen review, final all-target Clippy/binary compilation and joined
Linux gates belong to the integration owner; this record does not claim those
checks passed. [Connection/use](../native-credential-github.md) ·
[OpenSpec](../../openspec/changes/native-live-vault-credential-use/proposal.md).

## R1: interrupted in-memory poll reconciliation

Independent frozen review found that terminal input could cancel a live poll after
provider admission or receipt persistence. The pending invocation then attempted
entry again, correctly refused reentry, but blocked later jobs before retained
reconciliation. R1 inspects the durable phase first: only the same-process Committed
phase can retry fresh validation; Entered settles original Unknown, and Settled
resends the exact signed receipt. Pending clears only after settlement delivery is
acknowledged and recorded locally. The fixture now holds the provider and drops the
poll future, then verifies one original entry/Unknown and a later job completing
without reconnect. It also drops a poll with an already persisted result waiting
for settlement ACK and verifies exact receipt redelivery without another entry.
These final R1 source regressions await the integration owner's Linux gate; no
native Cargo run was attempted while another task owned that lane.

## R2: terminal admission failure after suspension

The same phase-based pending reconciliation now covers terminal local admission
failures, including a request that expires during a transient vault outage. Rather
than retaining an endlessly failing pending invocation, it settles the original
commitment according to durable entry state: NotDispatched for known non-entry,
Unknown for prior entry, or the exact already-saved receipt. Only transient
unavailability retains same-process retry permission; settlement ACK is still
required before pending clears. The fixture gives one synthetic job a short writer
deadline before request construction, suspends its post-commit freshness check,
waits through expiry, and verifies original NotDispatched plus a later job's
completion with no expired provider entry and no reconnect. Final R2 Linux execution
and independent source recheck remain with the integration owner; no native Cargo
lane was used for this revision.

## Independent R2 acceptance and joined scope

An independent reviewer reopened the final20 source hashes and accepted both pending
liveness repairs: expired unentered work settles NotDispatched, entered work Unknown,
and saved receipts resend exactly. Pending clears only after settlement ACK. Root
verified the patches and reopened live client, native prompt/transport and writer joins.

Initial joined Linux actual service and broker13 passed; Store6 passed and one
predecessor fixture failed because it retained the newer schema49/50 tables while
claiming schema36. The fixture alone was corrected and passed. R1's actual service
including held-provider and held-ACK cancellation passed. Final R2 expiry/service,
Clippy and binary gates are recorded with publication; passed unrelated cases were
not repeated. A root Clippy command feature-selection typo was corrected without
changing production code. Actual TLS/TTY/device/provider acceptance remains open.

## Final joined Linux acceptance

The final R2 source passed the actual assistant/vault/native/provider fixture
(51.05s), scoped all-target strict Clippy (25.25s) and assistant/companion/vault
binary builds (20.60s). Source hashes matched the executed snapshot. Earlier
broker and Store checks cover the unchanged paths; they were not repeated.
These synthetic tests include cancellation, expiry, key/item changes and receipt
redelivery. Real TLS/TTY/device/provider acceptance remains separate. CI stays off.
