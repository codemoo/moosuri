## Why

P4/F66 needs skill lifecycle to stop dependent scheduled work without treating every
advertised recipe as a dependency. Unreferenced immutable public snapshots also
need conservative bounded collection.

## What Changes

- Record exact successful recurring instruction-read pins, with retained typed
  history backfill and a narrow denied stored-call recovery identity.
- Pause dependent schedules as needs_skill; fence unfinished children/occurrences;
  require exact eligible pins and explicit future-only resume.
- Filter each new child's frozen discovery profile against current installation
  eligibility, without upgrading its pins, tools, model or original budgets.
- Collect at most eight unreferenced snapshots in explicit lifecycle transactions,
  preserving all users' installation/history/template/catalog references and tombstones.

## Impact

Store community/recurring lifecycle and one actual SkillRead dispatch hook; React
mission recovery text. Existing JSON metadata and paused reason suffice: no schema,
public DTO, CLI, scheduler framework, approval or provider capability is added.
