# Credential-broker foundation

2026-11-14. Bounded synthetic foundation accepted after independent review and Linux verification.

This is the P8/F69 foundation: exact account/device pairing and signatures, immutable
local grant ceilings, online commitment/revocation, original-identity recovery,
fixed nonsecret receipts, exclusive local ownership and fsynced crash state.
Schema37 quarantines restored enrollment and preserves historical signed receipts.
Default HTTP routes are absent; there is no production adapter or invocation mint.

Independent review found no material defect in the frozen protocol/storage source.
Native tests cover RFC8032/reference encoding, process lock/crash boundaries, replay,
revocation, fixed projection, signed HTTP, encrypted restore and existing ciphertext
authority. Native broker14/Store6/ciphertext4/HTTP1+1 and schema17/restore11 selections
passed; the latter groups overlap. Relevant Clippy, format and plan checks passed.

The first Linux broker suite passed13 tests (one additional ACL case is Darwin-only).
Its real encrypted-restore fixture initially failed because the shared temporary
folder helper relied on the host umask. The fixture now explicitly creates a0700
parent, as required by the existing restore contract. Production validation and all
restore assertions remain unchanged. The original failure is retained. The repaired Linux Store6, ciphertext authority4,
default-route-absence HTTP1, gated signed HTTP1, relevant all-target Clippy and
default embedded-web build passed. The independent source review remains applicable:
root changed only the test folder setup and preserved the earlier Calendar tests
while merging predecessor-schema fixture adjustments.

No UI behavior changed, so the prior accepted web evidence was retained. Formatting,
plan metadata, strict OpenSpec and public-source/new-history secret checks accompany
publication. Linux exercised the946-file integrated source plus14 public built assets;
this record is an additional documentation file.

No real credential, account, upstream provider or device was used. Device-local
unlock/provider adapters, first-connection and offline-waiting UX, independent signed
distribution and full P8 remain open. A coordinated rollback of all local files has
no external monotonic anchor in this unit. Mail's final-preview approval is unchanged.
No deployment, CI activation or original collector/service change is included.
