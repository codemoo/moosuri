## Implementation
- [x] Add revisioned account status and opaque management receipts; adopt auth epochs and enforce login/session fencing.
- [x] Split password preparation from final delivery-fenced commit, preserving current-role and target-CAS checks.
- [x] Add existing-people-screen controls, one-time password display and same-request retry with account/response fencing.
- [x] Add meaningful account-isolation, lost-ACK, conflict, post-fsync failure and HTTP/React tests.

## Acceptance
- [x] Pass focused and affected platform/UI checks and document actual outcomes.
- [x] Independently review authentication, durable revocation, role and secret boundaries.
- [x] Integrate schema18 with artifacts16/schedules17; verify Linux and publish the coherent unit.
