## ADDED Requirements

### Requirement: Literal source field separators

Source-bound calculation SHALL accept one colon, full-width colon or equals sign
between a unique current-request field label and a plain decimal in the source.

#### Scenario: Natural field request

- **WHEN** the user names `재료 단가` and the unique current source contains
  `재료 단가: 13.75`
- **THEN** calculation SHALL bind `13.75`, its exact source byte range and actual
  read/arithmetic receipts without requiring punctuation in the user request.

#### Scenario: Malformed or ambiguous field

- **WHEN** the label repeats, separators repeat, a line break or word separates
  label and value, or the token is an exponent/expression
- **THEN** the binder SHALL reject the field without guessing a value.

### Requirement: Accurate PDF extraction capability

The ordinary media-answer prompt SHALL distinguish supported supplied PDF text
from unsupported visual interpretation and unextracted content.

#### Scenario: Current or earlier selected PDF

- **WHEN** the current conversation supplies labelled PDF extraction
- **THEN** the assistant SHALL be instructed to use that text and extraction
  offsets while preserving untrusted-data and unavailable-effect boundaries.
