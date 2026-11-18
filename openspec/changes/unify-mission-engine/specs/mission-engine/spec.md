# Mission execution

## ADDED Requirements

### Requirement: Execute delegated work on the originally admitted assistant engine

Moosuri SHALL use an owner-authorized binding with verified function tools for foreground chat, task planning, one-off missions and scheduled occurrences. Each dispatch SHALL use the saved model, effort and binding revision. Existing mission and effect grants SHALL apply.

#### Scenario: Scheduled execution after a model change

- **WHEN** the owner selects another model after creating a durable task with a verified Claude subscription or personal compatible model
- **THEN** the task uses its original binding and actual tool receipts to verify completion

#### Scenario: Tool capability or binding is unavailable

- **WHEN** the owner creates a mission without a selected tool-capable model
- **THEN** the server returns a model-specific setup error and the form retains the task while the owner opens model settings

#### Scenario: Revoked bindings and old runs

- **WHEN** a saved tool proof or pinned binding is lost
- **THEN** dispatch cannot silently fall back to another engine, and a historical foreground run retains its original mission permissions
