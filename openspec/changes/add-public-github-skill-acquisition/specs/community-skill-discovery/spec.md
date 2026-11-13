## ADDED Requirements

### Requirement: Immutable public GitHub acquisition
The server SHALL accept a public GitHub repository, relative package directory and ref, resolve a full commit once, and read only bounded immutable Git tree/blob objects. It SHALL reject redirects, nonregular/executable entries, unsupported files, inconsistent Git blob hashes or decoded sizes, and unsupported package capabilities. It SHALL NOT use Git subprocesses, extract archives, send credentials/private context or execute package instructions.

#### Scenario: Branch moves during acquisition
- **WHEN** the upstream branch changes after resolution
- **THEN** every tree/blob request still refers to the original resolved object IDs and the installed digest identifies those exact bytes

### Requirement: External identity and activation isolation
External stable identity MUST include normalized repository and directory, preserve legacy curated run pins, and never replace a bundled twin by display-name collision. Preview SHALL NOT install or execute; activation SHALL revalidate current user authority and the expected per-user revision.

#### Scenario: Account changes while downloading
- **WHEN** an account is revoked during acquisition
- **THEN** late staging/activation is rejected and the next account never receives the old account response

#### Scenario: Same package name from two authors
- **WHEN** the user installs two repositories with the same SKILL name
- **THEN** their installation/version/enable state and pins remain separate and native capabilities do not expand

### Requirement: Reviewable one-action installation
The UI SHALL show the exact source/version, supported compatibility and missing connections before one deliberate install/update action. Source preview SHALL NOT claim safety certification or inherit an author's execution proof. Mail's existing final human approval remains required.

#### Scenario: User installs a previewed procedure
- **WHEN** the user reviews a source and chooses install or update
- **THEN** the exact previewed digest is activated for that user and appears in the existing suggestion cards without starting a task
- **AND** an older catalog response cannot hide the accepted installation or replace its controls
