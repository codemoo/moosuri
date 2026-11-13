## 1. Core
- [x] 1.1 Add bounded v2 envelopes, secret ownership, native entropy interface and item encryption.
- [x] 1.2 Add committed-header admission and pending recovery/rewrap/root-rotation states.
- [x] 1.3 Add RFC6238 TOTP and strict streaming local CSV preview/encryption.

## 2. Verification
- [x] 2.1 Freeze native vector/attack tests, clippy/fmt/dependency/plan/spec evidence.
- [x] 2.2 Complete independent core review and root integration.
- [x] 2.3a Complete the separate WASM/Worker/UI/ciphertext service/journal integration; [service/client evidence](../../../docs/reviews/2026-11-13-vault-service-client.md).
- [x] 2.3b Complete bounded authenticated client archive export/import; [archive evidence](../../../docs/reviews/2026-11-13-vault-archive.md).
- [ ] 2.3c Complete old-v1 compatibility/migration, standalone service backup/restore and trusted-device broker gates before full v1 vault support claims. The implemented slices do not complete this aggregate contract.
