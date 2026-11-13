## ADDED Requirements

### Requirement: Calendar consent is owned and separate
The system SHALL use explicit per-owner Calendar consent and encrypted scoped credentials,
reusing the configured operator app without silently expanding Gmail or shared model grants.

#### Scenario: Missing or partial Calendar consent
- **WHEN** required Calendar scope, refresh token, original session or pinned app is absent
- **THEN** no active Calendar connection is created or refreshed

#### Scenario: Callback from a different session or owner
- **WHEN** a captured OAuth handle is completed by another session/account or after epoch change
- **THEN** one-use original owner admission refuses it

### Requirement: Only explicit eligible primary events can be changed
The system SHALL admit writes only from direct authenticated requests to a freshly verified
owner-primary calendar and eligible attendee-free timed nonrecurring events, using exact ETags.

#### Scenario: Owner creates or edits a solo event
- **WHEN** the explicit request and all owner/session/revision/eligibility checks are valid
- **THEN** dispatch is durably committed before one provider mutation with invitations/email disabled

#### Scenario: Foreign, stale or invitation-bearing event
- **WHEN** calendar ownership, ETag, session revision or solo-event eligibility is invalid
- **THEN** no write dispatch is admitted and no attendee invitation or notification email is requested

### Requirement: Bounded reads do not imply deletion
The system SHALL scope event reads to an explicit max31-day range and disclose partial pages.

#### Scenario: A page omits events
- **WHEN** provider pagination or a bounded range returns only some events
- **THEN** the UI shows that bound and does not delete omitted events

### Requirement: Committed unknown work is looked up without replay
The system SHALL retain exact UUID/payload/dispatch/provider receipts and SHALL NOT blindly
resend an unknown create/update/delete after response loss, restart or retry.

#### Scenario: Lost acknowledgement
- **WHEN** an exact request is retried after an uncertain committed outcome
- **THEN** the stored state is returned and lookup reads the same provider event ID without mutation

#### Scenario: Exact operation evidence versus current absence
- **WHEN** lookup finds matching operation/digest and requested current create/update fields
- **THEN** saved is confirmed
- **AND WHEN** delete lookup only finds404/410
- **THEN** current absence is reported without claiming this request caused deletion

#### Scenario: Withdrawal or installation restore
- **WHEN** session/account/connection/app authority is withdrawn or a backup is restored
- **THEN** new dispatch and stale private result/refresh are fenced
- **AND** restored unfinished operations are quarantined without resend
