## ADDED Requirements

### Requirement: Private Tailscale connection lifecycle
The installer and CLI SHALL connect an existing local Tailscale device to an
exact private HTTPS origin, retain Moosuri account authentication and preserve
unrelated Serve configuration. R13/F16/F62.

#### Scenario: Connect during setup or afterward
- **WHEN** the current node is connected and its HTTPS certificate domain is available
- **THEN** the origin is detected, an owned Serve mapping is created and the managed
  server uses that origin while preserving data and login-autostart policy.

#### Scenario: Authentication and platform prerequisites
- **WHEN** the daemon requires login or is unavailable
- **THEN** the user receives bounded existing-profile login or installation guidance;
  secret URLs stay on the controlling terminal, and local/manual setup remains available.

#### Scenario: Conflicting or concurrently changed routes
- **WHEN** a root is already unowned/shared/public, or another writer changes the ETag
- **THEN** the change is refused without replacing that configuration.

#### Scenario: Interrupted external or local configuration
- **WHEN** connection stops after Serve mutation or during origin/example publication
- **THEN** start/update admission remains closed and explicit retry finishes using
  its durable receipt, without another external write for an already matching route.

#### Scenario: Disconnect or uninstall
- **WHEN** an owned binding is removed
- **THEN** only the unchanged mapping belonging to the same node is removed;
  disconnect restores the saved origin and unrelated/changed routes remain intact.

#### Scenario: PWA reachability and application authentication
- **WHEN** a mapping has been saved
- **THEN** configuration and actual HTTPS readiness are reported separately; normal
  Moosuri sign-in, secure cookies and exact-origin validation still apply.
