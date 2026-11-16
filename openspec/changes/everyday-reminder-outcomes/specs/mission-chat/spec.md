## ADDED Requirements

### Requirement: Model clarification fields match stored reason codes
The system SHALL expose the same finite clarification reason vocabulary to the
model and Store, while keeping user-facing unsupported outcomes locally generated.

#### Scenario: Unsupported booking request
- **WHEN** the current owner asks for an unsupported automatic booking and the classifier returns the declared unsupported reason
- **THEN** the chat explains that no task was saved or scheduled
- **AND** no booking, reminder or delegated mission is created
