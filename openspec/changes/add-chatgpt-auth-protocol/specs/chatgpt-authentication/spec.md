## ADDED Requirements

### Requirement: Official bounded ChatGPT authentication protocol

The connector SHALL use only exact official HTTPS discovery and advertised exact supported authorization/token/revocation/JWKS endpoints, validate public-client RS256/S256 support, and bound DNS, complete request duration and bodies without redirects, proxies or retries.

#### Scenario: Discovery or HTTP confusion

- **WHEN** discovery changes origin/path/mechanism, HTTP redirects, exceeds a body bound, stalls or truncates a response
- **THEN** authentication fails with sanitized errors and no credential forwarding to another endpoint

### Requirement: Consuming registration attempts

The connector SHALL create fresh state/nonce/S256 PKCE, accept only the original exact127.0.0.1 callback within its TTL, reject duplicate/ambiguous parameters and require an issued client ID for initial dynamic registration. Accepted or rejected callbacks SHALL consume the in-process attempt.

#### Scenario: Invalid callback

- **WHEN** callback state/TTL is invalid, an initial issued client is missing, or callback parameters conflict
- **THEN** no pending redemption is returned and the in-process attempt cannot be reused

### Requirement: Pinned returning authentication

Issued-client reauthorization SHALL retain the exact pending client ID if the callback omits it and reject any supplied mismatch. Returning authentication SHALL pin client, host and a previously verified subject.

#### Scenario: Returning client identity

- **WHEN** a returning callback omits its client ID or supplies a different one
- **THEN** omission retains the pinned issued client while a mismatch fails
- **AND** the later signed subject must still match the previously verified identity

### Requirement: Incomplete registration recovery

A retained incomplete issued registration SHALL support fresh reauthorization after code rejection/expiry with fresh state/nonce/PKCE, the same client/host and no initial registration hint, without forging a verified subject.

#### Scenario: Issued registration recovers after first code failure

- **WHEN** the first exchange fails with invalid_grant or an expired code before identity is verified
- **THEN** the retained issued registration starts a fresh authorization on the new loopback port using the same client/host, accepts an omitted callback client ID, rejects a supplied mismatch and establishes identity only through verified signed claims

### Requirement: Verified redemption boundary

Pending redemption SHALL expose the issued registration before consuming exact code-exchange fields. Signature and issuer/audience/expiry/issue-time/nonce/subject checks SHALL precede identity acceptance.

#### Scenario: Invalid signed identity

- **WHEN** a token has an invalid signature/key/claim
- **THEN** no verified bundle is returned and pending redemption cannot be reused

### Requirement: Granted plan scope and private carriers

The connector SHALL base plan permission on token-response scopes, preserve identity-only plan-disabled status and retain secrets in private non-debug/nonserializable carriers.

#### Scenario: Identity without plan permission

- **WHEN** a valid identity response lacks plan scopes even if callback scopes claim them
- **THEN** sign-in is plan-disabled and plan access-token retrieval fails

### Requirement: Exact rotation and revocation

The connector SHALL omit scope on refresh, return full replacement bundles and accept empty successful revocation. Known terminal token failures SHALL differ from temporary transport/HTTP failures.

#### Scenario: Refresh rotation or remote logout failure

- **WHEN** refresh succeeds or a temporary failure/revocation failure occurs
- **THEN** success yields a complete replacement while the old carrier remains unchanged, temporary failure does not assert terminal invalidation, and remote revocation remains unconfirmed on failure

### Requirement: Application ownership remains a caller duty

The adapter SHALL NOT imply that process-memory consumption supplies durable user ownership, refresh single-flight, atomic persistence or local logout.

#### Scenario: Protocol success without application integration

- **WHEN** the connector returns a verified bundle
- **THEN** owner-authorized persistence, refresh reservation and local logout still require separate application implementation before personal selection is enabled
