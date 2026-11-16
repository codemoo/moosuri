## ADDED Requirements

### Requirement: Guided bilingual preview setup
The installer SHALL provide one-command bootstrap and English/Korean interactive
setup on supported macOS and Linux architectures, including Linux inside WSL2.

#### Scenario: First local installation
- **WHEN** a regular OS user runs the bootstrap in a terminal and accepts local defaults
- **THEN** it verifies the pinned archive and signed bundle, creates one administrator,
  offers optional model setup and starts or clearly explains how to start the dashboard.

#### Scenario: Secret input and temporary password
- **WHEN** the user enters a password/API key or requests a temporary password
- **THEN** input is hidden and the generated password is shown only on the controlling
  terminal, while mandatory first password change and account isolation remain enforced.

#### Scenario: Interrupted or repeated setup
- **WHEN** setup is repeated after signed runtime activation
- **THEN** completed account creation is preserved and only known setup steps resume;
  unrelated runtime paths, services and modified managed files are not overwritten.

### Requirement: Owned service and local access
The installer SHALL support loopback-only local HTTP and exact user-owned service
management while preserving explicit HTTPS/Tailscale configuration.

#### Scenario: Service unavailable
- **WHEN** the current user has no usable launchd/systemd session
- **THEN** installation remains usable through foreground mode and does not claim
  unattended startup or modify system services.

#### Scenario: Uninstall
- **WHEN** the user explicitly removes the installed runtime
- **THEN** only unchanged managed runtime/service files are removed and private data remains.
