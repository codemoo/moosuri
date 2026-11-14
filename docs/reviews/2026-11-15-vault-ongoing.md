# Non-sealing vault snapshot evidence

Source unit: live same-Database capture, opt-in local sequential worker, shared
HTTP writer wiring, explicit snapshot format/restore and documentation. No new
dependency, schema, HTTP export endpoint, permission UI, production data, deployment
or actual credentials were added/accessed. The full newer-journal historical
contract remains explicitly pending; old snapshots are not called latest.

Native Rust1.89 selected `ongoing_snapshot` gates passed4: lib2, CLI parse1 and
actual ciphertext DB/recovery1. Tests use synthetic local age identities/ciphertext
and local adapter fixtures only. They prove writer/HTTP availability, sequential
worker shutdown/no extra snapshot, missing-stage rejection/no source seal, immutable
capture while source appends, correct subsequent source reopen, age snapshot
restore, session/pending quarantine, body/receipt abortion, repeated restore keeping
fresh work, and owner deletion preserved when included at capture. A deliberately
older snapshot retains its captured ciphertext, demonstrating the documented gap.

Initial selected compile found a test-only Debug requirement on Owner; assertion
was changed to equality without Debug. Production source compiled unchanged.
Final separate snapshot manifest/head/provenance fields were verified by rerunning
the4 selected gates. The existing exact sealed-current receipt/tombstone/reservation
recovery test passed1 after the framing helpers were factored. Vault-service
all-target Clippy with `-D warnings` passed on final source. Exact commands/hashes
are recorded in the freeze handoff. Main owns independent review/Linux integration acceptance.
No full old crypto suite, real account/provider, power-loss or production recovery
acceptance is claimed.


## Independent and Linux acceptance

Independent review verified all14 frozen files and accepted the shared-writer,
independent descriptor, writer-release, distinct-format, no-overwrite publication,
restore-quarantine and exact-repeat/new-work boundaries without a material finding.
Linux passed the four selected snapshot gates and one unchanged exact sealed-current
recovery gate, then all-target Clippy and the vault binary build with existing
public vault-web assets. The joined default assistant and vault binary build also passed against the
integrated call-feed source. No client behavior or assets were changed.
This acceptance is synthetic DB/router/worker behavior, not a production service
rollout or physical crash/power-loss test. Newer pinned-journal recovery remains a
separate implementation unit.
