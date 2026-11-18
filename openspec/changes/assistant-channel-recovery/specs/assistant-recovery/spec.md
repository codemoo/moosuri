## ADDED Requirements

### Requirement: Preserve assistant intent and outcomes across channels

The assistant SHALL route explicit task clauses to the planner even when another clause asks to remember something. Channel delivery SHALL distinguish unfinished output from a final answer and isolate a missing model to its requesting account.

#### Scenario: Missing model on a shared Telegram bot
- **WHEN** one paired owner sends text without an effective model
- **THEN** the bot queues setup guidance, acknowledges the update and admits subsequent updates for other owners

#### Scenario: Mixed request
- **WHEN** a memory imperative and a separate explicit scheduled task occur in one turn
- **THEN** the planner receives the entire current request and unsupported combinations have no partial writes

### Requirement: Recover through the owner's selected connection

Collection extraction SHALL pin a verified compatible model to the granting owner and recheck that binding before dispatch and commit. Chat SHALL retain a rejected draft during model setup. Confirmed task mutations SHALL trigger a fresh inventory read.

#### Scenario: Connection withdrawn
- **WHEN** a private compatible connection is withdrawn while opt-in extraction is in flight
- **THEN** a late response cannot save memories or switch to a shared provider

#### Scenario: Recovering an attachment
- **WHEN** image verification is required
- **THEN** the owner can verify the saved model in settings and explicitly resend the original draft
