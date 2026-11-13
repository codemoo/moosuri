## ADDED Requirements

### Requirement: Collection opt-in bounds extraction scope and attribution
The system SHALL start disabled, process new source revisions only after an explicit
collection opt-in, and allow an explicit one-current-source request without hidden
historical backfill. It SHALL preserve reported source-speaker labels or an explicit
owner-assigned binding without inferring self or verified identity.

#### Scenario: Unknown or conflicting reported speaker
- **WHEN** a source has no usable single reported speaker or conflicts with its explicit binding
- **THEN** it is skipped with a fixed attribution reason and no memory is created

#### Scenario: Enable a collection with old content
- **WHEN** an owner enables extraction
- **THEN** only later committed revisions enqueue automatically; an old source requires its explicit action

### Requirement: Background extraction has independent bounded authority
The system SHALL issue a private grant/lease bound to tenant, account epoch,
collection policy, source revision and verified model binding, without fabricating
UserContext or deriving authority from source text. Daily calls, serialized input
bytes and reserved output tokens SHALL be durable and shared across retries.

#### Scenario: Logout or authority change
- **WHEN** an enabled owner logs out
- **THEN** the independent grant may continue; password/recovery, source withdrawal, collection or provider changes fence pending work and late results

#### Scenario: Ambiguous dispatch or budget exhaustion
- **WHEN** a dispatch acknowledgement is unknown or its budget is exhausted
- **THEN** unknown work is not automatically repeated and exhausted work waits with truthful status without claiming saved facts

### Requirement: Literal evidence and suppressions are checked at commit
The model SHALL select only complete bounded paragraph indexes. Store SHALL build
facts/attribution/evidence from pinned source bytes, validate provenance and commit
all selected memories with an idempotent receipt. Extraction SHALL NOT silently
merge or overwrite manual assertions.

#### Scenario: Stored instructions or malformed model output
- **WHEN** source text contains instructions or a model supplies fields outside index selection
- **THEN** text remains untrusted data and extra authority/invalid indexes cannot mutate memory; semantic fact eligibility remains separately evaluated

#### Scenario: Forgetting while work is pending
- **WHEN** an evidence-backed memory is forgotten or its source withdrawn
- **THEN** pending leases are fenced and exact source-quote suppression is checked before any later prompt and commit, preventing exact re-extraction under a new job or policy epoch

#### Scenario: An email uses CRLF paragraph separators
- **WHEN** complete paragraphs fit the individual and aggregate bounds although the whole email exceeds one paragraph limit
- **THEN** LF and CRLF separators retain exact source bytes and selected paragraphs produce exact evidence spans without truncating an oversized paragraph

#### Scenario: Source revision or selection namespace changes
- **WHEN** source eligibility, grant or knowledge generation changes after acquisition
- **THEN** a late selected result cannot commit or silently reinterpret indexes

### Requirement: User status reflects actual bounded results
The system SHALL expose explicit controls, committed memories with evidence/origin
and fixed pending/skipped/budget/failure observations without claiming all source
content or all historical material was processed.

#### Scenario: Zero facts or partial input
- **WHEN** a bounded call completes with no new facts or leaves oversized/uncovered paragraphs
- **THEN** it reports that result honestly and does not claim complete document understanding
