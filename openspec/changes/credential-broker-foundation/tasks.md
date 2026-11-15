## 1. Foundation source
- [x] 1.1 Add distinct bounded pairing/authentication/receipt types and native Ed25519 implementation.
- [x] 1.2 Add writer transactions, single-use challenges, revocation ordering and restore quarantine.
- [x] 1.3 Add device-local immutable grant ceilings, exclusive ownership and fsynced nonsecret journal.
- [x] 1.4 Add feature-gated HTTP and synthetic authority/adapter fixtures only.

## 2. Evidence and acceptance
- [x] 2.1 Run focused native writer/HTTP/crash/replay and existing ciphertext/restore regressions.
- [x] 2.2 Independently review frozen protocol/storage source and integrate accepted fixes.
- [x] 2.3 Record integrated native/Linux source hashes and limitations without claiming real accounts/devices. Linux broker13, repaired Store6/ciphertext4/HTTP1+1, Clippy/default build passed; the restore fixture explicitly sets its private parent permissions.

## 3. Deliberately still open in parent credential-use work
- [x] 3.1 Bridge supported GitHub/Google typed reads to native unlock/provider use: [GitHub evidence](../../../docs/reviews/2026-11-14-native-github-credential.md), [Google evidence](../../../docs/reviews/2026-11-15-native-google-credential.md).
- [x] 3.2 Add bounded first-connection, locked/offline waiting, same-run resume and selected-credential epoch/rotation behavior: [chat integration](../../../docs/reviews/2026-11-14-assistant-flows-integration.md), [Google rotation evidence](../../../docs/reviews/2026-11-15-native-google-credential.md). Other adapters and external effects remain open in3.3/3.4.
- [ ] 3.3 Prove adapter-specific secret containment and approved mail effect adoption.
- [ ] 3.4 Ship and verify independently signed supported native distribution/device paths.
