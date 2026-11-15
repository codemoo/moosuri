## ADDED Requirements

### Requirement: Explicit pinned HTTPS bundle transport

For P10 installation distribution, the CLI SHALL accept an explicit versioned HTTPS bundle
base, an independent owner-private Ed25519 pin and a new private destination. It SHALL NOT
infer an official key/latest release, attach credentials, follow redirects or execute a
bundle during download. Local install/update contracts SHALL remain available separately.

#### Scenario: Complete evidence-bearing bundle becomes locally usable

- **WHEN** an operator supplies a directly serving versioned HTTPS base, correct independent pin and new private path
- **THEN** only fixed manifest/signature/binary and signed-bound provenance/inventory basenames are requested
- **AND** exact signature, native target, product, signed version/size/hash and evidence contracts pass before atomic no-replace publication
- **AND** local verify and explicit install can consume the bundle without implicit initialization or service startup

#### Scenario: Manifest authority fails before large downloads

- **WHEN** a manifest signature is missing/invalid, its target/path/version is refused or its authenticated evidence reference is invalid
- **THEN** no untrusted manifest path is requested and the executable is not requested
- **AND** no destination is published and only owned unchanged staging files are cleaned

#### Scenario: Tampered partial or oversized response

- **WHEN** any fixed response is missing, truncated, oversized, redirected or disagrees with its signed length/hash/provenance
- **THEN** download fails with a value-free error within bounded requests/body/time limits
- **AND** no incomplete destination is published or bundle code run

#### Scenario: Existing path or replaced staging contents

- **WHEN** the selected destination already exists or appears before publication
- **THEN** its files/directory are never overwritten
- **AND** failure cleanup never recursively removes entries or removes replaced/unrelated staging identities

#### Scenario: Explicit private HTTPS mirror

- **WHEN** a mirror requires the explicitly supplied owner-private bounded CA certificate
- **THEN** ordinary TLS certificate and hostname validation remain enabled with that added root
- **AND** the independent signing pin is still required
- **AND** HTTP, URL credentials, query/fragment, encoded traversal and automatic redirects remain refused
