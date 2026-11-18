## ADDED Requirements

### Requirement: Consistent account password enrollment
Account bootstrap, password changes and recovery enrollment SHALL require at least eight Unicode scalar values and at most256 UTF-8 bytes. CLI and web guidance SHALL agree. Passwords SHALL NOT be normalized or truncated. Login SHALL continue to verify existing hashes without applying a new minimum.

#### Scenario: Exact boundary and rejected mutation
- **WHEN** an owner enrolls an eight-character password
- **THEN** enrollment and login succeed
- **AND** a seven-character or over256-byte replacement is rejected without revoking the current session

#### Scenario: Unicode and existing accounts
- **WHEN** a password contains multibyte characters
- **THEN** characters and the byte cap are checked independently
- **AND** a valid existing hash remains usable even if an older password has fewer than eight characters

#### Scenario: Successful rotation
- **WHEN** an authenticated owner changes to an accepted password
- **THEN** previous sessions and the previous password are revoked through the existing journaled change path
