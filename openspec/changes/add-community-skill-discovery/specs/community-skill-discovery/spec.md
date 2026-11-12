## Purpose

Let contributors publish reusable skill content through Git and let users discover and use compatible capabilities in the assistant's existing suggestions interface with private user state preserved.

## ADDED Requirements

### Requirement: Existing card discovery
The agent UI SHALL expose community capabilities through suggestions and available-work cards with truthful ready, installable, connection-needed or unsupported status.

#### Scenario: Missing personal connector
- **WHEN** a compatible recipe needs a connector the user has not connected
- **THEN** the card offers the required connection flow and does not claim immediate execution

### Requirement: Scoped immutable installation
Installation MUST bind source, exact revision and content digest to the authenticated user; failed updates retain the old active version.

#### Scenario: Interrupted update
- **WHEN** package staging fails before activation
- **THEN** the previous pinned version remains available and other users are unaffected

### Requirement: Private sharing boundary
Shared recipes SHALL be reconstructed from allowed structures and synthetic examples without private traces, credentials or transferable execution authority.

#### Scenario: Imported success claim
- **WHEN** an author reports successful runs in an imported recipe
- **THEN** those claims do not satisfy the receiving user's local proof or grant permissions

### Requirement: Local recommendation and authority
Catalog recommendation MUST NOT send private context to the registry, and package content MUST NOT execute hooks or expand tool or credential authority.

#### Scenario: Skill requests a secret on another host
- **WHEN** package instructions request a new credential destination
- **THEN** the current tool/broker scope rejects that request without exposing the secret
