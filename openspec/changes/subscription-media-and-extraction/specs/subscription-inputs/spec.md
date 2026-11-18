## ADDED Requirements

### Requirement: Read attached evidence with the selected subscription

Moosuri SHALL use the owner’s selected subscription to analyze supported attachments. It SHALL require an exact image proof before delivering pixels and exclude tools from media turns.

#### Scenario: Model cannot verify pixels
- **WHEN** the selected model returns the wrong answer to an independent image challenge
- **THEN** the server does not admit images for that owner, binding revision, model and effort

#### Scenario: Conversation moves to a subscription connection
- **WHEN** an earlier attachment revision was selected only on another connection
- **THEN** the owner must select it for this connection before a follow-up can send its contents

### Requirement: Extract memory only from a live collection grant

The extraction worker SHALL use the collection grant’s exact source and subscription binding. It SHALL fence late results against the same source revision and grant.

#### Scenario: Source withdrawn while the model runs
- **WHEN** the owner deletes a source after dispatch
- **THEN** a later response cannot create memories from it or trigger a second model call
