## ADDED Requirements

### Requirement: Bounded owned historical evidence
The assistant SHALL search earlier successful whole turns in the current thread or
explicit owned-conversations scope, preserve original user/assistant roles and time,
and read only exact revisions discovered by that run. Failed, partial, future,
foreign, deleted and invalidated history SHALL be excluded. History SHALL NOT grant
current authorization or be published into saved knowledge or artifacts.

#### Scenario: Recall beyond recent context
- **WHEN** a relevant original turn precedes more than100 synthetic turns
- **THEN** the actual Runtime search/read/final-answer path retrieves its real ID,
  revision, timestamp and role-labelled text within the existing budgets

#### Scenario: Old profile or forged pin
- **WHEN** an old profile omits the recall flag, or a read uses an undiscovered,
  inherited-only, foreign or stale revision pin
- **THEN** no expanded read capability or historical content is returned

### Requirement: Candidate and derived-answer withdrawal
Every returned search candidate and read SHALL record exact source provenance and
inherit upstream evidence dependencies. Context descendants SHALL retain withdrawal
edges. Source-thread or upstream withdrawal SHALL purge derived output/checkpoints,
prevent retry and keep reopened history purged.

#### Scenario: Unread candidate deletion
- **WHEN** a model sees multiple search candidates, reads only one, and an unread
  candidate's thread is deleted
- **THEN** the consuming answer and its context descendants are purged

#### Scenario: Upstream source withdrawal
- **WHEN** saved evidence used by a recalled assistant turn is withdrawn
- **THEN** the recalled answer and its text-only descendants are purged and cannot
  resurrect through retry or reopening the installation
