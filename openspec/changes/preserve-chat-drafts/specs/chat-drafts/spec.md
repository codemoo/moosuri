## ADDED Requirements

### Requirement: Current-tab drafts are bound to a verified exact server session
The system SHALL recover only bounded composer and uncertain-request metadata from current-tab
sessionStorage after server authentication confirms the same nonrestricted account/session scope and
its actual thread inventory. The public scope SHALL be domain-separated from the opaque session
digest, account and epoch, SHALL expose no bearer/digest, and SHALL never authenticate any request.

#### Scenario: Same session refresh
- **WHEN** the tab refreshes with the same verified server session and an existing owned thread
- **THEN** the selected thread and its bounded next draft are restored after the owned thread list
- **AND** the previous pending request is uncertain and reconciled by GET only.

#### Scenario: Reconnect after failed or loading boot
- **WHEN** an offline boot fails or is still loading and connectivity returns, or the user retries
- **THEN** the retry preserves inaccessible draft/pending bytes until fresh authentication succeeds
- **AND** exact-session and current owned-inventory validation still precede composer restoration
- **AND** abandoned boot replies, including late401/failure/changed-account replies, cannot clear
  saved recovery or replace the new boot result
- **AND** reconnect restores the original pending UUID/carrier without automatically posting it.

#### Scenario: Pending send and next draft
- **WHEN** an earlier send has no acknowledgement and the user already typed a next draft
- **THEN** refresh preserves both separately and never automatically sends either one
- **AND** explicit resubmission keeps the exact original UUID, attachments and target revisions/context
- **AND** a failed historical run does not overwrite the composer.

#### Scenario: Auth transition, replacement or failed logout
- **WHEN** account/session scope changes, logout starts, a password mutation starts, or401 occurs
- **THEN** the owned recovery key is cleared synchronously and its owner lease is retired
- **AND** late responses and failed logout cannot restore old drafts, even for the same account.
- **WHEN** a restricted/unverified/expired or legacy scope-less session opens
- **THEN** no prior stored composer text is loaded.

#### Scenario: Deleted threads and unavailable storage
- **WHEN** an authenticated current list/lookup identifies a missing or deleted thread
- **THEN** its saved draft/pending slot is removed.
- **WHEN** storage is corrupt, expired, oversized, denied or full
- **THEN** ordinary in-memory chat remains usable without content logging.
- **AND** envelopes remain within16slots/128KiB/8hours and8192 UTF-8 bytes per text.

#### Scenario: Browser trust and private caches
- **WHEN** using draft recovery
- **THEN** drafts remain plaintext only in the current tab's sessionStorage
- **AND** service-worker/cache/localStorage, credentials, private API results and media bytes are excluded.
- **AND** browser clone/crash/session restore behavior is documented as browser trust, not vault security.
