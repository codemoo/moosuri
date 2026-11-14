## ADDED Requirements

### Requirement: Chat-first page code
The web application SHALL load the default chat eagerly and defer secondary page
code until selected. Deferral SHALL NOT change model/effort, account authority,
draft ownership or private-data caching. The existing public-shell precache
policy SHALL retain its build-generated public-only asset allowlist.

#### Scenario: Initial chat
- **WHEN** an online foreground browser opens the default chat
- **THEN** its module graph does not eagerly import settings and other secondary page modules

### Requirement: Recoverable screen loading
An unloaded page SHALL show a brief status while preserving navigation. A page
code failure SHALL offer return to chat and explicit app reload without an
automatic reload or automatic request replay.

#### Scenario: Screen download fails
- **WHEN** a secondary page module fails to download after a draft was entered
- **THEN** returning to chat retains that same unsent draft
- **AND** no message is automatically submitted
