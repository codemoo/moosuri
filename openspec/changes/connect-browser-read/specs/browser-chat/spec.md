## ADDED Requirements

### Requirement: Pinned optional rendered reads
The system SHALL expose `browser_read` only for profile 4 with an admitted and
currently matching trusted deployment pin. Its model arguments MUST contain only
a public HTTP(S) URL. It MUST NOT expose login, click, form, arbitrary script,
attachment, send or mail-approval capabilities.

#### Scenario: Rendered page read
- **WHEN** a configured run invokes `browser_read` successfully
- **THEN** a fresh isolated capsule returns actual bounded rendered content and
  the current run records a distinct browser-page observation.

#### Scenario: Deployment changed or disabled
- **WHEN** the current deployment is absent or differs from the admitted pin
- **THEN** the slot cannot start or publish browser IO results, including on retry.

### Requirement: Bounded isolated execution and recovery
The system SHALL use only the reviewed Linux capsule, exact pinned image/network/
proxy resources and checked seccomp bytes, with no host-browser fallback. It SHALL
reject excess capacity immediately, bound protocol IO, cancel on lost authority
and await owned subprocess/container cleanup even with stalled stdout. Unknown
started slots MUST NOT automatically reissue IO or replenish run budgets.

#### Scenario: Cancellation during a stalled read
- **WHEN** browser output stalls and run authority is revoked
- **THEN** IO is cancelled, owned cleanup is awaited and late content is not stored.

#### Scenario: Another user's or unrelated container
- **WHEN** identity or ownership checks differ
- **THEN** content is rejected and unrelated resources are not removed.

### Requirement: Honest browser evidence
Browser observations SHALL derive only from matching successfully completed typed
browser slots, retain tenant scope and purge behavior, and label coverage limits.
HTTP results and model prose MUST NOT create browser-page provenance.

#### Scenario: Forged result kind
- **WHEN** an HTTP slot contains a `browser_page` payload
- **THEN** no browser observation is exposed.
