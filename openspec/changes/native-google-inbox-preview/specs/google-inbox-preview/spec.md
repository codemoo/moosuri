## ADDED Requirements

### Requirement: Separately selected bounded personal INBOX preview

The system SHALL allow a direct current personal request to read up to5 INBOX previews through a fixed trusted-native Google operation, with subject/from/date/snippet display fields only. It SHALL verify actual refresh scope, stable account identity and current native authority, and SHALL preserve the original invocation while waiting/restarting/settling.

#### Scenario: Existing counts selection cannot read contents

- **WHEN** only a counts grant or an old counts-enabled tool profile exists
- **THEN** preview SHALL remain unauthorized/waiting for separate native preview selection, without provider-content entry or an automatic privilege upgrade.

#### Scenario: Preview result contains untrusted mail

- **WHEN** fixed INBOX list and metadata reads succeed
- **THEN** the signed bounded projection SHALL show at most5 literal previews, disclose truncation, missing/deleted/moved observations and unrequested additional pages, reveal no provider IDs/account metadata/credential echoes, and grant no action authority.

#### Scenario: Empty, unknown and interrupted operation

- **WHEN** the list is empty, an item disappears, provider transport fails or an entered operation restarts without a receipt
- **THEN** the result SHALL truthfully report empty/partial/unknown respectively and SHALL NOT replay the entered read or fabricate a complete-mailbox/latest-order claim.

#### Scenario: Original chat receives one terminal receipt

- **WHEN** a separately authorized current-chat preview waits for the native device
- **THEN** the same run/step/call/nonce SHALL settle idempotently with fixed literal rendering and no additional model call; cancellation/revocation SHALL prevent output resurrection.

#### Scenario: Previous protocol bytes remain usable

- **WHEN** an existing FixedProbe/GitHub/counts request, receipt, projection or registration command is encoded
- **THEN** its previous signature bytes SHALL remain unchanged; preview SHALL use separate explicit discriminants and a default-false persisted profile flag.

### Requirement: Persistent display-only and per-request content failure

Preview contents SHALL remain owner-visible but SHALL NOT enter future model history, conversation search/read or learned-skill authority, independent of feature/profile changes or restart. Malformed/unusable mail SHALL fail only the original request without suspending a credential that passed actual authentication, scope and identity checks.

#### Scenario: Later model request or recall after reopen

- **WHEN** an owner reopens a displayed preview and starts another chat or searches/reads earlier turns
- **THEN** the owner history retains the preview while no preview canary reaches model context or recall, even if the broker feature is disabled.

#### Scenario: Invalid mail followed by a new valid request

- **WHEN** mail has malformed structure, duplicate selected headers, disallowed controls or a credential echo
- **THEN** that entered request settles unknown without secret output or replay, and a separate valid request can reuse the same grant without reselection.

#### Scenario: Near-cap original output

- **WHEN** a preview is admitted near the chat output limit, or a retained older request has only4KiB remaining
- **THEN** admission reserves the bounded rendered maximum or settlement uses an honest content-free fallback, and the original terminal slot settles without an endless transaction rollback.
