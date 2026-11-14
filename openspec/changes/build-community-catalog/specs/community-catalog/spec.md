## ADDED Requirements

### Requirement: Verified static public catalog generation

The CLI SHALL generate deterministic bounded static package/catalog files only from an explicitly selected exact public GitHub commit and package directories, using the existing verified adapter, four-file compatibility checks and external v2 package format. It SHALL preserve curated pins and SHALL NOT accept local bytes with asserted source identity, read private installation/learning data, run Git/hooks/scripts, activate packages or publish remotely.

#### Scenario: Exact public snapshot

- **WHEN** an author selects an available full lowercase public commit and compatible package directories
- **THEN** sorted catalog and unchanged v2 envelope artifacts contain exact verified source/file identities and separate content/artifact digests, independent of input order or local changes

#### Scenario: Rejected or incomplete source

- **WHEN** a branch/tag, wrong commit, invalid file/layout/license/capability, unavailable source or acquisition bound is encountered
- **THEN** generation fails with bounded content-free diagnostics and publishes no incomplete output

### Requirement: Atomic local release output

The CLI SHALL publish a complete generated set only to a new ordinary directory via the safe authoring publication primitive, without opening installation state or overwriting prior output. It SHALL retain and recheck a pinned root/current-effective-UID-owned ancestor chain, reject group/world-writable non-sticky and foreign-owned sticky ancestors, and reject unsupported directory ACL grants/policies with bounded diagnostics. Root/same-UID operator mutations remain trusted.

#### Scenario: Partial failure or conflict

- **WHEN** acquisition/staging fails or the destination exists
- **THEN** prior output and installations are preserved and no partial catalog appears at the destination

#### Scenario: Explicit contributor workflow

- **WHEN** a contributor generates and reviews release files
- **THEN** remote publishing remains an explicit contributor action and current installation continues through verified public GitHub selection


#### Scenario: Mutable or foreign output ancestry

- **WHEN** an init/check/catalog path has a mutable non-sticky ancestor, foreign-owned ancestor even if sticky, or unsupported ACL policy
- **THEN** it is rejected before source acquisition or output creation without changing ancestor permissions or existing destinations

#### Scenario: Trusted sticky temporary ancestry

- **WHEN** a normal repository or root/current-user owned sticky temporary ancestor contains only trusted owned following directories with supported ACL policy
- **THEN** ordinary authoring and complete new-directory publication remain supported
