## ADDED Requirements

### Requirement: Deliberate destination focus
The workspace SHALL offer a keyboard-visible bypass link and focus its current
heading after explicit navigation. Background updates SHALL NOT move focus.

#### Scenario: Mobile menu selects Calendar
- **WHEN** the user chooses Calendar from the conversation drawer
- **THEN** the drawer closes and the Calendar heading receives focus
- **AND** returning to chat preserves its unsent draft

#### Scenario: Background refresh while typing
- **WHEN** the workspace refreshes its known session or saved conversation
- **THEN** the input keeps focus unless the user explicitly navigates

### Requirement: Accurate navigation and removed-control recovery
Navigation SHALL mark the actual page, label Gmail and device settings accurately,
and restore dialog focus to a connected visible destination when its original
control no longer exists.

#### Scenario: Settings and deletion
- **WHEN** a settings page is open
- **THEN** an unrelated conversation is not marked as the current page
- **WHEN** deletion removes the invoking conversation control
- **THEN** focus moves to the remaining workspace heading
