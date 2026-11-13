## ADDED Requirements

### Requirement: User-owned client cryptography
The core SHALL use explicit bounded versioned owner-bound AES256-GCM envelopes with random root/item keys, Argon2id13 password wrapping and independent HKDF recovery. Account reset SHALL grant no vault unlock and every item field SHALL remain encrypted. Unsupported format/KDF/size and cross-owner/item/revision/purpose tamper SHALL fail without secret-bearing diagnostics.

#### Scenario: Wrong owner or metadata tamper
- **WHEN** ciphertext or authenticated binding/epoch/revision is altered
- **THEN** unlock/decrypt fails and no plaintext is returned

### Requirement: Explicit committed key states
Raw candidate headers SHALL NOT unlock directly. Prepared and changed epochs SHALL require exact trusted-storage receipt admission and new recovery verification before activation. Password rewrap SHALL retain item bytes. Root rotation SHALL require a bounded client-pinned inventory. The core SHALL NOT claim durable journal/rollback enforcement from a digest alone.

#### Scenario: Lost or mismatched candidate acknowledgement
- **WHEN** an acknowledgement is absent or does not match the candidate
- **THEN** the candidate remains uncommitted and the prior active state is retained

### Requirement: Secret-safe local TOTP and import
TOTP SHALL follow RFC6238 SHA1/256/512 vectors with bounded URI/options and checked clock arithmetic. CSV SHALL stream under total/record/field/column caps, preserve quoted Unicode/newlines, apply explicit generic username selection, report duplicate/blank/unsupported rows without value-bearing preview, and produce stable encrypted selected batches. Neither path SHALL expose seeds/passwords/codes in a general model/log/serialization interface.

#### Scenario: Multiline duplicate import and cancellation
- **WHEN** quoted multiline rows repeat or import is cancelled
- **THEN** duplicates are counted, values stay in trusted memory, and no partial plaintext or false storage completion is returned

### Requirement: Honest integration scope
Native cryptography evidence SHALL NOT imply v1 wire compatibility, verified WASM/UI, vault service/backup/archive/restore, independent malicious-server-safe client or credential broker completion.

#### Scenario: Server can replace delivered web code
- **WHEN** only ordinary server-delivered JavaScript is used
- **THEN** DB-encryption protection is not advertised as protection against malicious replacement of that client
