## ADDED Requirements

### Requirement: Select a Google account independently of member sign-in
The connection SHALL select only an item in the authenticated member's vault.
The selected Google email MAY differ from the Moosuri member email.

#### Scenario: No Google login exists
- **WHEN** the member starts Google connection with an empty Google selection
- **THEN** the vault requests email and password and saves them encrypted before Kiri starts

#### Scenario: One Google login exists
- **WHEN** exactly one Google login exists, even with a different email
- **THEN** that item is selected without an extra account-choice step

#### Scenario: Multiple Google logins exist
- **WHEN** several Google logins exist
- **THEN** the vault displays their emails and waits for the member's explicit selection

#### Scenario: Selection is tampered with
- **WHEN** a handoff's selected email changes after encryption or on replay
- **THEN** the handoff is rejected before credentials are submitted

#### Scenario: Provider returns another account
- **WHEN** Google returns a mailbox different from the pending request's selected email
- **THEN** the request is rejected before a grant is saved

#### Scenario: Installation app is missing
- **WHEN** the installation has no OAuth app
- **THEN** the member can still reach vault selection and no completed service connection is claimed

#### Scenario: Handoff response is lost
- **WHEN** Deliver has no confirmed response
- **THEN** the vault stays open until canonical status resolves the delivery
