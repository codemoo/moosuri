## ADDED Requirements

### Requirement: Public or private remote access
Moosuri SHALL offer Tailscale private access and user-owned public HTTPS access while retaining local setup for users who defer networking. R13/F16/F62.

#### Scenario: Public native HTTPS
- **WHEN** the user configures a domain and reachable external TCP443
- **THEN** native TLS issues and renews certificates with private atomic account/certificate caches, retains ordinary account authentication and ignores spoofed forwarding headers.

#### Scenario: Existing HTTPS deployment
- **WHEN** the user retains an existing proxy or private split-DNS deployment
- **THEN** unrelated TLS services remain unchanged and previous HTTPS access behavior is preserved outside the new managed public mode.

#### Scenario: User-registered domain validation
- **WHEN** the user provides a domain whose DNS they already registered
- **THEN** the installer resolves it and requires a valid HTTPS response containing an independent one-use value from this server before activating public application access; it never creates DNS records or accepts DNS API tokens.

#### Scenario: Interrupted access changes
- **WHEN** domain verification or local origin publication is interrupted
- **THEN** the installation-bound intent supports resume or previous-origin restoration without losing accounts or changing DNS records.

#### Scenario: Real reachability and removal
- **WHEN** status or disconnect is requested
- **THEN** DNS/local/HTTPS observations remain distinct; disconnect restores prior access and retains DNS/certificate material, and removal preserves private data.
