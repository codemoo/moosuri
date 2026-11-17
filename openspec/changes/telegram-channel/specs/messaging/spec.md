## ADDED Requirements

### Requirement: Optional Telegram assistant transport
Moosuri SHALL provide account-isolated Telegram private text conversations alongside the PWA using the existing assistant runtime. R30/F71.

#### Scenario: Private account pairing
- **WHEN** a fully authenticated owner opens a one-use unexpired connection link in a private Telegram chat
- **THEN** the verified sender and chat map only to that account, with no email/username inference or access from group messages.

#### Scenario: Shared assistant behavior
- **WHEN** the paired user sends text or supported live guidance
- **THEN** the same Store, agent, memory, tools, budgets and recorded conversations apply, and mail sends still require the web final-preview approval.

#### Scenario: Durable receive and send
- **WHEN** updates repeat, a send loses its acknowledgement, or the server restarts
- **THEN** one update cannot admit duplicate work, uncertain deliveries do not automatically resend and explicit flood-control responses receive bounded delayed retries.

#### Scenario: Connection revocation
- **WHEN** the user disconnects, account authority changes, the bot changes, or a backup is restored
- **THEN** invalid connections cannot admit work or authorize new private deliveries; ordinary web logout alone does not disconnect the independent channel.

#### Scenario: Optional notifications
- **WHEN** a paired owner opts into Telegram notices
- **THEN** only new eligible inbox events produce generic links, with read, dismissed, quiet and muted source decisions respected and no private body automatically forwarded.
