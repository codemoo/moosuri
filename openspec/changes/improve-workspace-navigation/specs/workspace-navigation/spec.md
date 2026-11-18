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

### Requirement: Distinct chat-first workspace destinations

The workspace SHALL use Astryx's dark theme for the shell and keep chat as the
default. A compact Astryx SideNav rail SHALL expose chat, conversation search,
incoming notices, ideas, goals, and the media library on wide screens. The
workspace SHALL open recent conversations, secondary destinations, and account
settings in an on-demand drawer. On narrow screens the drawer SHALL also
provide the primary destinations, and menu, search and new-chat entry points
SHALL remain visible at the top.

#### Scenario: Find an owned conversation
- **WHEN** the owner opens title search and selects a result
- **THEN** the existing conversation opens and the search closes
- **AND** the owner's drafts stay bound to their original conversations

#### Scenario: Open a destination without loading unrelated forms
- **WHEN** the owner opens notices, goals, or the library directly
- **THEN** the app shows the corresponding records, with schedule editing kept in its own screen

#### Scenario: Compact screens and consistent app identity
- **WHEN** the user opens sign-in, conversation search, model settings or the PWA installer
- **THEN** these surfaces use Moosuri’s shared Astryx dark theme
- **AND** the shell and install surfaces use the same folded-collar icon
- **AND** necessary input, status and confirmation text remains available

#### Scenario: Open conversation history from the desktop rail layout
- **WHEN** the owner opens the workspace drawer and finds a prior conversation
- **THEN** the drawer closes when that conversation is selected
- **AND** the main navigation remains compact and the selected conversation retains its draft
