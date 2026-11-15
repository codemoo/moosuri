## ADDED Requirements

### Requirement: Explicit bounded missing-field continuation
The system SHALL complete an explicit task or Calendar request from a following authenticated answer while preserving supplied typed fields. Only bounded immediately preceding clarification with matching authority SHALL contribute prior request text. New values SHALL be supported by the current answer. Recording and applying a mission schedule SHALL independently validate the full lineage. General history, retrieval and model text SHALL NOT create or expand action authority.

#### Scenario: Continuation lifetime and short timing quotes
- **WHEN** the immediately preceding completed clarification matches owner/thread/session/epoch/model and its original30-minute expiry has not passed
- **THEN** at most four literal answers may complete the request without repeating supplied fields or routine approval
- **AND** a short answer timing quote is allowed only while full-lineage validation preserves original weekday/date constraints

#### Scenario: Weekly task missing clock
- **WHEN** an owner explicitly requests a weekly Monday read task without a clock and then answers9AM
- **THEN** one existing mission schedule SHALL preserve Monday and the original task and use09:00
- **AND** each turn SHALL use only one bounded interpreter call and require no approval popup.

#### Scenario: Calendar create missing duration
- **WHEN** an explicit Calendar create has title/date/start but lacks end/duration
- **THEN** only end/duration SHALL be asked
- **WHEN** the same original owner/session replies45minutes
- **THEN** the actual typed service SHALL submit one create with that duration and show its real receipt.

#### Scenario: Scope or authority changes
- **WHEN** a proposal changes the original operation, title/task or completed fields, or an answer supplies no missing value
- **THEN** no action SHALL commit.
- **WHEN** a different thread/session/account/model, intervening ordinary turn, expiry, Calendar pin withdrawal or restore breaks lineage
- **THEN** no pending action SHALL be inherited or replayed.

#### Scenario: Numeric Calendar answer preserves authenticated message boundaries
- **WHEN** a saved Calendar start quote ends its original message and a later authenticated reply supplies45minutes
- **THEN** the later digits SHALL NOT invalidate the previously validated clock, including after reopening storage
- **AND** the typed create SHALL preserve the original start and add exactly45minutes.
- **WHEN** a new temporal quote occurs in both earlier prose and a malformed current reply
- **THEN** only the current reply SHALL supply the new field and no earlier occurrence SHALL rescue it
- **AND** newlines within a single message SHALL retain the complete maximal-span clock validation.

#### Scenario: Malformed short quote drops a weekday
- **WHEN** an original Monday request is completed by9AM but the interpreter proposes Daily09:00 with only the short answer as its time quote
- **THEN** Store SHALL reject before saving a schedule.
- **WHEN** the proposal preserves WeeklyMonday09:00 with that short answer quote, including for a task longer than1024bytes
- **THEN** full-lineage civil validation SHALL preserve the weekday without copying the long task into the persisted time quote.

### Requirement: Actual task receipt identifies current control
A current explicit task control MAY use the immediately preceding same-owner/thread/session actual task receipt as the selected target identity, while preserving its exact revision and all existing owner/uncertainty/budget checks. A separately selected current target SHALL take precedence. Ordinary noncandidate chat SHALL not acquire a classifier call merely because a receipt exists.

#### Scenario: Pause the saved task
- **WHEN** the owner asks to pause that task after its actual saved receipt
- **THEN** only that actual task at the saved revision SHALL be controlled with the current request as authority.
- **WHEN** that revision is stale, unknown or withdrawn
- **THEN** the system SHALL reject or clarify without choosing another task.

### Requirement: Actual receipt identifies a current follow-up target
A current explicit deictic Calendar change MAY use only the immediately preceding same-thread actual applied create/update receipt as target identity. Store SHALL preserve its owned connection/event ID and require a fresh unchanged ETag and existing Calendar eligibility. The receipt SHALL NOT grant a new action by itself; unknown/deleted/revoked outcomes SHALL NOT create such identity authority.

#### Scenario: Move the just-saved event
- **WHEN** the current owner says move that event to an explicit destination date after an actual applied receipt
- **THEN** the exact saved event SHALL be read and updated once, preserving known actual duration
- **AND** source and destination dates SHALL remain distinct and date-only movement SHALL preserve the local start clock including seconds.

#### Scenario: DST and stale target
- **WHEN** the preserved local clock falls in an IANA gap/fold or the current ETag differs from the saved receipt
- **THEN** no provider write SHALL occur and the user SHALL be asked to resolve the current value/target.

### Requirement: Existing personal and external authority remains bounded
Personal ChatGPT continuation SHALL remain foreground Calendar-only. Background missions SHALL retain existing verified profile admission. Calendar scope SHALL remain primary owner solo timed nonrecurring and notification-free. Mail sending SHALL retain final preview approval and SHALL NOT use this continuation grant.

#### Scenario: A clarification requests a new external effect
- **WHEN** a following answer changes a pending Calendar request into mail sending or a background task
- **THEN** the Calendar continuation SHALL NOT authorize that effect
- **AND** mail still requires its complete final preview and explicit send approval
