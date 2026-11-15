## ADDED Requirements

### Requirement: Shared civil expressions use the captured request instant and IANA zone
Reminders, missions and Calendar SHALL deterministically interpret supported
literal relative days, weekdays, current ISO week and next ISO week using the
captured request instant and actual IANA timezone. A same-day past one-shot clock
SHALL NOT roll forward to another week.

#### Scenario: Next ISO Friday crosses year and local day
- **WHEN** the reference is 2026-12-31T15:30Z in Asia/Seoul and the request says 다음 주 금요일 오전 아홉 시 십오 분
- **THEN** its one-shot instant is 2027-01-08T00:15Z, independently of the host timezone

#### Scenario: Bare weekday and current week retain distinct scope
- **WHEN** a request uses a bare weekday or this Friday / 이번 주 금요일
- **THEN** the former selects the upcoming local weekday and the latter the current ISO week, rejecting a past resulting one-shot instant

### Requirement: Clock and frequency authority remain literal and complete
The Store SHALL reject proposed date, clock or frequency that disagrees with its
complete overlapping current source expression. Unrelated literal task/title data
SHALL NOT supply scheduling authority. Malformed clock suffixes SHALL NOT become
content after accepting a shorter numeric prefix.

#### Scenario: Model strips the week or recurrence qualifier
- **WHEN** a temporal quote drops next week from next week Friday or changes explicit weekly recurrence to one-shot
- **THEN** no schedule, delegated mission or Calendar action is created from the altered temporal scope

#### Scenario: Invalid clock tail or unsupported one-shot local time
- **WHEN** a request supplies 오전9시60분, 오전13시, 09:60, contradictory markers or an IANA DST gap/fold
- **THEN** it clarifies without a saved one-shot effect or fallback instant

#### Scenario: Missing and ambiguous temporal values
- **WHEN** the request says next Friday / 다음 금요일, a week without weekday, weekend, morning alone or a missing clock
- **THEN** the system requests the missing unambiguous value instead of inventing a date or clock

### Requirement: Existing continuation and ordinary-chat boundaries are preserved
Existing owner/session/thread, continuation budgets, exact source, original supplied
temporal fields, reference and timezone SHALL remain effective through answer and
reopen. Supported time handling SHALL NOT add an ordinary-chat model classifier.

#### Scenario: Reopen follows a partial time and changed browser zone
- **WHEN** a supplied next-week day and clock awaits AM/PM or missing clock across reopen
- **THEN** the original supplied scope and zone survive, and only the missing field is filled under existing continuation rules

#### Scenario: Ordinary weather and unrelated temporal title words
- **WHEN** ordinary chat says 내일 날씨 알려줘 or a task/title contains temporal words outside the scheduling span
- **THEN** weather remains ordinary chat and literal data does not authorize or revoke a separate valid schedule
