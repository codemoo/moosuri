## Why

The credential-use contract needs a closed authority/local enforcement boundary
before decrypted credentials may reach any typed adapter. Ciphertext admission is
not credential permission. R14/R16/R20/R28, F33/F44/F48/F57/F58/F63/F64/F69 and CRED02/CRED03
remain the parent contract; this unit does not complete `connect-vault-credentials`.

## What Changes

- Add gated pairing, Ed25519 device authentication, exact invocation commitments,
  revocation ordering and signed private settlement to the assistant writer/HTTP.
- Add a native device-owned grant ceiling and private single-owner nonsecret journal.
- Exercise only a synthetic fixed-destination probe; leave all production dispatch,
  actual credential unlocking, waiting UX, adapter integration and signed delivery off.

## Capabilities

### New Capabilities
- `credential-broker-foundation`: Bounded authority and device-local enforcement.

### Modified Capabilities
None. Existing ciphertext authority and mail final-preview approval stay intact.

## Impact

New native crate, schema0037, feature-gated HTTP and synthetic lifecycle evidence.
See [foundation boundaries](../../../docs/credential-broker-foundation.md).
