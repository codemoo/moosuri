## ADDED Requirements

### Requirement: Recoverable initial installation
Initial installation SHALL publish the complete verified runtime and guided setup
choices atomically, without replacing an existing destination directory.

#### Scenario: Interrupted first installation
- **WHEN** the process stops before initial publication
- **THEN** the final runtime path remains absent and the same installation can retry;
  private data and unrelated paths remain unchanged.

#### Scenario: Interrupted remote setup after publication
- **WHEN** HTTPS or Tailscale setup stops before administrator creation
- **THEN** rerun preserves the exact chosen internal port and the published runtime.

### Requirement: Serialized setup and truthful readiness
Setup SHALL serialize state mutations with start/stop/autostart/removal and honor
runtime writer admission. Readiness SHALL have bounded memory and elapsed time.

#### Scenario: Another command while setup is waiting for input
- **WHEN** a control command overlaps the setup wizard
- **THEN** it reports that control is busy before mutating state or managed files.

#### Scenario: Setup while a foreground runtime is ready
- **WHEN** the installed foreground server owns runtime admission and is ready
- **THEN** setup preserves accounts/settings and returns the existing web entry
  without opening another database writer.

#### Scenario: Browser during slow startup
- **WHEN** setup starts a foreground server or the user opens a desktop web entry
- **THEN** the browser opens only after local readiness and the embedded web page
  have passed checks; readiness waiting ends within a fixed total budget.

### Requirement: Actionable local controls
The desktop SHALL expose only usable web actions and retain sanitized failure
guidance until a subsequent non-help action succeeds or the menu restarts.

#### Scenario: Stopped server and failed action
- **WHEN** the server is stopped or an action fails
- **THEN** web actions are unavailable until ready, and failures identify the
  appropriate status, foreground, service, update or recovery path without raw logs.

#### Scenario: Menu exit with a queued action
- **WHEN** quit is requested while another action is queued
- **THEN** the quit intent cannot be dropped and no further queued action starts;
  already-started lifecycle work remains bounded and server lifetime stays separate.
