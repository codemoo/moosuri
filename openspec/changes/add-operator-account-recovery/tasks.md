## 1. Implementation
- [x] 1.1 Add stopped local recovery of an existing journal-locked account with explicit role and fresh temporary credential.
- [x] 1.2 Advance the durable auth epoch before atomic credential/session/head updates; preserve poison/replay behavior.
- [x] 1.3 Harden bounded owner-only password-file input without exposing secrets.

## 2. Verification
- [x] 2.1 Verify recovery, temporary exchange, concurrent attempts, failed commit/retry, lost response and other-account isolation.
- [x] 2.2 Verify CLI installation/lock/input boundaries and run format/clippy/regression checks.
- [x] 2.3 Independently review auth/storage boundaries and record actual platform results.

Encrypted archives, off-host latest journals, restore and Linux process-kill
acceptance remain separate work in complete-runtime-recovery-contracts.

Local synthetic evidence: [recovery acceptance record](../../../docs/reviews/2026-11-13-operator-account-recovery.md).
