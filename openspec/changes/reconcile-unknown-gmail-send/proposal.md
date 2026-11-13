# Read-only reconciliation of unknown Gmail sends

## Why

F15 and R21 require truthful external-work receipts. A committed send with a lost response cannot safely be retried. A bounded Sent lookup can provide additional evidence while preserving the unknown send outcome and final-preview approval boundary.

## What Changes

- Commit versioned frozen-message equality evidence before payload purge; old unknown rows retain explicit missing-oracle status.
- Derive separately leased read authority from the original dispatch and current owner/account/connection/readonly scope, without a browser session or send capability.
- Search fixed users/me Sent by original RFC Message-ID and inspect one complete unique raw candidate conservatively.
- Persist at most four automatic attempts at commitment +0/15/120/600 seconds, plus authenticated same-origin POST manual checks at most once per minute per action.
- Expose matching observation/last-check/candidate status without claiming a send response, recipient delivery or permission to resend. Quarantine restored lookup authority.

## Impact

Additive schema31; mail Store/service/connector/API/UI and focused synthetic tests. Existing Calendar30 migration/behavior and vault source remain unchanged. Gmail draft CRUD, compose consent and fixed scheduled send remain separate pending slices. No new model approve/send tool or OAuth scope.
