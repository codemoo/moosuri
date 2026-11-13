## ADDED Requirements

### Requirement: Local synthetic package template
The CLI SHALL create exactly SKILL.md, moosuri.toml, examples.json and LICENSE for `skills init <slug> [--output DIRECTORY]` in a new directory, using original synthetic normal/unsupported examples, existing capability names and standard Apache-2.0 text. It SHALL finish the bounded files before non-replacing publication and SHALL NOT open an installation, copy private data, install, execute or publish remotely.

#### Scenario: Create and check inside an ordinary repository
- **WHEN** a contributor supplies a valid slug and new destination with an existing repository parent
- **THEN** init creates the four-file compatible template without changing other paths or permissions
- **AND** a missing configured data directory remains absent

#### Scenario: Existing destination or unsafe path
- **WHEN** init encounters an existing destination, parent traversal, symlink component or installation-data ancestor
- **THEN** it fails without replacement, chmod or installation initialization
- **AND** cleanup preserves unknown paths and contents

### Requirement: Shared bounded compatibility check
The CLI SHALL check exactly four UTF-8 regular nonexecutable single-link files with nofollow reads, at most32KiB per file and64KiB total. It SHALL apply the same content compatibility rules as curated and GitHub installation and SHALL explain incompatible file/rules without echoing content. The result SHALL NOT imply safety, redistribution rights or successful task execution.

#### Scenario: Unsupported metadata or unsafe files
- **WHEN** a package includes an unknown capability/key, malformed frontmatter/examples/license, extra/missing file, oversized file, link, executable or special file
- **THEN** check fails without modifying package bytes or creating an installation

#### Scenario: Existing installation compatibility remains stable
- **WHEN** the authoring facade checks existing curated package content
- **THEN** its content acceptance matches the installer
- **AND** original package bytes, digests and pins remain unchanged

### Requirement: Contributor-controlled sharing
The CLI SHALL leave editing, review, commit and publishing to the contributor. It SHALL NOT run Git, scripts, runtime tasks, network acquisition, automatic publishing or installed-package edits. Existing public GitHub UI installation SHALL continue to activate reviewed immutable source bytes under current account authority.

#### Scenario: Check a local package
- **WHEN** a contributor checks their local source files
- **THEN** only compatibility is reported and no user installation or external action starts
