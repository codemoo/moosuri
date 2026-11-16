## ADDED Requirements

### Requirement: Provider connection continuation retains original owner guidance
For the existing bounded mission foundation (R23, F27), a stable provider
connection continuation MUST retain accepted pending and previously applied live
instructions in the same carrier's resumed model context. It MUST retain each
original receipt request ID, text and admission attempt while transferring its
execution ownership to the current bounded carrier attempt.

#### Scenario: Pending guidance at stable yield
- **WHEN** owner guidance is accepted after a completed read and before a stable provider connection yield
- **THEN** the resumed carrier exposes that same receipt as pending guidance
- **AND** its next admitted step can apply it once

#### Scenario: Applied guidance at stable yield
- **WHEN** a completed step applied owner guidance before a provider connection yield
- **THEN** the resumed carrier exposes the same applied guidance for its reconstructed model context
- **AND** the completed read is not replayed

#### Scenario: Original boundaries survive continuation
- **WHEN** a supported provider connection wait resumes at attempt1 or attempt32
- **THEN** the same model/profile, checkpoint, receipts, deadline and model/tool/repair accounting remain
- **AND** a retired or cancelled lease cannot apply instructions
- **AND** cancellation closes later undispatched guidance without applying it
