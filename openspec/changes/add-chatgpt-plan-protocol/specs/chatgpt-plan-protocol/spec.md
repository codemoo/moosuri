## ADDED Requirements

### Requirement: Explicit personal-plan protocol foundation

The connector SHALL provide an opt-in ChatGPT plan route using only the fixed official public API origin, independently of shared provider configuration and OAuth login availability.

#### Scenario: Supported text request

- **WHEN** a caller constructs the plan client with a resolved credential and a valid exact model request
- **THEN** encoding SHALL send input array, store false and stream true, preserve instruction order using developer messages and omit unsupported fields including max_output_tokens and previous_response_id.

#### Scenario: Ordinary API regression

- **WHEN** an ordinary API client encodes text or flat function definitions
- **THEN** its existing encoding and checkpoint dialect SHALL remain unchanged.

### Requirement: Account catalog is distinct from capability proof

The connector SHALL parse bounded SIWC models/slug/display_name/visibility metadata, preserve visible account ordering and return explicit truncation without inferring tools, effort, media or login success.

#### Scenario: Wrong catalog dialect or malformed metadata

- **WHEN** the response uses ordinary data/id shape, missing SIWC fields, duplicate slugs or oversized metadata
- **THEN** parsing SHALL fail safely without API fallback, secret exposure or unbounded work.

### Requirement: Namespace and continuation identity

Plan tools SHALL require explicit capability and the fixed moosuri namespace. Complete calls and private replay history SHALL preserve namespace/name/call_id, exact model/effort and dialect through checkpoint restore and result submission.

#### Scenario: Fragmented two-round call

- **WHEN** a namespaced function call arrives in arbitrary UTF-8 fragments and its matching result is submitted after checkpoint restore
- **THEN** encoding and decoding SHALL retain exact call identity and raw continuation items without response-ID continuation.

#### Scenario: Namespace, call or dialect mismatch

- **WHEN** a namespace is foreign/missing, a name or ID changes, or a checkpoint from another dialect is supplied
- **THEN** the connector SHALL reject before dispatch or executable call release.

### Requirement: Truthful failure and bounded transport

The connector SHALL retain safe known subscription error categories and partial observations, bound all transport/parsing work, and perform no inference replay, retry or fallback. Incomplete and failed streams SHALL remain distinct from completed success.

#### Scenario: Usage limit after text

- **WHEN** a plan usage-limit failure arrives after streamed text or usage
- **THEN** those observations SHALL remain available, the error SHALL carry only its safe known category and no completed/executable result SHALL be produced.

#### Scenario: Trailing failure or missing terminal

- **WHEN** the stream is interrupted, lacks a terminal or includes contradictory data before clean HTTP EOF
- **THEN** no successful tool round SHALL be returned and plan text transport SHALL fail rather than silently accepting a terminal prefix.

#### Scenario: Failed usage or progress has no established exact identity

- **WHEN** failed-frame usage precedes response creation or omits/mismatches the established response ID or model
- **THEN** the plan connector SHALL fail without publishing that usage or a successful terminal/executable round; prior valid text SHALL remain a partial observation.
- **WHEN** a full progress/queued snapshot omits or contradicts the established response ID/model and a later terminal matches the original response
- **THEN** the contradictory stream SHALL remain failed rather than accepting that later terminal, on text and tool paths.
- **WHEN** an established response fails with explicit matching ID/model and valid usage
- **THEN** its sanitized error and attributed partial usage SHALL remain available without completed success.
