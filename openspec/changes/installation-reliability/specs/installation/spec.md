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
- **THEN** chat offers an explicit start-and-open action for an owned user service,
  navigation waits for readiness, administration requires readiness, and failures
  identify a safe status, foreground, service, port-conflict, update or recovery path.

#### Scenario: Menu exit with a queued action
- **WHEN** quit is requested while another action is queued
- **THEN** the quit intent cannot be dropped and no further queued action starts;
  already-started lifecycle work remains bounded and server lifetime stays separate.

### Requirement: Installation-bound daily controls
Local controls SHALL verify the intended runtime identity before opening its web
page. Managed serving SHALL bind that identity to its installed data directory.
Restart SHALL serialize stop, writer-release confirmation and start, while
preserving login-autostart policy.

#### Scenario: Another instance on the saved port
- **WHEN** a different installation or generic server answers the local health probe
- **THEN** controls report a port conflict and do not start or open that server.

#### Scenario: Stopped shortcut without a service manager
- **WHEN** the user invokes start-and-open in a terminal without a user service
- **THEN** the terminal hosts the foreground server, explains its lifetime and
  opens the browser only after bounded readiness; no hidden daemon is created.

#### Scenario: Existing service file collision
- **WHEN** a new service install finds a different existing service file
- **THEN** it preserves that file, does not claim ownership, and setup can finish
  in foreground mode; uninstall does not remove the unrelated service.

#### Scenario: WSL shortcut refresh interruption
- **WHEN** a generated legacy shortcut is refreshed or refresh stops before state save
- **THEN** setup and removal recognize only exact old/new templates for that prefix;
  unrelated changes are preserved, and current shortcuts use start-and-open.
