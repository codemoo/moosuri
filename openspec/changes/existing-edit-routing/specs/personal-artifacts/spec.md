## ADDED Requirements

### Requirement: Current existing-document edits retain the document route

An explicit current-user request to read, edit and save one existing document
SHALL retain isolated artifact intent admission. Read/save cost cues SHALL NOT
permanently replace its mutation profile with a read-only profile. Quoted title
or body text SHALL NOT act as an edit instruction. Genuine source/calculation to
a new private report SHALL retain the compound-report route.

#### Scenario: Exact named edit without a composer target

- **WHEN** the owner names one existing document and requests a read, edit and save
- **THEN** the saved receipt identifies the same document with its next revision
- **AND** no duplicate document or background mission is created

### Requirement: A current named target agrees with the composer pin

Before sealing an artifact edit, the Store SHALL validate any provided target
quote against the current literal request. An explicit named target SHALL agree
with the selected document. A mismatch SHALL clarify without mutating either
document. A null target quote or a supported unquoted deictic reference MAY use
the explicitly selected document.

#### Scenario: Selected A and named B conflict

- **WHEN** the composer selects document A and the current request names document B
- **THEN** the request clarifies before exposing a writable artifact grant
- **AND** both document identities, revisions and contents remain unchanged

#### Scenario: Explicit selection remains usable

- **WHEN** the current edit has a matching name, a supported deictic reference or no target quote
- **THEN** its stored receipt identifies the selected document with its next revision

#### Scenario: Repeated quoted deictic title remains a literal target

- **WHEN** selected document A conflicts with quoted title B, and that deictic-looking title also appears in quoted body text
- **THEN** repeated occurrences SHALL NOT turn B into an unquoted reference to A
- **AND** both documents remain unchanged
