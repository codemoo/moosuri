# Change: account-owned Google Calendar v1

## Why
F37 requires scoped Calendar reads and explicitly requested owner-primary writes with actual
provider receipts, ETags and truthful recovery. Gmail consent is not Calendar authorization.

## What changes
- Add independent per-owner Calendar OAuth consent/credentials using the configured Google app.
- Add bounded Calendar metadata/primary event reads and explicit direct form create/update/delete.
- Verify owner-primary and solo nonrecurring event eligibility; use ETags and suppress invitations/email.
- Persist exact request/dispatch/receipt and offer lookup-only unknown recovery without blind resend.
- Fence session/account/app/connection revisions and quarantine restored unfinished operations.

## Impact
F37 bounded primary-calendar slice; no agent, conversational or mission Calendar authority.
Reserved migration26 after missions25; isolated base24 uses version25 until root integration.
No registry dependency node/version, toolchain, CI or deployment expansion.
