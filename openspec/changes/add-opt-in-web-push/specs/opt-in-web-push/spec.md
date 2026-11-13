## ADDED Requirements

### Requirement: Explicit private device enrollment
The system SHALL request browser notification permission only from a user gesture,
bind subscription authority to current account/device/session/epoch/generation, and
exclude endpoints and subscription keys from ordinary/admin metadata DTOs and logs.

#### Scenario: Shared browser changes account while enrollment is pending
- **WHEN** an older account's enrollment response arrives after another account or local operation takes ownership
- **THEN** it cannot overwrite the current binding or create a retry under the new account
- **AND** an uncertain retry preserves its original request UUID and exact payload

#### Scenario: Newer on or off intent overtakes an older request
- **WHEN** two tabs allocate successive durable device intent revisions and browser, POST or ACK work completes out of order
- **THEN** lifetime and generation checks fence stale browser work and durable server receipts reject stale new requests before replacing authority
- **AND** exact request replay never repeats mutation, including off replay after a newer on
- **AND** successful enrollment orders that device across accounts while an account's off intent cannot retire or order another account's binding

#### Scenario: Explicit off followed by immediate on
- **WHEN** a signed-in user turns device notices off then on in the same settings view
- **THEN** off clears display authority while preserving the authenticated account lifetime
- **AND** the newer on can enroll with a greater durable revision while logout still ends that lifetime

### Requirement: Exclusive browser subscription ownership
The system SHALL serialize subscription lookup, unsubscribe, subscribe and reuse
across tabs with a native origin/service-worker Web Lock, without expiring ownership
while browser mutation remains pending. It SHALL recheck the current account and
device generation after acquiring ownership and after asynchronous browser work.
Explicit off and account-switch cleanup SHALL immediately fence local authority and
coordinate with the same browser ownership.

#### Scenario: An old unsubscribe is already in flight
- **WHEN** a newer tab starts enrollment while an older tab's unsubscribe is pending
- **THEN** the newer operation waits for exclusive browser ownership before looking up or reusing a subscription
- **AND** after the old mutation finishes it reads or creates a live subscription before enrollment
- **AND** a queued intent superseded by off or account switch cannot mutate browser state after acquiring ownership

### Requirement: Unsupported browser ownership stays inactive
Missing native Web Locks SHALL make enrollment unsupported before permission or
subscription mutation, without an expiring lease fallback.

#### Scenario: Browser has no native Web Locks
- **WHEN** the user opens notification settings in a browser without Web Locks
- **THEN** enrollment is shown as unsupported and no permission or subscription mutation is requested

### Requirement: Bounded honest push delivery
The system SHALL use a bounded durable fair queue and recheck authority immediately
before external dispatch.2xx SHALL mean push-service acceptance only; expired endpoints,
known backoff, unknown network outcomes and restore quarantine SHALL remain distinct.
Only approved HTTPS push-service destinations with public pinned DNS and no redirects,
proxy or implicit retry SHALL receive requests.

#### Scenario: Logout or read races request preparation
- **WHEN** current session/account eligibility or unread state changes before dispatch commitment
- **THEN** no provider request is authorized
- **AND** reminder inbox and text chat remain independent of push failure

#### Scenario: Unknown network outcome survives restart
- **WHEN** a committed dispatch has no verified service result
- **THEN** it remains unknown without automatic resend and does not claim device display or reading

### Requirement: Generic account-fenced service-worker display
The service worker SHALL display fixed generic text only after current local generation
and passive authenticated authorization succeed. It SHALL not cache private API/content
or accept a remote notification route. Existing app navigation SHALL preserve drafts.

#### Scenario: Old push arrives after account switch
- **WHEN** an old binding's event or held authorization response is observed under another account
- **THEN** no private or stale-account notification is displayed or opened
- **AND** only a fixed same-origin schedules route can be opened for a current binding

### Requirement: Retired restore authority
The system SHALL keep VAPID private material in an optional runtime-only private file,
never in exports/source/browser/logs. Store/stopped opens SHALL not generate keys.
Staged export and restore hooks SHALL clear private subscription/session carriers,
retire signing metadata and quarantine unfinished work before activation.

#### Scenario: Restore requires fresh enrollment
- **WHEN** a sanitized snapshot is activated
- **THEN** previous subscription/signing authority cannot send or display new work automatically
- **AND** a fresh authenticated explicit enrollment can create new authority while inbox records remain intact
