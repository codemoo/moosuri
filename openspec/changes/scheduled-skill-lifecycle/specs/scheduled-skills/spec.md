## ADDED Requirements

### Requirement: Actual scheduled skill dependencies
Recurring tasks SHALL record only exact successful instruction reads or a concrete
admitted stored read denied by withdrawal as dependencies, rather than advertised
recipe discovery. New children SHALL use only currently eligible pins within the
original frozen profile and SHALL NOT gain newly installed or upgraded recipes.

#### Scenario: Unrelated discovery and no prior run
- **WHEN** a discoverable skill is disabled or removed without any actual dependency
- **THEN** unrelated schedules remain active and their next children cannot read the withdrawn stale pin
- **AND** another user's installation and schedules remain unchanged

### Requirement: Exact dependency withdrawal and explicit recovery
Disabling/removing an actual dependency SHALL pause the parent with needs_skill,
fence pending occurrences and unfinished children, and retain original pins,
accounting/tool receipts and budgets. Enabled exact active/previous snapshots SHALL
remain eligible across update/rollback. Re-enable SHALL NOT silently resume.

#### Scenario: Read, update, disable and resume
- **WHEN** a child successfully reads a pinned skill and its installation updates or rolls back
- **THEN** the retained original snapshot remains the dependency without replacement
- **WHEN** that snapshot becomes unavailable or disabled
- **THEN** dependent work stops and late output cannot restore execution
- **WHEN** the exact dependency is eligible and the owner explicitly resumes
- **THEN** only future occurrences become eligible with no replay of old children

### Requirement: Conservative immutable snapshot collection
Lifecycle maintenance SHALL remove only snapshots with zero retained consumer
references and SHALL bound deletions per mutation. Other users, rollback pointers,
admitted/historical chat pins, recurring profiles/dependencies and cached catalog
consumers SHALL protect their exact bytes. Tombstones SHALL remain.

#### Scenario: Orphan beside a historical pin
- **WHEN** lifecycle maintenance encounters an orphan preview and a snapshot referenced by execution history
- **THEN** only the unreferenced snapshot can be removed
- **AND** an evicted uninstalled preview requires exact re-preview rather than replacement
