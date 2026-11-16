## ADDED Requirements

### Requirement: Literal titles do not create a compound research task
The system SHALL keep research words inside a quoted output title separate from
operations requested outside that title.

#### Scenario: Save an existing answer
- **WHEN** the owner asks to save prior content as the document “여행 취소 계산”
- **THEN** the request is eligible for isolated local document creation
- **AND** a research word in its title alone does not suppress that classifier

### Requirement: A current edit can name its immediately saved record
The system SHALL resolve a bounded literal previous-record reference using an
actual current owned save receipt and SHALL recheck its revision before editing.

#### Scenario: Rename and append after restart
- **WHEN** a completed save is followed by “방금 만든 문서” title change, then “그 문서” content addition
- **THEN** the same document receives new revisions after actual pinned reads
- **AND** request replay never repeats the mutation, including across reopen

#### Scenario: No trustworthy recent target
- **WHEN** the previous record is stale, withdrawn, from another context, or conflicts with an exact named record
- **THEN** no record is changed by guessing its identity
- **AND** existing owner-edited text remains intact
