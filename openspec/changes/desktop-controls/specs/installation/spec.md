## ADDED Requirements

### Requirement: Minimal desktop server controls
The installed assistant SHALL provide a Rust desktop menu for supported macOS and
Linux desktop sessions while retaining the authenticated web app for chat and
settings. Desktop controls SHALL operate on the complete installed server and
clearly distinguish that scope from per-user tasks. Headless services SHALL not
require a graphical session; WSL2 SHALL retain its Linux service/web access.

#### Scenario: The browser and menu close
- **WHEN** an owner closes their browser or quits the menu app
- **THEN** the managed server and authorized scheduled work continue independently.

#### Scenario: Stop versus login-autostart
- **WHEN** an owner starts or stops the server from its menu
- **THEN** the saved login-autostart choice is unchanged; toggling login-autostart does not stop a running server.

#### Scenario: A service exists but is not ready
- **WHEN** a user service is loaded or enabled but its process/dashboard is unavailable
- **THEN** the menu does not report it as a ready running server and exposes concise diagnostics.

#### Scenario: Existing desktop entries and removal
- **WHEN** setup or uninstall encounters modified or unrelated shortcut/service files
- **THEN** it preserves those files; ordinary removal stops only its owned menu/server and retains private data.

#### Scenario: Platform desktop support is absent
- **WHEN** installation occurs over SSH, on a headless host, or inside WSL2
- **THEN** CLI/service/web access remains available without requiring a native tray; Linux tray visibility additionally requires a StatusNotifier host.
