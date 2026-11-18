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
