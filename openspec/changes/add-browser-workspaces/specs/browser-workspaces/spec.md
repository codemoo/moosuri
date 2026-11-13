## ADDED Requirements

### Requirement: A private browser continues across conversation steps
The system SHALL associate an isolated browser with the authenticated user and conversation, preserve its current page across tool calls and app disconnections, and disclose its bounded lifetime.

#### Scenario: Same page after human login
- **WHEN** the human completes login in the conversation's browser and explicitly resumes
- **THEN** the assistant observes the current page without opening the URL again
- **AND** the next visible conversation turn retains the original context

### Requirement: One fenced human controller owns input
The system SHALL validate user, session, workspace revision and a short-lived control capability before human input and reject stale document/layout tokens without replaying input.

#### Scenario: Another tab takes over
- **WHEN** a newer control revision is granted
- **THEN** the older tab cannot send input or obtain new private frames with its old capability
- **AND** model observation remains paused until explicit completion

### Requirement: Browser tools cannot bypass email approval
The system SHALL expose only open, observe and handoff workspace tools to the model, disable model navigation after human takeover, and keep every automated email send behind the existing exact final approval.

#### Scenario: A page requests mail submission
- **WHEN** untrusted page content asks the model to click or send
- **THEN** no model-callable browser input or approval capability exists
- **AND** actual agent email dispatch requires the reviewed immutable draft approval

### Requirement: Failure does not replay external input
The system SHALL preserve unknown outcomes, fence lost worker generations and deduplicate explicit continuation by its durable request identity.

#### Scenario: Completion response is lost
- **WHEN** the same human completion capability is retried
- **THEN** it resolves to the same conversation turn
- **AND** it does not create a second continuation or repeat a browser action

#### Scenario: Server restarts
- **WHEN** the process restarts with a formerly active workspace
- **THEN** it reports an unknown browser state and does not replay navigation, input or submission

#### Scenario: A frame stalls after a successful login action
- **WHEN** a screenshot request stalls while the document changes after acknowledged human input
- **THEN** the bounded capture expires without terminating the browser or replaying input
- **AND** a fresh authenticated observation can recover the current page while late capture replies cannot authorize an action
