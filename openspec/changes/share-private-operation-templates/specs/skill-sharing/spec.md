## ADDED Requirements

### Requirement: Reconstruct shareable operation packages without private trace bytes
The service SHALL authenticate the owner and exact route revision before reconstructing only a supported known operation from original public templates and synthetic examples.

#### Scenario: Download a learned calculator procedure
- **WHEN** the owner requests a current calculator route's share preview
- **THEN** four compatible files and a regular-file archive are returned without private inputs, identity or success proofs

#### Scenario: Another account or withdrawn route
- **WHEN** another account requests the route or the route has been removed or changed
- **THEN** no private-route package is returned

### Requirement: Sharing is an explicit local download
The suggestions UI SHALL display inert package text and download only on the owner's action; it SHALL NOT publish, install, execute or send as a side effect.

#### Scenario: Account changes while a preview is pending
- **WHEN** the previous account's response arrives after the account lifetime ends
- **THEN** it is not rendered or downloaded
