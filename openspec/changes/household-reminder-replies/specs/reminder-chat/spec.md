## ADDED Requirements

### Requirement: A short activity reply completes the existing reminder draft
The system SHALL accept bounded literal Korean activity labels for an immediately
pending owned missing-content question while preserving its saved time and timezone.

#### Scenario: Answer after reopen
- **WHEN** the owner asks “내일 오전 7시에 알려줘.”, reopens and answers “우산 챙기기”
- **THEN** one reminder has that literal message and the original local time
- **AND** exact request replay returns the same run without a model call or duplicate reminder

#### Scenario: Natural cancellation preserves saved reminders
- **WHEN** the owner answers a pending reminder question with “됐어, 취소해 줘.”
- **THEN** only that unsaved draft closes with a truthful local confirmation
- **AND** saved reminders remain unchanged and model dispatch count stays zero

#### Scenario: New or ambiguous instructions
- **WHEN** a reply is quoted, conditional, a new command, or supplies a conflicting time phrase
- **THEN** it SHALL NOT be adopted as an unlabelled activity value for the pending draft
