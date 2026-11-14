## ADDED Requirements

### Requirement: Independent finalized Japan Post observation proofs
The system SHALL learn a private structural Japan Post latest-summary route only from two distinct canonical current-request numbers on separate finalized owner runs with actual completed typed dispatches and final adoption of exact number/reported date/status/official URL. Learned metadata SHALL retain no number/status. not_found/provider_changed, generic success prose, prior-number fallback, same-input replay, incomplete runs and foreign-owner receipts SHALL NOT promote.

#### Scenario: Candidate then usable
- **WHEN** two independent direct current-number lookups return observed summaries adopted in their final answers
- **THEN** the owner's route becomes candidate then usable, retaining only the existing proof digests/run links.

### Requirement: Fresh current-request parcel execution
Usable private route execution SHALL require the exact frozen pin, foreground owner authority, current web-read/builtin parcel permissions and exactly one Japan Post number explicitly supplied in the current request. It SHALL perform fresh IO through the existing external dispatch lifecycle and store one private child receipt under the parent slot. It SHALL NOT replay training/history numbers/status or grant background, carrier/history or write extensions.

#### Scenario: Fresh C after A/B
- **WHEN** a new request supplies C after proofs A/B
- **THEN** the actual provider request uses C and its new observed result; same-slot replay does not resend or consume budget twice.

#### Scenario: Missing or mismatched current number
- **WHEN** the current request lacks a unique explicit Japan Post number or the invocation selects a different number
- **THEN** no external IO starts and no previous example supplies the missing authority.

### Requirement: Existing privacy and portable synthetic sharing
Existing owner isolation, deletion/revocation, proof revalidation, run budget and tombstone contracts SHALL apply. Export SHALL reconstruct only the fixed supported route with repository synthetic examples and SHALL exclude private numbers/status/trace/account/credential/proof content and automatic execution/publication authority.

#### Scenario: Withdrawn proof or procedure
- **WHEN** a proof conversation is deleted or the procedure is disabled/removed
- **THEN** readiness and actual derived output follow the existing invalidation contract; no late response revives it.
