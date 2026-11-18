## ADDED Requirements

### Requirement: Current owner inbox connection helper
The assistant SHALL offer an in-app helper for an admitted current-owner mailbox connection request, and SHALL keep Google login secrets outside of chat.

#### Scenario: Semantic request without a connection
- **WHEN** a tool-capable agent selects the Gmail connection helper for the current request and the server confirms the missing read route
- **THEN** the chat displays device setup, a personal vault option and a route back to the conversation

### Requirement: Device-local Gmail metadata
The assistant SHALL let an operator bind an individual user to a loopback GWS bridge that retrieves bounded current INBOX metadata under the exact configured Google account.

#### Scenario: Device reads for its bound owner
- **WHEN** the owner requests their latest inbox mail and their authenticated device answers for the pinned mailbox
- **THEN** the chat displays the retrieved sender, subject and date

#### Scenario: Device unavailable
- **WHEN** the device cannot establish the requested read
- **THEN** the assistant reports the connection failure without fabricating mail or dispatching a send

### Requirement: Confirm the device mailbox before reading
The assistant SHALL check only the authenticated device profile before presenting the mailbox to the signed-in owner. The assistant SHALL require new confirmation when the registered binding changes and distinguish unavailable devices, expired logins and mismatched accounts. Starting Google authentication SHALL require a user action on a registered connection.

#### Scenario: First use from settings
- **WHEN** the owner opens settings with a registered device that has the expected Google login
- **THEN** the owner can confirm the verified mailbox and settings performs no inbox read

### Requirement: Resume the original inbox lookup
The assistant SHALL allow an owner to continue the latest-inbox request that opened the connection helper once the connection is ready. Continuation SHALL be idempotent, restricted to the authentic request in the same conversation and unavailable after a superseding turn.

#### Scenario: Return to the connection card
- **WHEN** an owner connects the mailbox from an eligible chat and the confirmation response is retried
- **THEN** at most one continued lookup is submitted and the result belongs to the original thread

#### Scenario: Other pending action
- **WHEN** a user finishes setup while another conversation action is pending
- **THEN** the assistant does not dispatch that action through inbox continuation

#### Scenario: Browser adds the local time zone
- **WHEN** the original chat request carries only its browser time zone and no selected target
- **THEN** the confirmed read continues using the original time zone and a selected schedule, mission or artifact remains ineligible
