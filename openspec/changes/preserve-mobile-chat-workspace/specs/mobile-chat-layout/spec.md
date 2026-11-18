## ADDED Requirements

### Requirement: Visible workspace at normal zoom
The application SHALL fit the normal-scale visual viewport when it is available,
retain dynamic-viewport CSS fallback, and SHALL NOT disable user zoom.

#### Scenario: Keyboard-sized viewport
- **WHEN** the visible viewport shrinks during drafting
- **THEN** the composer remains inside the application frame and the draft remains intact.

### Requirement: Stable conversation position
The conversation SHALL retain bottom following or the visible older run position
when composer height, viewport size or asynchronous content changes.

#### Scenario: Reading before a late image
- **WHEN** a reader is away from the bottom and earlier content grows
- **THEN** the same visible run remains at the same relative scrollport position.

### Requirement: Compact and reachable tools
The idle composer SHALL put the attachment trigger, message field, voice
trigger and send button on one row with accessible names. The attachment trigger
SHALL disclose photo, file and library controls. Expanded tools and long
navigation SHALL remain scrollable.

#### Scenario: Long menu and small viewport
- **WHEN** navigation or recording content exceeds the available height
- **THEN** its controls remain reachable without expanding the application beyond the viewport.

#### Scenario: Attach a file while drafting
- **WHEN** the user opens attachment options and picks a file
- **THEN** the composer shows upload progress or an available preview
- **AND** the existing draft remains available and text submission is independent of unfinished uploads
