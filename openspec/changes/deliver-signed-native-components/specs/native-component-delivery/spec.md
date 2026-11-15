## ADDED Requirements

### Requirement: Separate authenticated native component packages

For P8/P10, optional credential-broker and vault-service delivery SHALL bind a separate
signed component identity, supported native target, SemVer, sequence, fixed basename,
exact size/hash and any signed evidence. The assistant install/update contract SHALL
remain separate. Delivery SHALL NOT execute binaries, deploy services, initialize state,
provision credentials, pair devices or imply an official signing authority.

#### Scenario: Recipient obtains a component without compiling

- **WHEN** an operator selects a directly serving version/component/target HTTPS base, expected component/target, independent Ed25519 pin and new private destination
- **THEN** exact signature and strict signed manifest are authenticated before evidence/binary downloads
- **AND** only fixed basenames with bounded exact sizes/hashes/evidence and valid native architecture/tables are admitted
- **AND** the complete package is atomically published without overwrite for separate explicit operator use

#### Scenario: Component confusion or malformed payload

- **WHEN** a signed package names another component/target, an escaping basename, a mismatched version path or a malformed/truncated native container
- **THEN** the package is refused and no incomplete destination is published
- **AND** untrusted metadata never selects arbitrary paths or causes binary execution

#### Scenario: Tampered or interrupted response

- **WHEN** a signature/evidence/binary is missing, changed, partial, oversized or redirected
- **THEN** the request fails within the existing bounded HTTPS policy and unchanged owned staging entries are cleaned without recursive deletion
- **AND** unrelated existing paths are retained and no running service/state is changed

#### Scenario: Packager evidence remains limited

- **WHEN** a packager generates component provenance and fresh source-lock inventory before externally signing the exact final manifest
- **THEN** the evidence binds the exact component/target/version/digest as packager claims
- **AND** ordinary verifier output does not claim build attestation, license clearance, device trust or independent vault deployment

#### Scenario: Explicit operator lifecycle

- **WHEN** a recipient verifies a package or chooses a different signed version
- **THEN** pairing, host/TLS/UID configuration, private-state compatibility, stop/start and upgrades remain explicit documented operations
- **AND** no automatic component state migration, rollback or antirollback authority is inferred from the package sequence
