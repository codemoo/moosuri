# F51-03 vault recovery across two hosts

The rehearsal used an arm64 macOS publisher and a separate x86-64 Linux receiver.
Both ran native builds of `moosuri-vault-service` at `bd1a6ce`. The fixture used
two synthetic vault accounts and two distinct age recipients for the snapshot
and journal. A synthetic assistant adapter exercised the live vault session
and lifecycle routes. The Linux recovery operator held both age identities in
a private kit before the simulated source failure.

## Verified recovery point

| Event on 2026-11-20 (UTC) | Time | Journal boundary |
| --- | --- | --- |
| Baseline logout | 12:04:45 | Sequence 1 |
| Last recoverable commit, a synthetic account deletion | 12:05:18 | Sequence 3 |
| Operator queried the Linux receiver's accepted receipt | 12:05:47 | Generation 2, sequence 3 |
| Logout after the publication tunnel stopped | 12:05:47 | Sequence 4, outside the accepted receipt |
| macOS publisher stopped and its vault data path was moved aside | 12:06:18 | Source ended at sequence 4 |
| Linux-only restore verified with networking disabled | 12:06:29 | Generation 2, sequence 3 |

The receiver returned the identical first-generation receipt for a repeated
publication before accepting generation 2. A separate operator connection
verified the generation 2 head hash and the encrypted-only journal inventory.
The post-checkpoint logout advanced the source to sequence 4 while the
publisher reported `lagging periodic_gap` and the receiver stayed at sequence 3.
The restored SQLite database had a fresh installation UUID, retained the
sequence 3 deletion record and contained zero imported sessions. The exact
restore reran successfully, and a wrong pin left its destination absent.
SQLite integrity and foreign-key checks passed. A second restore used only
the Linux operator's prepositioned keys and receiver files from a non-root
container with networking disabled.

The recovered commit preceded the simulated outage by 60 seconds in this run.
That is an observed gap for this fixture. Production SSH account isolation,
operator deployment, media durability under power loss and a bounded recovery
point during receiver outages remain separate acceptance work.

## Reproduce the rehearsal

1. On the source, build and embed the vault web client, then build the service.
   Build the same service on the receiver for the recovery CLI. [Build instructions](../vault-service-client.md#build-and-operator-integration)
2. Use private fixture directories on separate hosts. Give the source a pinned
   SSH alias and a fresh publication-only key constrained to
   `moosuri-vault-service recovery receive-journal --target-id f51-synthetic`.
   This run used a separate loopback-only OpenSSH listener on Linux and a
   pre-established loopback tunnel. The source's OpenSSH invocation used only
   the fixture SSH configuration. A shell command sent with the publication
   key could produce no output and failed. [Receiver and SSH setup](../vault-journal-publication.md#initialize-the-independent-target)
3. Commit a synthetic vault logout, leave sessions open, and capture a live
   snapshot at sequence 1. Stage its ciphertext and exact head on Linux. Run
   `recovery target-init` there and confirm `recovery target-status` returns
   `current: null`. Enable the source publication worker with a 30-second
   interval. Check the first accepted generation and its unchanged receipt on
   the next periodic publication. [Live snapshot](../vault-ongoing-backups.md) · [Source worker](../vault-journal-publication.md#enable-the-source-worker)
4. Commit another logout and deliver a synthetic account deletion through the
   vault lifecycle adapter. Confirm the deleted account's session is rejected
   while the other account still works. Query `recovery target-status` through
   the independent operator connection. Record its generation, journal sequence
   and exact trusted head pin. Verify the selected journal generation contains
   only `journal.age` and `head.json` and that the receipt pin matches the latter
   byte for byte. [Independent pin](../vault-journal-publication.md#independently-query-and-recover)
5. Stop the publication tunnel and commit one more logout. Wait for the source
   to report `lagging periodic_gap` and confirm the receiver's receipt stays
   fixed. Stop the source and make its data path unavailable. On Linux, run
   `recovery restore-historical` into an absent path using the already retained
   snapshot, journal, independently queried pin and the recovery operator's kit.
   Inspect the restored sequence, deletion record, new installation UUID and
   session count. A wrong pin must leave its destination absent. [Restore CLI](../vault-historical-recovery.md#recover-through-the-independently-selected-head)

The automated local CLI reference uses the same receiver, status and historical
restore entry points:

```sh
cargo test --locked -p moosuri-vault-service --test publication \
  publisher_actual_receiver_cli_status_pin_restores_without_source_or_receiver_identity
```
