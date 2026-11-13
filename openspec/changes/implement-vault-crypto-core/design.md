# Design

Use the [versioned core design](../../../docs/vault-core-design.md) and [integration guide](../../../docs/vault-core.md). Required primitives remain AES256GCM/Argon2id13/HKDF-SHA256; owner/tenant AAD and header MAC require an explicit v2 format. No silent old-ciphertext migration or server escrow.

Pending prepare/rewrap/recovery/rotation states require explicit recovery verification and exact trusted-storage commitment before activation; raw Header cannot unlock. Receipt construction is a trusted adapter boundary, not proof of physical durability. Later service CAS/journal/nonce/restore quarantine and client lifetime enforcement remain mandatory. CSV and TOTP never become general model tool output.
