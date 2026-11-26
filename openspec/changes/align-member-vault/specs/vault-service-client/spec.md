## ADDED Requirements

### Requirement: New vault passwords and existing ciphertext

New passwords for creation, password changes and root-key rotation SHALL contain at least eight Unicode scalar values and at most 1024 UTF-8 bytes. Existing vault and archive credentials SHALL retain their original byte interpretation.

#### Scenario: Set a new password

- **WHEN** a member creates a vault, changes its password or rotates its root key
- **THEN** seven ASCII, Korean or supplementary characters are rejected and eight within the byte limit can be used to reopen the committed vault

#### Scenario: Open a historical vault or backup

- **WHEN** a member opens a vault created with a previously accepted short multibyte password or imports an encrypted archive made before a password change
- **THEN** the original credential opens its matching ciphertext

### Requirement: Member vault interface

The separate vault origin SHALL present creation, unlock, recovery, list, detail, editor, settings and import using the member interface’s typography, surfaces and responsive layout.

#### Scenario: Move between the assistant and vault

- **WHEN** a member opens the vault from the authenticated assistant, edits an item, locks the vault or disconnects
- **THEN** the appropriate screen and its actions remain usable at mobile and desktop widths, and the member can return to the assistant

#### Scenario: Disconnect request is interrupted

- **WHEN** the server cannot confirm disconnecting the session
- **THEN** local secrets are locked and the interface offers a retry
